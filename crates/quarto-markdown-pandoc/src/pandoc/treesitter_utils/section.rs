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
                let mut caption_inlines;
                let caption_source_info;
                let mut caption_attr: Option<crate::pandoc::attr::Attr> = None;
                let mut caption_attr_source: Option<crate::pandoc::attr::AttrSourceInfo> = None;

                if let Block::CaptionBlock(caption_block) = &blocks[i] {
                    caption_inlines = caption_block.content.clone();
                    caption_source_info = caption_block.source_info.clone();

                    // Extract Inline::Attr if present at the end
                    if let Some(crate::pandoc::inline::Inline::Attr(attr, attr_source)) =
                        caption_inlines.last()
                    {
                        caption_attr = Some(attr.clone());
                        caption_attr_source = Some(attr_source.clone());
                        caption_inlines.pop();

                        // Trim trailing space before the attribute
                        caption_inlines = super::postprocess::trim_inlines(caption_inlines).0;
                    }
                } else {
                    unreachable!()
                }

                // Now modify the table
                if let Block::Table(ref mut table) = blocks[i - 1] {
                    // Apply caption attributes to table if present
                    if let Some(attr) = caption_attr {
                        // Merge attributes: caption attributes override table attributes
                        for (key, value) in &attr.2 {
                            table.attr.2.insert(key.clone(), value.clone());
                        }
                        for class in &attr.1 {
                            if !table.attr.1.contains(class) {
                                table.attr.1.push(class.clone());
                            }
                        }
                        if table.attr.0.is_empty() && !attr.0.is_empty() {
                            table.attr.0 = attr.0.clone();
                        }

                        // Merge attr_source if present
                        if let Some(attr_source) = caption_attr_source {
                            for (key_source, value_source) in attr_source.attributes {
                                table
                                    .attr_source
                                    .attributes
                                    .push((key_source, value_source));
                            }
                            for class_source in attr_source.classes {
                                table.attr_source.classes.push(class_source);
                            }
                            if table.attr_source.id.is_none() && attr_source.id.is_some() {
                                table.attr_source.id = attr_source.id;
                            }
                        }
                    }

                    table.caption = Caption {
                        short: None,
                        long: Some(vec![Block::Plain(Plain {
                            content: caption_inlines,
                            source_info: caption_source_info.clone(),
                        })]),
                        source_info: caption_source_info.clone(),
                    };

                    // Extend table's source_info to include the caption
                    let table_start_offset = table.source_info.start_offset();
                    let caption_end_offset = caption_source_info.end_offset();
                    // Extract file_id from table's source info
                    let file_id = match &table.source_info {
                        quarto_source_map::SourceInfo::Original { file_id, .. } => *file_id,
                        quarto_source_map::SourceInfo::Substring { parent, .. } => {
                            match **parent {
                                quarto_source_map::SourceInfo::Original { file_id, .. } => file_id,
                                _ => quarto_source_map::FileId(0), // Fallback
                            }
                        }
                        quarto_source_map::SourceInfo::Concat { pieces } => {
                            if let Some(piece) = pieces.first() {
                                match &piece.source_info {
                                    quarto_source_map::SourceInfo::Original { file_id, .. } => *file_id,
                                    _ => quarto_source_map::FileId(0), // Fallback
                                }
                            } else {
                                quarto_source_map::FileId(0) // Fallback
                            }
                        }
                    };
                    table.source_info = quarto_source_map::SourceInfo::original(
                        file_id,
                        table_start_offset,
                        caption_end_offset,
                    );
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
