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
    for (node, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateAttr(attr) => {
                if node == "commonmark_attribute" {
                    return PandocNativeIntermediate::IntermediateAttr(attr);
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
