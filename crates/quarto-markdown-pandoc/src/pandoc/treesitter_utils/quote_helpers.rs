/*
 * quote_helpers.rs
 *
 * Functions for processing quoted text nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, QuoteType, Quoted};
use crate::pandoc::location::node_source_info_with_context;

/// Process quoted text (single or double quotes)
pub fn process_quoted(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    quote_type: QuoteType,
    delimiter_name: &str,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            _ if node_name == delimiter_name => {} // Skip delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateInline(Inline::Quoted(Quoted {
        quote_type,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
