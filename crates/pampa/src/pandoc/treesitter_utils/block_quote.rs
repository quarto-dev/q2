/*
 * block_quote.rs
 *
 * Functions for processing block quote nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, BlockQuote, Blocks};
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_block_quote(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content: Blocks = Vec::new();
    for (node_type, child) in children {
        if node_type == "block_quote_marker" || node_type == "block_continuation" {
            // Skip marker nodes
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(section) => {
                content.extend(section);
            }
            _ => {
                // Skip unknown intermediates (shouldn't happen in practice)
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::BlockQuote(BlockQuote {
        content,
        source_info: node_source_info_with_context(node, context),
    }))
}
