/*
 * document.rs
 *
 * Functions for processing document-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, RawBlock};
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::pandoc::{MetaValueWithSourceInfo, Pandoc};

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_document(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(_, child)| {
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => blocks.push(block),
            PandocNativeIntermediate::IntermediateSection(section) => {
                blocks.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, _range) => {
                // for now we assume it's metadata and emit it as a rawblock
                blocks.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: node_source_info_with_context(node, context),
                }));
            }
            _ => panic!("Expected Block or Section, got {:?}", child),
        }
    });
    PandocNativeIntermediate::IntermediatePandoc(Pandoc {
        // Legitimate default: Initial document creation - metadata populated later from YAML
        meta: MetaValueWithSourceInfo::default(),
        blocks,
    })
}
