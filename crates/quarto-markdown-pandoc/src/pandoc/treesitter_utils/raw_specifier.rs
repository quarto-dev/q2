/*
 * raw_specifier.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::{convert_range, node_source_info_with_context};
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a raw format specifier, handling pandoc-reader format
pub fn process_raw_specifier(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // like code_content but skipping first character
    let raw = node.utf8_text(input_bytes).unwrap().to_string();
    if raw.chars().nth(0) == Some('<') {
        PandocNativeIntermediate::IntermediateBaseText(
            "pandoc-reader:".to_string() + &raw[1..],
            convert_range(&node_source_info_with_context(node, context).range),
        )
    } else {
        PandocNativeIntermediate::IntermediateBaseText(
            raw[1..].to_string(),
            convert_range(&node_source_info_with_context(node, context).range),
        )
    }
}
