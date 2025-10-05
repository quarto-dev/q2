/*
 * backslash_escape.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_location;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a backslash escape by removing the leading backslash
pub fn process_backslash_escape(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    // This is a backslash escape, we need to extract the content
    // by removing the backslash
    let text = node.utf8_text(input_bytes).unwrap();
    if text.len() < 2 || !text.starts_with('\\') {
        panic!("Invalid backslash escape: {}", text);
    }
    let content = &text[1..]; // remove the leading backslash
    PandocNativeIntermediate::IntermediateBaseText(content.to_string(), node_location(node))
}
