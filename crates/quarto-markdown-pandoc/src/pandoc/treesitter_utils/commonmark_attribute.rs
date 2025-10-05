/*
 * commonmark_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use std::collections::HashMap;

/// Process a commonmark attribute (id, classes, key-value pairs)
pub fn process_commonmark_attribute(
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut attr = ("".to_string(), vec![], HashMap::new());
    children.into_iter().for_each(|(node, child)| match child {
        PandocNativeIntermediate::IntermediateBaseText(id, _) => {
            if node == "id_specifier" {
                attr.0 = id;
            } else if node == "class_specifier" {
                attr.1.push(id);
            } else {
                panic!("Unexpected commonmark_attribute node: {}", node);
            }
        }
        PandocNativeIntermediate::IntermediateKeyValueSpec(spec) => {
            for (key, value) in spec {
                attr.2.insert(key, value);
            }
        }
        PandocNativeIntermediate::IntermediateUnknown(_) => {}
        _ => panic!("Unexpected child in commonmark_attribute: {:?}", child),
    });
    PandocNativeIntermediate::IntermediateAttr(attr)
}
