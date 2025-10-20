/*
 * note_reference.rs
 *
 * Functions for processing note reference nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, NoteReference};
use crate::pandoc::location::{convert_range, node_source_info_with_context};

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_note_reference(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut id = String::new();
    for (node, child) in children {
        if node == "note_reference_delimiter" {
            // This is a marker node, we don't need to do anything with it
        } else if node == "note_reference_id" {
            if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                id = text;
            } else {
                panic!("Expected BaseText in note_reference_id, got {:?}", child);
            }
        } else {
            panic!("Unexpected note_reference node: {}", node);
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::NoteReference(NoteReference {
        id,
        range: convert_range(&node_source_info_with_context(node, context).range),
    }))
}
