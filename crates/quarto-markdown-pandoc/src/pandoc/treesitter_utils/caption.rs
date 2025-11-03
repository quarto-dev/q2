/*
 * caption.rs
 *
 * Functions for processing caption nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, CaptionBlock};
use crate::pandoc::inline::Inlines;
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_caption(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut caption_inlines: Inlines = Vec::new();
    let mut caption_attr: Option<(
        crate::pandoc::attr::Attr,
        crate::pandoc::attr::AttrSourceInfo,
    )> = None;

    for (node_name, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateInline(inline) => {
                caption_inlines.push(inline);
            }
            PandocNativeIntermediate::IntermediateInlines(inlines) => {
                caption_inlines.extend(inlines);
            }
            PandocNativeIntermediate::IntermediateAttr(attr, attr_source) => {
                // Attributes from attribute_specifier nodes
                if node_name == "attribute_specifier" {
                    caption_attr = Some((attr, attr_source));
                }
            }
            _ => {
                // Skip other nodes (colon marker, whitespace markers, etc.)
            }
        }
    }

    // If we found an attribute, append it as Inline::Attr
    if let Some((attr, attr_source)) = caption_attr {
        caption_inlines.push(crate::pandoc::inline::Inline::Attr(attr, attr_source));
    }

    PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(CaptionBlock {
        content: caption_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
