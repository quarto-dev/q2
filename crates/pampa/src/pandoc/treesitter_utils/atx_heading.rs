/*
 * atx_heading.rs
 *
 * Functions for processing ATX heading nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, AttrSourceInfo};
use crate::pandoc::block::{Block, Header};
use crate::pandoc::inline::Inline;
use crate::pandoc::location::node_source_info_with_context;
use hashlink::LinkedHashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_atx_heading(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut level = 0;
    let mut content: Vec<Inline> = Vec::new();
    let mut attr: Attr = (String::new(), vec![], LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_kind, child) in children {
        match node_kind.as_str() {
            "block_continuation" => continue,
            "atx_h1_marker" => level = 1,
            "atx_h2_marker" => level = 2,
            "atx_h3_marker" => level = 3,
            "atx_h4_marker" => level = 4,
            "atx_h5_marker" => level = 5,
            "atx_h6_marker" => level = 6,
            "attribute" | "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(inner_attr, inner_attr_source) =
                    child
                {
                    attr = inner_attr;
                    attr_source = inner_attr_source;
                }
                // Skip non-Attr intermediates (shouldn't happen in practice)
            }
            _ => {
                // Inline content directly as children (pandoc_str, pandoc_emph, etc.)
                match child {
                    PandocNativeIntermediate::IntermediateInline(inline) => content.push(inline),
                    PandocNativeIntermediate::IntermediateInlines(inlines) => {
                        content.extend(inlines)
                    }
                    _ => {
                        // Skip unknown intermediates (marker nodes, etc.)
                    }
                }
            }
        }
    }

    // Strip trailing Space nodes from content (Pandoc strips spaces before attributes)
    while let Some(last) = content.last() {
        if matches!(last, Inline::Space(_)) {
            content.pop();
        } else {
            break;
        }
    }

    PandocNativeIntermediate::IntermediateBlock(Block::Header(Header {
        level,
        attr,
        content,
        source_info: node_source_info_with_context(node, context),
        attr_source,
    }))
}
