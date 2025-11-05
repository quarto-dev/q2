/*
 * document.rs
 *
 * Functions for processing document-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, RawBlock};
use crate::pandoc::pandoc::{MetaValueWithSourceInfo, Pandoc};

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_document(
    _node: &tree_sitter::Node,
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
            PandocNativeIntermediate::IntermediateUnknown(_) => {
                // Skip unknown nodes - these occur when tree-sitter encounters parse errors
                // The parse errors are already reported via the log observer
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
