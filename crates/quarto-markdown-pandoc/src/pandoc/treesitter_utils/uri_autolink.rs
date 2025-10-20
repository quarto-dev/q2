/*
 * uri_autolink.rs
 *
 * Functions for processing URI autolink nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, Link, Str};
use crate::pandoc::source_map_compat;
use std::collections::HashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_uri_autolink(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // This is a URI autolink, we need to extract the content
    // by removing the angle brackets
    let text = node.utf8_text(input_bytes).unwrap();
    if text.len() < 2 || !text.starts_with('<') || !text.ends_with('>') {
        panic!("Invalid URI autolink: {}", text);
    }
    let content = &text[1..text.len() - 1]; // remove the angle brackets
    let mut attr = ("".to_string(), vec![], HashMap::new());
    // pandoc adds the class "uri" to autolinks
    attr.1.push("uri".to_string());
    PandocNativeIntermediate::IntermediateInline(Inline::Link(Link {
        content: vec![Inline::Str(Str {
            text: content.to_string(),
            source_info: source_map_compat::node_to_source_info_with_context(
                node, context,
            ),
        })],
        attr,
        target: (content.to_string(), "".to_string()),
        source_info: source_map_compat::node_to_source_info_with_context(node, context),
    }))
}
