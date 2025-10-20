/*
 * note_definition_fenced_block.rs
 *
 * Functions for processing note definition fenced block nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Blocks, NoteDefinitionFencedBlock};
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_note_definition_fenced_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut id = String::new();
    let mut content: Blocks = Vec::new();

    for (node_name, child) in children {
        if node_name == "fenced_div_note_id" {
            if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                // The text includes the ^ prefix, strip it
                id = text.strip_prefix('^').unwrap_or(&text).to_string();
            } else {
                panic!("Expected BaseText in fenced_div_note_id, got {:?}", child);
            }
        } else if node_name == "block_continuation" {
            // This is a marker node, we don't need to do anything with it
        } else {
            match child {
                PandocNativeIntermediate::IntermediateBlock(block) => {
                    content.push(block);
                }
                PandocNativeIntermediate::IntermediateSection(blocks) => {
                    content.extend(blocks);
                }
                _ => {
                    // Ignore other intermediate nodes
                }
            }
        }
    }

    PandocNativeIntermediate::IntermediateBlock(Block::NoteDefinitionFencedBlock(
        NoteDefinitionFencedBlock {
            id,
            content,
            source_info: node_source_info_with_context(node, context),
            source_info_qsm: None,
        },
    ))
}
