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
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_atx_heading<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut level = 0;
    let mut content: Vec<Inline> = Vec::new();
    let mut attr: Attr = ("".to_string(), vec![], LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_kind, child) in children {
        if node_kind == "block_continuation" {
            continue;
            // This is a marker node, we don't need to do anything with it
        } else if node_kind == "atx_h1_marker" {
            level = 1;
        } else if node_kind == "atx_h2_marker" {
            level = 2;
        } else if node_kind == "atx_h3_marker" {
            level = 3;
        } else if node_kind == "atx_h4_marker" {
            level = 4;
        } else if node_kind == "atx_h5_marker" {
            level = 5;
        } else if node_kind == "atx_h6_marker" {
            level = 6;
        } else if node_kind == "inline" {
            // Old grammar: inline wrapper node
            if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                content.extend(inlines);
            } else {
                panic!("Expected Inlines in atx_heading, got {:?}", child);
            }
        } else if node_kind == "attribute" || node_kind == "attribute_specifier" {
            if let PandocNativeIntermediate::IntermediateAttr(inner_attr, inner_attr_source) = child
            {
                attr = inner_attr;
                attr_source = inner_attr_source;
            } else {
                panic!("Expected Attr in attribute, got {:?}", child);
            }
        } else {
            // New grammar: inline content directly as children (pandoc_str, pandoc_emph, etc.)
            // Collect any inline nodes
            if let PandocNativeIntermediate::IntermediateInline(inline) = child {
                content.push(inline);
            } else if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                content.extend(inlines);
            } else if node_kind.starts_with("atx_h") && node_kind.ends_with("_marker") {
                // Skip closing markers
                continue;
            } else {
                writeln!(
                    buf,
                    "Warning: Unhandled node kind in atx_heading: {}",
                    node_kind
                )
                .unwrap();
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
