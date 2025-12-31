/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::filter_context::FilterContext;
use crate::filters::FilterReturn::Unchanged;
use crate::filters::topdown_traverse;
use crate::filters::{Filter, FilterReturn};
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::rawblock_to_config_value;
use crate::pandoc::{self, Block};
use crate::readers::qmd_error_messages::{produce_diagnostic_messages, produce_error_message_json};
use crate::traversals;
use crate::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
use quarto_parse_errors::TreeSitterLogObserverTrait;
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
    let mut log_observer = quarto_parse_errors::TreeSitterLogObserver::default();
    parser
        .parser
        .set_logger(Some(Box::new(|log_type, message| if log_type == LogType::Parse {
            log_observer.log(log_type, message);
        })));
    let _tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");
    produce_error_message_json(&log_observer)
}

pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    prune_errors: bool,
    parent_source_info: Option<quarto_source_map::SourceInfo>,
) -> Result<
    (
        pandoc::Pandoc,
        ASTContext,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ),
    Vec<quarto_error_reporting::DiagnosticMessage>,
> {
    let mut parser = MarkdownParser::default();
    let mut fast_log_observer = quarto_parse_errors::TreeSitterLogObserverFast::default();
    let mut log_observer = quarto_parse_errors::TreeSitterLogObserver::default();

    parser
        .parser
        .set_logger(Some(Box::new(|log_type, message| if log_type == LogType::Parse {
            fast_log_observer.log(log_type, message);
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
            prune_errors,
            parent_source_info,
        );
    }

    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    // Create ASTContext early so we can use it for error diagnostics
    let mut context = ASTContext::with_filename(filename.to_string());
    // Store parent source info for recursive parses
    context.parent_source_info = parent_source_info;
    // Add the input content to the SourceContext for proper error rendering
    let input_str = String::from_utf8_lossy(input_bytes).to_string();
    context.source_context = quarto_source_map::SourceContext::new();
    context
        .source_context
        .add_file(filename.to_string(), Some(input_str));

    if fast_log_observer.had_errors() {
        parser
            .parser
            .set_logger(Some(Box::new(|log_type, message| if log_type == LogType::Parse {
                log_observer.log(log_type, message);
            })));
        parser
            .parse(input_bytes, None)
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
            // Produce structured DiagnosticMessage objects with proper source locations
            let mut diagnostics = produce_diagnostic_messages(
                input_bytes,
                &log_observer,
                filename,
                &context.source_context,
            );

            // Prune diagnostics based on ERROR nodes if enabled
            if prune_errors {
                use crate::readers::qmd_error_messages::{
                    collect_error_node_ranges, get_outer_error_nodes,
                    prune_diagnostics_by_error_nodes,
                };

                let error_nodes = collect_error_node_ranges(&tree);
                let outer_nodes = get_outer_error_nodes(&error_nodes);
                diagnostics =
                    prune_diagnostics_by_error_nodes(diagnostics, &error_nodes, &outer_nodes);
            }

            return Err(diagnostics);
        }
    }

    let depth = crate::utils::concrete_tree_depth::concrete_tree_depth(&tree);
    // this is here mostly to prevent our fuzzer from blowing the stack
    // with a deeply nested document
    if depth > 100 {
        let diagnostic = quarto_error_reporting::generic_error!(format!(
            "The input document is too deeply nested (max depth: {} > 100).",
            depth
        ));
        return Err(vec![diagnostic]);
    }

    // Note: We no longer need to check parse_is_good(&tree) here because
    // the log_observer.had_errors() check above already catches parse errors
    // and produces better formatted diagnostics via produce_diagnostic_messages.
    // The old parse_is_good check was causing duplicate error messages.
    print_whole_tree(&mut tree.walk(), &mut output_stream);

    // Create diagnostic collector and convert to Pandoc AST
    let mut error_collector = DiagnosticCollector::new();
    let mut result = match pandoc::treesitter_to_pandoc(
        &mut output_stream,
        &tree,
        input_bytes,
        &context,
        &mut error_collector,
    ) {
        Ok(pandoc) => pandoc,
        Err(diagnostics) => {
            // Return diagnostics directly
            return Err(diagnostics);
        }
    };
    // Store ConfigMapEntry objects directly (Phase 5: no MetaValueWithSourceInfo conversion)
    let mut meta_from_parses: Vec<ConfigMapEntry> = Vec::new();
    // Track the source_info of the metadata block (for simple case with single block)
    let mut meta_source_info: Option<quarto_source_map::SourceInfo> = None;
    // Create a separate diagnostic collector for metadata parsing warnings
    let mut meta_diagnostics = DiagnosticCollector::new();

    result = {
        let mut filter = Filter::new().with_raw_block(|rb, _ctx| {
            if rb.format != "quarto_minus_metadata" {
                return Unchanged(rb);
            }
            // Phase 5: Work directly with ConfigValue, no MetaValueWithSourceInfo conversion
            // rawblock_to_config_value uses DocumentMetadata context, so strings are already
            // parsed as markdown (PandocInlines/PandocBlocks), not raw Scalar(String).
            let config_value = rawblock_to_config_value(&rb, &mut meta_diagnostics);

            // Check if this is lexical metadata directly on ConfigValue
            let is_lexical = config_value
                .get("_scope")
                .is_some_and(|v| v.is_string_value("lexical"));

            if is_lexical {
                // Lexical metadata - return as BlockMetadata
                // ConfigValue is already fully processed (strings parsed as markdown)
                FilterReturn::FilterResult(
                    vec![Block::BlockMetadata(MetaBlock {
                        meta: config_value,
                        source_info: rb.source_info.clone(),
                    })],
                    false,
                )
            } else {
                // Document-level metadata - extract entries and merge into meta_from_parses
                if let ConfigValueKind::Map(entries) = config_value.value {
                    // Store the source_info (for simple case with single metadata block)
                    if meta_source_info.is_none() {
                        meta_source_info = Some(config_value.source_info);
                    }
                    for entry in entries {
                        meta_from_parses.push(entry);
                    }
                }
                FilterReturn::FilterResult(vec![], false)
            }
        });
        let mut ctx = FilterContext::new();
        topdown_traverse(result, &mut filter, &mut ctx)
    };

    // Merge meta_from_parses into result.meta
    // Both are now ConfigMapEntry - no conversion needed
    if let ConfigValueKind::Map(ref mut entries) = result.meta.value {
        for entry in meta_from_parses {
            entries.push(entry);
        }
        // Update the overall metadata source_info if we captured one
        if let Some(captured_source_info) = meta_source_info {
            result.meta.source_info = captured_source_info;
        }
    }

    // Merge metadata diagnostics into main error_collector
    for diagnostic in meta_diagnostics.into_diagnostics() {
        error_collector.add(diagnostic);
    }

    // Collect all warnings
    let warnings = error_collector.into_diagnostics();

    Ok((result, context, warnings))
}
