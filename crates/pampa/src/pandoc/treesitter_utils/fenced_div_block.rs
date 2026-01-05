/*
 * fenced_div_block.rs
 *
 * Functions for processing fenced div block nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::Attr;
use crate::pandoc::block::{Block, Div, RawBlock};
use crate::pandoc::location::node_source_info_with_context;
use hashlink::LinkedHashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_fenced_div_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut attr: Attr = (String::new(), vec![], LinkedHashMap::new());
    let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
    let mut content: Vec<Block> = Vec::new();
    for (node, child) in children {
        if node == "block_continuation" {
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateAttr(a, as_) => {
                attr = a;
                attr_source = as_;
            }
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(blocks) => {
                content.extend(blocks);
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
                // Skip unexpected intermediates (shouldn't happen in practice)
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Div(Div {
        attr,
        content,
        source_info: node_source_info_with_context(node, context),
        attr_source,
    }))
}
