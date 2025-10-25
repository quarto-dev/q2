/*
 * section.rs
 *
 * Functions for processing section-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, RawBlock};

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_section(
    _section_node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(node, child)| {
        if node == "block_continuation" {
            return;
        }
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => blocks.push(block),
            PandocNativeIntermediate::IntermediateSection(section) => {
                blocks.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                blocks.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: quarto_source_map::SourceInfo::from_range(
                        context.current_file_id(),
                        range,
                    ),
                }));
            }
            _ => panic!("Expected Block or Section, got {:?}", child),
        }
    });
    PandocNativeIntermediate::IntermediateSection(blocks)
}
