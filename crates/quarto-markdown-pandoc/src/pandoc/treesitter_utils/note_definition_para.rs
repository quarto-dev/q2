/*
 * note_definition_para.rs
 *
 * Functions for processing note definition paragraph nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::block::{Block, NoteDefinitionPara};
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_note_definition_para(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let mut id = String::new();
    let mut content = Vec::new();

    for (node_name, child) in children {
        if node_name == "ref_id_specifier" {
            if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                // Strip the "[^" prefix and "]:" suffix to extract just the ID
                // e.g., "[^1]:" -> "1"
                let stripped = text
                    .strip_prefix("[^")
                    .and_then(|s| s.strip_suffix("]:"))
                    .unwrap_or(&text);
                id = stripped.to_string();
            } else {
                panic!("Expected BaseText in ref_id_specifier, got {:?}", child);
            }
        } else if node_name == "paragraph" {
            if let PandocNativeIntermediate::IntermediateBlock(Block::Paragraph(para)) = child {
                content = para.content;
            } else {
                panic!("Expected Paragraph in inline_ref_def, got {:?}", child);
            }
        }
    }

    PandocNativeIntermediate::IntermediateBlock(Block::NoteDefinitionPara(NoteDefinitionPara {
        id,
        content,
        source_info: node_source_info_with_context(node, context),
    }))
}
