/*
 * inline_link.rs
 *
 * Functions for processing inline link nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, is_empty_attr};
use crate::pandoc::inline::{Inline, is_empty_target, make_cite_inline, make_span_inline};
use crate::pandoc::location::node_source_info_with_context;
use std::collections::HashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_inline_link<T: Write, F>(
    node: &tree_sitter::Node,
    link_buf: &mut T,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    let mut target = ("".to_string(), "".to_string());
    let mut content: Vec<Inline> = Vec::new();

    for (node, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateRawFormat(_, _) => {
                // TODO show position of this error
                let _ = writeln!(
                    link_buf,
                    "Raw attribute specifiers are unsupported in links and spans: {}. Ignoring.",
                    node_text()
                );
            }
            PandocNativeIntermediate::IntermediateAttr(a) => attr = a,
            PandocNativeIntermediate::IntermediateBaseText(text, _) => {
                if node == "link_destination" {
                    target.0 = text; // URL
                } else if node == "link_title" {
                    target.1 = text; // Title
                } else if node == "language_attribute" {
                    // TODO show position of this error
                    let _ = writeln!(
                        link_buf,
                        "Language specifiers are unsupported in links and spans: {}. Ignoring.",
                        node_text()
                    );
                } else {
                    panic!("Unexpected inline_link node: {}", node);
                }
            }
            PandocNativeIntermediate::IntermediateUnknown(_) => {}
            PandocNativeIntermediate::IntermediateInlines(inlines) => content.extend(inlines),
            PandocNativeIntermediate::IntermediateInline(inline) => content.push(inline),
            _ => panic!("Unexpected child in inline_link: {:?}", child),
        }
    }
    let has_citations = content
        .iter()
        .any(|inline| matches!(inline, Inline::Cite(_)));

    // an inline link might be a Cite if it has citations, no destination, and no title
    // and no attributes
    let is_cite = has_citations && is_empty_target(&target) && is_empty_attr(&attr);

    PandocNativeIntermediate::IntermediateInline(if is_cite {
        make_cite_inline(
            attr,
            target,
            content,
            crate::pandoc::source_map_compat::node_to_source_info_with_context(node, context),
        )
    } else {
        make_span_inline(
            attr,
            target,
            content,
            crate::pandoc::source_map_compat::node_to_source_info_with_context(node, context),
        )
    })
}
