/*
 * shortcode.rs
 *
 * Functions for processing shortcode-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::inline::Inline;
use crate::pandoc::location::node_location;
use crate::pandoc::shortcode::{Shortcode, ShortcodeArg};
use std::collections::HashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

// Helper function to process shortcode_naked_string and shortcode_name nodes
pub fn process_shortcode_string_arg(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let id = node.utf8_text(input_bytes).unwrap().to_string();
    PandocNativeIntermediate::IntermediateShortcodeArg(
        ShortcodeArg::String(id),
        node_location(node),
    )
}

// Helper function to process shortcode_string nodes
pub fn process_shortcode_string(
    extract_quoted_text_fn: &dyn Fn() -> PandocNativeIntermediate,
    node: &tree_sitter::Node,
) -> PandocNativeIntermediate {
    let PandocNativeIntermediate::IntermediateBaseText(id, _) = extract_quoted_text_fn() else {
        panic!(
            "Expected BaseText in shortcode_string, got {:?}",
            extract_quoted_text_fn()
        )
    };
    PandocNativeIntermediate::IntermediateShortcodeArg(
        ShortcodeArg::String(id),
        node_location(node),
    )
}

pub fn process_shortcode_keyword_param<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut result = HashMap::new();
    let mut name = String::new();
    for (node, child) in children {
        match node.as_str() {
            "shortcode_name" => {
                let PandocNativeIntermediate::IntermediateShortcodeArg(
                    ShortcodeArg::String(text),
                    _,
                ) = child
                else {
                    panic!("Expected BaseText in shortcode_name, got {:?}", child)
                };
                if name.is_empty() {
                    name = text;
                } else {
                    result.insert(name.clone(), ShortcodeArg::String(text));
                }
            }
            "shortcode_string"
            | "shortcode_number"
            | "shortcode_naked_string"
            | "shortcode_boolean" => {
                let PandocNativeIntermediate::IntermediateShortcodeArg(arg, _) = child else {
                    panic!("Expected ShortcodeArg in shortcode_string, got {:?}", child)
                };
                result.insert(name.clone(), arg);
            }
            "block_continuation" => {
                // This is a marker node, we don't need to do anything with it
            }
            _ => {
                writeln!(buf, "Warning: Unhandled node kind: {}", node).unwrap();
            }
        }
    }
    let range = node_location(node);
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::KeyValue(result), range)
}

pub fn process_shortcode(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let is_escaped = node.kind() == "shortcode_escaped";
    let mut name = String::new();
    let mut positional_args: Vec<ShortcodeArg> = Vec::new();
    let mut keyword_args: HashMap<String, ShortcodeArg> = HashMap::new();
    for (node, child) in children {
        match (node.as_str(), child) {
            (
                "shortcode_naked_string",
                PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(text), _),
            )
            | (
                "shortcode_name",
                PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(text), _),
            )
            | (
                "shortcode_string",
                PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(text), _),
            ) => {
                if name.is_empty() {
                    name = text;
                } else {
                    positional_args.push(ShortcodeArg::String(text));
                }
            }
            (
                "shortcode_keyword_param",
                PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::KeyValue(spec), _),
            ) => {
                for (key, value) in spec {
                    keyword_args.insert(key, value);
                }
            }
            ("shortcode", PandocNativeIntermediate::IntermediateInline(Inline::Shortcode(arg))) => {
                positional_args.push(ShortcodeArg::Shortcode(arg));
            }
            ("shortcode_number", PandocNativeIntermediate::IntermediateShortcodeArg(arg, _))
            | ("shortcode_boolean", PandocNativeIntermediate::IntermediateShortcodeArg(arg, _)) => {
                positional_args.push(arg);
            }
            ("shortcode_delimiter", _) => {
                // This is a marker node, we don't need to do anything with it
            }
            (child_type, child) => panic!(
                "Unexpected node in {:?}: {:?} {:?}",
                node,
                child_type,
                child.clone()
            ),
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::Shortcode(Shortcode {
        is_escaped,
        name,
        positional_args,
        keyword_args,
    }))
}

pub fn process_shortcode_boolean(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let value = node.utf8_text(input_bytes).unwrap();
    let value = match value {
        "true" => ShortcodeArg::Boolean(true),
        "false" => ShortcodeArg::Boolean(false),
        _ => panic!("Unexpected shortcode_boolean value: {}", value),
    };
    let range = node_location(node);
    PandocNativeIntermediate::IntermediateShortcodeArg(value, range)
}

pub fn process_shortcode_number(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let value = node.utf8_text(input_bytes).unwrap();
    let range = node_location(node);
    let Ok(num) = value.parse::<f64>() else {
        panic!("Invalid shortcode_number: {}", value)
    };
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::Number(num), range)
}
