/*
 * atx_heading.rs
 *
 * Functions for processing ATX heading nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::Attr;
use crate::pandoc::block::{Block, Header};
use crate::pandoc::inline::Inline;
use crate::pandoc::location::{SourceInfo, node_location};
use std::collections::HashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_atx_heading<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut level = 0;
    let mut content: Vec<Inline> = Vec::new();
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    for (node, child) in children {
        if node == "block_continuation" {
            continue;
            // This is a marker node, we don't need to do anything with it
        } else if node == "atx_h1_marker" {
            level = 1;
        } else if node == "atx_h2_marker" {
            level = 2;
        } else if node == "atx_h3_marker" {
            level = 3;
        } else if node == "atx_h4_marker" {
            level = 4;
        } else if node == "atx_h5_marker" {
            level = 5;
        } else if node == "atx_h6_marker" {
            level = 6;
        } else if node == "inline" {
            if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                content.extend(inlines);
            } else {
                panic!("Expected Inlines in atx_heading, got {:?}", child);
            }
        } else if node == "attribute" {
            if let PandocNativeIntermediate::IntermediateAttr(inner_attr) = child {
                attr = inner_attr;
            } else {
                panic!("Expected Attr in attribute, got {:?}", child);
            }
        } else {
            writeln!(buf, "Warning: Unhandled node kind in atx_heading: {}", node).unwrap();
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Header(Header {
        level,
        attr,
        content,
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}
