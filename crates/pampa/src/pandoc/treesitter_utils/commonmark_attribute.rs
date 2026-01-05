/*
 * commonmark_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use hashlink::LinkedHashMap;
use quarto_source_map::SourceInfo;

/// Process a commonmark attribute (id, classes, key-value pairs)
/// Returns both the Attr and AttrSourceInfo with source locations for each component
pub fn process_commonmark_attribute(
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut attr = (String::new(), vec![], LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                if node == "attribute_id" {
                    attr.0 = text;
                    // Track source location of id
                    attr_source.id = Some(SourceInfo::from_range(context.current_file_id(), range));
                } else if node == "attribute_class" {
                    attr.1.push(text);
                    // Track source location of this class
                    attr_source.classes.push(Some(SourceInfo::from_range(
                        context.current_file_id(),
                        range,
                    )));
                }
                // Skip other node types
            }
            PandocNativeIntermediate::IntermediateKeyValueSpec(spec) => {
                // spec is Vec<(key, value, key_range, value_range)>
                for (key, value, key_range, value_range) in spec {
                    attr.2.insert(key, value);
                    // Convert ranges to SourceInfo
                    let key_source =
                        Some(SourceInfo::from_range(context.current_file_id(), key_range));
                    let value_source = Some(SourceInfo::from_range(
                        context.current_file_id(),
                        value_range,
                    ));
                    attr_source.attributes.push((key_source, value_source));
                }
            }
            _ => {
                // Skip unknown intermediates
            }
        };
    }

    PandocNativeIntermediate::IntermediateAttr(attr, attr_source)
}
