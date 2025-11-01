/*
 * section.rs
 *
 * Functions for processing section-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Plain, RawBlock};
use crate::pandoc::caption::Caption;

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

    // POST-PROCESS: Attach standalone captions to previous tables
    // The grammar allows captions as standalone blocks when separated from table by empty line
    let mut i = 0;
    while i < blocks.len() {
        if i > 0 {
            // Check if current block is a CaptionBlock followed by a Table
            let should_attach = matches!(
                (&blocks[i - 1], &blocks[i]),
                (Block::Table(_), Block::CaptionBlock(_))
            );

            if should_attach {
                // Extract caption data before modifying blocks
                let caption_inlines;
                let caption_source_info;
                if let Block::CaptionBlock(caption_block) = &blocks[i] {
                    caption_inlines = caption_block.content.clone();
                    caption_source_info = caption_block.source_info.clone();
                } else {
                    unreachable!()
                }

                // Now modify the table
                if let Block::Table(ref mut table) = blocks[i - 1] {
                    table.caption = Caption {
                        short: None,
                        long: Some(vec![Block::Plain(Plain {
                            content: caption_inlines,
                            source_info: caption_source_info.clone(),
                        })]),
                        source_info: caption_source_info,
                    };
                }

                // Remove the standalone CaptionBlock
                blocks.remove(i);
                continue; // Don't increment i, check the same index again
            }
        }
        i += 1;
    }

    PandocNativeIntermediate::IntermediateSection(blocks)
}
