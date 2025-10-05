/*
 * thematic_break.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::block::{Block, HorizontalRule};
use crate::pandoc::location::{SourceInfo, node_source_info_with_context};
use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a thematic break (horizontal rule)
pub fn process_thematic_break(
    node: &tree_sitter::Node,
    context: &ParseContext,
) -> PandocNativeIntermediate {
    PandocNativeIntermediate::IntermediateBlock(Block::HorizontalRule(HorizontalRule {
        source_info: node_source_info_with_context(node, context),
    }))
}
