/*
 * quote_helpers.rs
 *
 * Functions for processing quoted text nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, QuoteType, Quoted, Space};
use crate::pandoc::location::node_source_info_with_context;

/// Process quoted text (single or double quotes)
pub fn process_quoted(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    quote_type: QuoteType,
    delimiter_name: &str,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();

    // Scan delimiters to check for captured spaces and save their ranges
    let mut leading_space_range: Option<quarto_source_map::Range> = None;
    let mut trailing_space_range: Option<quarto_source_map::Range> = None;
    let mut first_delimiter = true;

    for (node_name, child) in &children {
        if node_name == delimiter_name
            && let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                let text = std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                    .unwrap();

                if first_delimiter {
                    // Opening delimiter - check for leading space
                    if text.starts_with(char::is_whitespace) {
                        // Count leading whitespace characters
                        let leading_ws_count =
                            text.chars().take_while(|c| c.is_whitespace()).count();
                        // Calculate the range for just the leading whitespace
                        let ws_end_offset = range.start.offset + leading_ws_count;
                        leading_space_range = Some(quarto_source_map::Range {
                            start: quarto_source_map::Location {
                                offset: range.start.offset,
                                row: range.start.row,
                                column: range.start.column,
                            },
                            end: quarto_source_map::Location {
                                offset: ws_end_offset,
                                row: range.start.row,
                                column: range.start.column + leading_ws_count,
                            },
                        });
                    }
                    first_delimiter = false;
                } else {
                    // Closing delimiter - check for trailing space
                    if text.ends_with(char::is_whitespace) {
                        // Count trailing whitespace characters
                        let trailing_ws_count =
                            text.chars().rev().take_while(|c| c.is_whitespace()).count();
                        // Calculate the range for just the trailing whitespace
                        let ws_start_offset = range.end.offset - trailing_ws_count;
                        trailing_space_range = Some(quarto_source_map::Range {
                            start: quarto_source_map::Location {
                                offset: ws_start_offset,
                                row: range.end.row,
                                column: range.end.column - trailing_ws_count,
                            },
                            end: quarto_source_map::Location {
                                offset: range.end.offset,
                                row: range.end.row,
                                column: range.end.column,
                            },
                        });
                    }
                }
            }
    }

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            _ if node_name == delimiter_name => {} // Skip delimiters
            _ => {}
        }
    }

    let quoted_inline = Inline::Quoted(Quoted {
        quote_type,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    });

    // Build result with injected Space nodes as needed
    let mut result = Vec::new();

    if let Some(space_range) = leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    result.push(quoted_inline);

    if let Some(space_range) = trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
