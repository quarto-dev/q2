/*
 * thematic_break.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::block::{Block, HorizontalRule};
use crate::pandoc::location::{SourceInfo, node_location};
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a thematic break (horizontal rule)
pub fn process_thematic_break(node: &tree_sitter::Node) -> PandocNativeIntermediate {
    PandocNativeIntermediate::IntermediateBlock(Block::HorizontalRule(HorizontalRule {
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}
