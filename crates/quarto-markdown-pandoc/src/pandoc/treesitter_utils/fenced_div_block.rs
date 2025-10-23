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
use std::collections::HashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_fenced_div_block<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    let mut content: Vec<Block> = Vec::new();
    for (node, child) in children {
        if node == "block_continuation" {
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateBaseText(_, _) => {
                if node == "language_attribute" {
                    writeln!(
                        buf,
                        "Warning: language attribute unsupported in divs: {:?} {:?}",
                        node, child
                    )
                    .unwrap();
                } else {
                    writeln!(
                        buf,
                        "Warning: Unexpected base text in div, ignoring: {:?} {:?}",
                        node, child
                    )
                    .unwrap();
                }
            }
            PandocNativeIntermediate::IntermediateRawFormat(_, _) => {
                writeln!(
                    buf,
                    "Warning: Raw attribute specifiers are not supported in divs: {:?} {:?}",
                    node, child
                )
                .unwrap();
            }
            PandocNativeIntermediate::IntermediateAttr(a) => {
                attr = a;
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
                writeln!(
                    buf,
                    "Warning: Unhandled node kind in fenced_div_block: {:?} {:?}",
                    node, child
                )
                .unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Div(Div {
        attr,
        content,
        source_info: node_source_info_with_context(node, context),
    }))
}
