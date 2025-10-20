/*
 * paragraph.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Paragraph};
use crate::pandoc::inline::Inline;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a paragraph node, collecting inlines and filtering out block continuations
pub fn process_paragraph(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut inlines: Vec<Inline> = Vec::new();
    for (node, child) in children {
        if node == "block_continuation" {
            continue; // skip block continuation nodes
        }
        if let PandocNativeIntermediate::IntermediateInline(inline) = child {
            inlines.push(inline);
        } else if let PandocNativeIntermediate::IntermediateInlines(inner_inlines) = child {
            inlines.extend(inner_inlines);
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Paragraph(Paragraph {
        content: inlines,
        source_info: node_source_info_with_context(node, context),
        source_info_qsm: None,
    }))
}
