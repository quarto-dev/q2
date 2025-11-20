/*
 * link_title.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a link title by removing surrounding quotes
pub fn process_link_title(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let title = node.utf8_text(input_bytes).unwrap().to_string();
    let title = title[1..title.len() - 1].to_string();
    let source_info = node_source_info_with_context(node, context);
    let range = crate::pandoc::location::source_info_to_qsm_range_or_fallback(
        &source_info,
        context,
    );
    PandocNativeIntermediate::IntermediateBaseText(title, range)
}
