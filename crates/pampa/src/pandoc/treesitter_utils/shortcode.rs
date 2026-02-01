/*
 * shortcode.rs
 *
 * Functions for processing shortcode-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::{Inline, Shortcode, ShortcodeArg, Space};
use quarto_source_map::SourceInfo;
use std::collections::HashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

// Helper function to process shortcode_naked_string and shortcode_name nodes
pub fn process_shortcode_string_arg(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let id = node.utf8_text(input_bytes).unwrap().to_string();
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(id), range)
}

// Helper function to process shortcode_string nodes
pub fn process_shortcode_string(
    extract_quoted_text_fn: &dyn Fn() -> PandocNativeIntermediate,
    node: &tree_sitter::Node,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let PandocNativeIntermediate::IntermediateBaseText(id, _) = extract_quoted_text_fn() else {
        panic!(
            "Expected BaseText in shortcode_string, got {:?}",
            extract_quoted_text_fn()
        )
    };
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(id), range)
}

pub fn process_shortcode(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let is_escaped = node.kind() == "shortcode_escaped";

    // Check for leading whitespace (tree-sitter scanner may include it in the node)
    let text = node.utf8_text(input_bytes).unwrap();
    let has_leading_space = text.starts_with(char::is_whitespace);

    // Calculate the number of leading whitespace bytes
    let leading_space_len = text.len() - text.trim_start().len();

    // Calculate source info for the shortcode (excluding leading space if present)
    let source_info = if has_leading_space {
        let shortcode_start_byte = node.start_byte() + leading_space_len;
        let shortcode_start_pos = node.start_position();

        let shortcode_range = quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: shortcode_start_byte,
                row: shortcode_start_pos.row,
                column: shortcode_start_pos.column + leading_space_len,
            },
            end: quarto_source_map::Location {
                offset: node.end_byte(),
                row: node.end_position().row,
                column: node.end_position().column,
            },
        };
        SourceInfo::from_range(context.current_file_id(), shortcode_range)
    } else {
        node_source_info_with_context(node, context)
    };

    let mut name = String::new();
    let mut positional_args: Vec<ShortcodeArg> = Vec::new();
    let mut keyword_args: HashMap<String, ShortcodeArg> = HashMap::new();
    for (child_node, child) in children {
        match (child_node.as_str(), child) {
            (
                "shortcode_naked_string" | "shortcode_name" | "shortcode_string",
                PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(text), _),
            ) => {
                if name.is_empty() {
                    name = text;
                } else {
                    positional_args.push(ShortcodeArg::String(text));
                }
            }
            ("shortcode", PandocNativeIntermediate::IntermediateInline(Inline::Shortcode(arg))) => {
                positional_args.push(ShortcodeArg::Shortcode(arg));
            }
            ("shortcode_number", PandocNativeIntermediate::IntermediateShortcodeArg(arg, _)) => {
                positional_args.push(arg);
            }
            ("key_value_specifier", PandocNativeIntermediate::IntermediateKeyValueSpec(specs)) => {
                // Handle key-value pairs from key_value_specifier node
                for (key, value, _, _) in specs {
                    keyword_args.insert(key, ShortcodeArg::String(value));
                }
            }
            ("shortcode_delimiter", _) => {
                // This is a marker node, we don't need to do anything with it
            }
            _ => {
                // Skip unknown node types (shouldn't happen in practice)
            }
        }
    }

    let shortcode = Inline::Shortcode(Shortcode {
        is_escaped,
        name,
        positional_args,
        keyword_args,
        source_info,
    });

    // Return with leading Space if the scanner included whitespace in the node
    if has_leading_space {
        let space_range = quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node.start_byte(),
                row: node.start_position().row,
                column: node.start_position().column,
            },
            end: quarto_source_map::Location {
                offset: node.start_byte() + leading_space_len,
                row: node.start_position().row,
                column: node.start_position().column + leading_space_len,
            },
        };
        PandocNativeIntermediate::IntermediateInlines(vec![
            Inline::Space(Space {
                source_info: SourceInfo::from_range(context.current_file_id(), space_range),
            }),
            shortcode,
        ])
    } else {
        PandocNativeIntermediate::IntermediateInline(shortcode)
    }
}

pub fn process_shortcode_number(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let value = node.utf8_text(input_bytes).unwrap();
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    let Ok(num) = value.parse::<f64>() else {
        panic!("Invalid shortcode_number: {}", value)
    };
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::Number(num), range)
}
