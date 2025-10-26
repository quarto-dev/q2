/*
 * key_value_specifier.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use quarto_source_map::Range;
use std::io::Write;

/// Process key_value_specifier to build a Vec of key-value pairs with their source locations
pub fn process_key_value_specifier<T: Write>(
    buf: &mut T,
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut spec = Vec::new();
    let mut current_key: Option<(String, Range)> = None;
    for (node, child) in children {
        if let PandocNativeIntermediate::IntermediateBaseText(value, range) = child {
            if node == "key_value_key" {
                current_key = Some((value, range));
            } else if node == "key_value_value" {
                if let Some((key, key_range)) = current_key.take() {
                    spec.push((key, value, key_range, range));
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
