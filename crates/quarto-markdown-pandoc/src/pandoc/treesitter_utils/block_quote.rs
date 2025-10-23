/*
 * block_quote.rs
 *
 * Functions for processing block quote nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, BlockQuote, Blocks, RawBlock};
use crate::pandoc::location::node_source_info_with_context;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_block_quote<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content: Blocks = Vec::new();
    for (node_type, child) in children {
        if node_type == "block_quote_marker" {
            if matches!(child, PandocNativeIntermediate::IntermediateUnknown(_)) {
                if node_type != "block_continuation" {
                    writeln!(
                        buf,
                        "Warning: Unhandled node kind in block_quote: {}, {:?}",
                        node_type, child,
                    )
                    .unwrap();
                }
            }
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(section) => {
                content.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                content.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: quarto_source_map::SourceInfo::from_range(
                        quarto_source_map::FileId(0),
                        range,
                    ),
                }));
            }
            _ => {
                writeln!(
                buf,
                "[block_quote] Will ignore unknown node. Expected Block or Section in block_quote, got {:?}",
                child
                ).unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::BlockQuote(BlockQuote {
        content,
        source_info: node_source_info_with_context(node, context),
    }))
}
