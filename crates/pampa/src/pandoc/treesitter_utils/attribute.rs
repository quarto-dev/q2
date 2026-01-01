/*
 * attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process attribute node to extract commonmark attributes
pub fn process_attribute(
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    if let Some((node, child)) = children.into_iter().next() {
        match child {
            PandocNativeIntermediate::IntermediateAttr(attr, attr_source) => {
                if node == "commonmark_attribute" {
                    return PandocNativeIntermediate::IntermediateAttr(attr, attr_source);
                } else if node == "raw_attribute" {
                    panic!("Unexpected raw attribute in attribute: {:?}", attr);
                } else {
                    panic!("Unexpected attribute node: {}", node);
                }
            }
            _ => panic!("Unexpected child in attribute: {:?}", child),
        }
    }
    panic!("No commonmark_attribute found in attribute node");
}
