/*
 * document.rs
 *
 * Functions for processing document-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, RawBlock};
use crate::pandoc::location::range_to_source_info_with_context;
use crate::pandoc::pandoc::Pandoc;
use quarto_pandoc_types::ConfigValue;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_document(
    _node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateSection(section) => {
                blocks.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                blocks.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    // Reroot through `parent_source_info` when this parse is
                    // itself a re-parse (e.g. a content processor's
                    // converted buffer, Plan 7b) — otherwise a YAML
                    // front-matter error's ariadne snippet resolves against
                    // the converted buffer instead of the true original
                    // file.
                    source_info: range_to_source_info_with_context(&range, context),
                }));
            }
            PandocNativeIntermediate::IntermediateUnknown(_) => {
                // Skip unknown nodes - these occur when tree-sitter encounters parse errors
                // The parse errors are already reported via the log observer
            }
            _ => panic!("Expected Block or Section, got {:?}", child),
        }
    }
    PandocNativeIntermediate::IntermediatePandoc(Pandoc {
        // Legitimate default: Initial document creation - metadata populated later from YAML
        meta: ConfigValue::default(),
        blocks,
    })
}
