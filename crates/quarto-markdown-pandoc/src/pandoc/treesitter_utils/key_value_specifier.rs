/*
 * key_value_specifier.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use std::collections::HashMap;
use std::io::Write;

/// Process key_value_specifier to build a HashMap of key-value pairs
pub fn process_key_value_specifier<T: Write>(
    buf: &mut T,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let mut spec = HashMap::new();
    let mut current_key: Option<String> = None;
    for (node, child) in children {
        if let PandocNativeIntermediate::IntermediateBaseText(value, _) = child {
            if node == "key_value_key" {
                current_key = Some(value);
            } else if node == "key_value_value" {
                if let Some(key) = current_key.take() {
                    spec.insert(key, value);
                } else {
                    panic!("Found key_value_value without a preceding key_value_key");
                }
            } else {
                writeln!(buf, "Unexpected key_value_specifier node: {}", node).unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateKeyValueSpec(spec)
}
