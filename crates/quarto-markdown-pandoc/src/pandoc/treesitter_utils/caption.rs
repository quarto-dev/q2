/*
 * caption.rs
 *
 * Functions for processing caption nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, CaptionBlock};
use crate::pandoc::inline::Inlines;
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_caption(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut caption_inlines: Inlines = Vec::new();

    for (node_name, child) in children {
        if node_name == "inline" {
            match child {
                PandocNativeIntermediate::IntermediateInlines(inlines) => {
                    caption_inlines.extend(inlines);
                }
                _ => panic!("Expected Inlines in caption, got {:?}", child),
            }
        }
        // Skip other nodes like ":", blank_line, etc.
    }

    PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(CaptionBlock {
        content: caption_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
