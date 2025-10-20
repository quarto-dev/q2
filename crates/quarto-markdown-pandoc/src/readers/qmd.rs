/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::errors;
use crate::errors::parse_is_good;
use crate::filters::FilterReturn::Unchanged;
use crate::filters::topdown_traverse;
use crate::filters::{Filter, FilterReturn};
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::location::{SourceInfo, extract_filename_index, convert_range};
use crate::pandoc::meta::parse_metadata_strings_with_source_info;
use crate::pandoc::rawblock_to_meta_with_source_info;
use crate::pandoc::{self, Block, MetaValueWithSourceInfo};
use crate::readers::qmd_error_messages::{produce_error_message, produce_error_message_json};
use crate::traversals;
use crate::utils::diagnostic_collector::DiagnosticCollector;
use std::io::Write;
use tree_sitter::LogType;
use tree_sitter_qmd::MarkdownParser;

fn print_whole_tree<T: Write>(cursor: &mut tree_sitter_qmd::MarkdownCursor, buf: &mut T) {
    let mut depth = 0;
    traversals::topdown_traverse_concrete_tree(cursor, &mut |node, phase| {
        if phase == traversals::TraversePhase::Enter {
            writeln!(buf, "{}{}: {:?}", "  ".repeat(depth), node.kind(), node).unwrap();
            depth += 1;
        } else {
            depth -= 1;
        }
        true // continue traversing
    });
}

pub fn read_bad_qmd_for_error_message(input_bytes: &[u8]) -> Vec<String> {
    let mut parser = MarkdownParser::default();
    let mut log_observer = crate::utils::tree_sitter_log_observer::TreeSitterLogObserver::default();
    parser
        .parser
        .set_logger(Some(Box::new(|log_type, message| match log_type {
            LogType::Parse => {
                log_observer.log(log_type, message);
            }
            _ => {}
        })));
    let _tree = parser
        .parse(&input_bytes, None)
        .expect("Failed to parse input");
    return produce_error_message_json(&log_observer);
}

pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
where
    F: Fn(
        &[u8],
        &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
        &str,
    ) -> Vec<String>,
{
    let mut parser = MarkdownParser::default();
    let mut error_messages: Vec<String> = Vec::new();

    let mut log_observer = crate::utils::tree_sitter_log_observer::TreeSitterLogObserver::default();
    parser
        .parser
        .set_logger(Some(Box::new(|log_type, message| match log_type {
            LogType::Parse => {
                log_observer.log(log_type, message);
            }
            _ => {}
        })));

    // inefficient, but safe: if no trailing newline, add one
    if !input_bytes.ends_with(b"\n") {
        let mut input_bytes_with_newline = Vec::with_capacity(input_bytes.len() + 1);
        input_bytes_with_newline.extend_from_slice(input_bytes);
        input_bytes_with_newline.push(b'\n');
        return read(
            &input_bytes_with_newline,
            _loose,
            filename,
            output_stream,
            error_formatter,
        );
    }

    let tree = parser
        .parse(&input_bytes, None)
        .expect("Failed to parse input");
    log_observer.parses.iter().for_each(|parse| {
        writeln!(output_stream, "tree-sitter parse:").unwrap();
        parse
            .messages
            .iter()
            .for_each(|msg| writeln!(output_stream, "  {}", msg).unwrap());
        writeln!(output_stream, "---").unwrap();
    });
    if log_observer.had_errors() {
        if let Some(formatter) = error_formatter {
            // Use the provided error formatter
            return Err(formatter(input_bytes, &log_observer, filename));
        } else {
            // Use the default ariadne formatter
            return Err(produce_error_message(input_bytes, &log_observer, filename));
        }
    }

    let depth = crate::utils::concrete_tree_depth::concrete_tree_depth(&tree);
    // this is here mostly to prevent our fuzzer from blowing the stack
    // with a deeply nested document
    if depth > 100 {
        error_messages.push(format!(
            "The input document is too deeply nested (max depth: {} > 100).",
            depth
        ));
        return Err(error_messages);
    }

    let errors = parse_is_good(&tree);
    print_whole_tree(&mut tree.walk(), &mut output_stream);
    if !errors.is_empty() {
        let mut cursor = tree.walk();
        for error in errors {
            cursor.goto_id(error);
            error_messages.push(errors::error_message(&mut cursor, &input_bytes));
        }
    }
    if !error_messages.is_empty() {
        return Err(error_messages);
    }

    let context = ASTContext::with_filename(filename.to_string());

    // Create diagnostic collector and convert to Pandoc AST
    let mut error_collector = DiagnosticCollector::new();
    let mut result = match pandoc::treesitter_to_pandoc(
        &mut output_stream,
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    ) {
        Ok(pandoc) => pandoc,
        Err(diagnostics) => {
            // Convert diagnostics to strings based on format
            if error_formatter.is_some() {
                return Err(diagnostics
                    .iter()
                    .map(|d| d.to_json().to_string())
                    .collect());
            } else {
                return Err(diagnostics.iter().map(|d| d.to_text(None)).collect());
            }
        }
    };

    // Output warnings to stderr in appropriate format
    if error_formatter.is_some() {
        // JSON format
        let warnings = error_collector.to_json();
        for warning in warnings {
            eprintln!("{}", warning);
        }
    } else {
        // Text format (default)
        let warnings = error_collector.to_text();
        for warning in warnings {
            eprintln!("{}", warning);
        }
    }
    // Store complete MetaMapEntry objects to preserve key_source information
    let mut meta_from_parses: Vec<crate::pandoc::meta::MetaMapEntry> = Vec::new();

    result = {
        let mut filter = Filter::new().with_raw_block(|rb| {
            if rb.format != "quarto_minus_metadata" {
                return Unchanged(rb);
            }
            let filename_index = extract_filename_index(&rb.source_info);
            let range = convert_range(&rb.source_info.range);

            // Use new rawblock_to_meta_with_source_info - preserves source info!
            let meta_with_source = rawblock_to_meta_with_source_info(&rb, &context);

            // Check if this is lexical metadata
            let is_lexical =
                if let MetaValueWithSourceInfo::MetaMap { ref entries, .. } = meta_with_source {
                    entries.iter().any(|e| {
                    e.key == "_scope"
                        && matches!(
                            &e.value,
                            MetaValueWithSourceInfo::MetaString { value, .. } if value == "lexical"
                        )
                })
                } else {
                    false
                };

            if is_lexical {
                // Lexical metadata - parse strings and return as BlockMetadata
                let mut inner_meta_from_parses = Vec::new();
                let parsed_meta = parse_metadata_strings_with_source_info(
                    meta_with_source,
                    &mut inner_meta_from_parses,
                );

                // Merge inner metadata if needed
                let final_meta = if let MetaValueWithSourceInfo::MetaMap {
                    mut entries,
                    source_info,
                } = parsed_meta
                {
                    // Now inner_meta_from_parses preserves full MetaMapEntry with key_source
                    for entry in inner_meta_from_parses {
                        entries.push(entry);
                    }
                    MetaValueWithSourceInfo::MetaMap {
                        entries,
                        source_info,
                    }
                } else {
                    parsed_meta
                };

                return FilterReturn::FilterResult(
                    vec![Block::BlockMetadata(MetaBlock {
                        meta: final_meta,
                        source_info: SourceInfo::new(filename_index, range).to_source_map_info(),
                    })],
                    false,
                );
            } else {
                // Document-level metadata - parse strings and merge into meta_from_parses
                let mut inner_meta = Vec::new();
                let parsed_meta =
                    parse_metadata_strings_with_source_info(meta_with_source, &mut inner_meta);

                // Extract MetaMapEntry objects (preserving key_source) and store them
                if let MetaValueWithSourceInfo::MetaMap { entries, .. } = parsed_meta {
                    for entry in entries {
                        meta_from_parses.push(entry);
                    }
                }
                // Also add any inner metadata entries (now preserves key_source)
                for entry in inner_meta {
                    meta_from_parses.push(entry);
                }
                return FilterReturn::FilterResult(vec![], false);
            }
        });
        topdown_traverse(result, &mut filter)
    };

    // Merge meta_from_parses into result.meta
    // result.meta is MetaValueWithSourceInfo::MetaMap, so we need to append entries
    // Now meta_from_parses contains complete MetaMapEntry objects with key_source preserved
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &mut result.meta {
        for entry in meta_from_parses.into_iter() {
            entries.push(entry);
        }
    }
    Ok((result, context))
}
