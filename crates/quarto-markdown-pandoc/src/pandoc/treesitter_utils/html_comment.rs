/*
 * html_comment.rs
 *
 * Functions for processing HTML comment nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, RawInline};
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_html_comment(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // Get the full text of the HTML comment (including <!-- and -->)
    let text = node.utf8_text(input_bytes).unwrap().to_string();

    // Create a RawInline with format "quarto-html-comment"
    PandocNativeIntermediate::IntermediateInline(Inline::RawInline(RawInline {
        format: "quarto-html-comment".to_string(),
        text,
        source_info: node_source_info_with_context(node, context),
    }))
}
