/*
 * backslash_escape.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a backslash escape by removing the leading backslash
pub fn process_backslash_escape(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // This is a backslash escape, we need to extract the content
    // by removing the backslash
    let text = node.utf8_text(input_bytes).unwrap();
    if text.len() < 2 || !text.starts_with('\\') {
        panic!("Invalid backslash escape: {}", text);
    }
    let content = &text[1..]; // remove the leading backslash
    let source_info = node_source_info_with_context(node, context);
    let range = crate::pandoc::location::source_info_to_qsm_range_or_fallback(
        &source_info,
        context,
    );
    PandocNativeIntermediate::IntermediateBaseText(content.to_string(), range)
}
