/*
 * uri_autolink.rs
 *
 * Functions for processing URI autolink nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, Link, Space, Str};
use crate::pandoc::location::node_location;
use hashlink::LinkedHashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_uri_autolink(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // The tree-sitter scanner may include leading/trailing whitespace in the autolink token
    // because it consumes whitespace for indentation calculation before lexing inline tokens.
    // We need to split the token into separate Space nodes and the actual autolink.

    // Get the full node range (may include leading/trailing whitespace)
    let node_range = node_location(node);

    // Extract the full text from the node range
    let text = &input_bytes[node_range.start.offset..node_range.end.offset];
    let text_str = std::str::from_utf8(text).unwrap();

    // Count leading whitespace characters
    let leading_ws_count = text_str.chars().take_while(|c| c.is_whitespace()).count();

    // Count trailing whitespace characters
    let trailing_ws_count = text_str
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();

    // Extract the actual autolink text (trimmed)
    let autolink_text = text_str.trim();

    // Validate it's a proper autolink with angle brackets
    if autolink_text.len() < 2 || !autolink_text.starts_with('<') || !autolink_text.ends_with('>') {
        panic!("Invalid URI autolink: {}", autolink_text);
    }

    // Extract the URL (remove angle brackets)
    let url = &autolink_text[1..autolink_text.len() - 1];

    // Calculate range for leading space (if present)
    let leading_space_range = if leading_ws_count > 0 {
        Some(quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node_range.start.offset,
                row: node_range.start.row,
                column: node_range.start.column,
            },
            end: quarto_source_map::Location {
                offset: node_range.start.offset + leading_ws_count,
                row: node_range.start.row,
                column: node_range.start.column + leading_ws_count,
            },
        })
    } else {
        None
    };

    // Calculate range for the autolink itself (excluding whitespace)
    let autolink_range = quarto_source_map::Range {
        start: quarto_source_map::Location {
            offset: node_range.start.offset + leading_ws_count,
            row: node_range.start.row,
            column: node_range.start.column + leading_ws_count,
        },
        end: quarto_source_map::Location {
            offset: node_range.end.offset - trailing_ws_count,
            row: node_range.end.row,
            column: node_range.end.column - trailing_ws_count,
        },
    };

    // Calculate range for trailing space (if present)
    let trailing_space_range = if trailing_ws_count > 0 {
        Some(quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node_range.end.offset - trailing_ws_count,
                row: node_range.end.row,
                column: node_range.end.column - trailing_ws_count,
            },
            end: quarto_source_map::Location {
                offset: node_range.end.offset,
                row: node_range.end.row,
                column: node_range.end.column,
            },
        })
    } else {
        None
    };

    // Build the result with separate nodes for spaces and autolink
    let mut result = Vec::new();

    // Add leading space if present
    if let Some(space_range) = leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    // Add the autolink as a Link node
    let mut attr = (String::new(), vec![], LinkedHashMap::new());
    attr.1.push("uri".to_string()); // Pandoc adds the "uri" class to autolinks

    result.push(Inline::Link(Link {
        content: vec![Inline::Str(Str {
            text: url.to_string(),
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                autolink_range.clone(),
            ),
        })],
        attr,
        target: (url.to_string(), String::new()),
        source_info: quarto_source_map::SourceInfo::from_range(
            context.current_file_id(),
            autolink_range,
        ),
        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
    }));

    // Add trailing space if present
    if let Some(space_range) = trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    // Return as IntermediateInlines (multiple nodes) instead of single IntermediateInline
    PandocNativeIntermediate::IntermediateInlines(result)
}
