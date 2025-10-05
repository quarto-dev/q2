/*
 * setext_heading.rs
 *
 * Functions for processing setext heading nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::empty_attr;
use crate::pandoc::block::{Block, Header, Paragraph};
use crate::pandoc::location::{SourceInfo, node_source_info_with_context};
use crate::pandoc::parse_context::ParseContext;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_setext_heading<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let mut content = Vec::new();
    let mut level = 1;
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBlock(Block::Paragraph(Paragraph {
                content: inner_content,
                ..
            })) => {
                content = inner_content;
            }
            PandocNativeIntermediate::IntermediateSetextHeadingLevel(l) => {
                level = l;
            }
            _ => {
                writeln!(
                    buf,
                    "[setext_heading] Warning: Unhandled node kind: {}",
                    node.kind()
                )
                .unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Header(Header {
        level,
        attr: empty_attr(),
        content,
        source_info: node_source_info_with_context(node, context),
    }))
}
