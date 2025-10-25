/*
 * commonmark_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use quarto_source_map::SourceInfo;
use std::collections::HashMap;

/// Process a commonmark attribute (id, classes, key-value pairs)
/// Returns both the Attr and AttrSourceInfo with source locations for each component
pub fn process_commonmark_attribute(
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    children.into_iter().for_each(|(node, child)| match child {
        PandocNativeIntermediate::IntermediateBaseText(text, range) => {
            if node == "id_specifier" {
                attr.0 = text;
                // Track source location of id (empty id gets None)
                attr_source.id = if attr.0.is_empty() {
                    None
                } else {
                    Some(SourceInfo::from_range(context.current_file_id(), range))
                };
            } else if node == "class_specifier" {
                attr.1.push(text);
                // Track source location of this class
                attr_source.classes.push(Some(SourceInfo::from_range(
                    context.current_file_id(),
                    range,
                )));
            } else {
                panic!("Unexpected commonmark_attribute node: {}", node);
            }
        }
        PandocNativeIntermediate::IntermediateKeyValueSpec(spec) => {
            // TODO: We need to track individual key and value source locations
            // For now, just add empty entries to maintain structure
            for (key, value) in spec {
                attr.2.insert(key, value);
                // Placeholder: We don't have source info for keys/values yet
                attr_source.attributes.push((None, None));
            }
        }
        PandocNativeIntermediate::IntermediateUnknown(_) => {}
        _ => panic!("Unexpected child in commonmark_attribute: {:?}", child),
    });

    PandocNativeIntermediate::IntermediateAttr(attr, attr_source)
}
