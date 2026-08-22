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
    span: SourceInfo,
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
                // spec is Vec<(key, value, key_range, value_content_source)>.
                // The value slot already *is* a `SourceInfo`: it carries the
                // decoded value's content provenance, which cannot be
                // expressed as a raw `Range` once an escape has collapsed.
                for (key, value, key_range, value_content_source) in spec {
                    // Pandoc reserves the `id` and `class` keys: they fill the
                    // identifier and classes slots instead of the kv map, and
                    // the last id (of either form) wins. Processing here, in
                    // child order, preserves that positional semantics.
                    match key.as_str() {
                        "id" => {
                            attr.0 = value;
                            attr_source.id = Some(value_content_source);
                        }
                        "class" => {
                            // Whitespace-separated words, each its own class.
                            // All source entries point at the whole quoted
                            // value (per-word sub-spans: bd-0vfgz2cl).
                            for word in value.split_whitespace() {
                                attr.1.push(word.to_string());
                                attr_source.classes.push(Some(value_content_source.clone()));
                            }
                        }
                        _ => {
                            attr.2.insert(key, value);
                            let key_source =
                                Some(SourceInfo::from_range(context.current_file_id(), key_range));
                            attr_source
                                .attributes
                                .push((key_source, Some(value_content_source)));
                        }
                    }
                }
            }
            _ => {
                // Skip unknown intermediates
            }
        };
    }

    PandocNativeIntermediate::IntermediateAttr(attr, attr_source, span)
}
