/*
 * raw_specifier.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a raw format specifier, handling pandoc-reader format
pub fn process_raw_specifier(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ParseContext,
) -> PandocNativeIntermediate {
    // like code_content but skipping first character
    let raw = node.utf8_text(input_bytes).unwrap().to_string();
    if raw.chars().nth(0) == Some('<') {
        PandocNativeIntermediate::IntermediateBaseText(
            "pandoc-reader:".to_string() + &raw[1..],
            node_source_info_with_context(node, context).range,
        )
    } else {
        PandocNativeIntermediate::IntermediateBaseText(
            raw[1..].to_string(),
            node_source_info_with_context(node, context).range,
        )
    }
}
