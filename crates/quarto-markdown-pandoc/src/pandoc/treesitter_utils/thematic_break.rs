/*
 * thematic_break.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, HorizontalRule};
use crate::pandoc::source_map_compat;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a thematic break (horizontal rule)
pub fn process_thematic_break(
    node: &tree_sitter::Node,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    PandocNativeIntermediate::IntermediateBlock(Block::HorizontalRule(HorizontalRule {
        source_info: source_map_compat::node_to_source_info_with_context(node, context),
    }))
}
