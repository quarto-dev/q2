/*
 * code_span.rs
 *
 * Functions for processing code span nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Code, Inline, RawInline};
use crate::pandoc::location::node_source_info_with_context;
use hashlink::LinkedHashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_code_span<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut is_raw: Option<String> = None;
    let mut attr = ("".to_string(), vec![], LinkedHashMap::new());
    let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
    let mut language_attribute: Option<String> = None;
    let mut inlines: Vec<_> = children
        .into_iter()
        .map(|(node_name, child)| {
            let source_info = node_source_info_with_context(node, context);
            let range = crate::pandoc::source_map_compat::source_info_to_qsm_range_or_fallback(
                &source_info,
                context,
            );
            match child {
                PandocNativeIntermediate::IntermediateAttr(a, as_) => {
                    attr = a;
                    attr_source = as_;
                    // IntermediateUnknown here "consumes" the node
                    (
                        node_name,
                        PandocNativeIntermediate::IntermediateUnknown(range.clone()),
                    )
                }
                PandocNativeIntermediate::IntermediateRawFormat(raw, _) => {
                    is_raw = Some(raw);
                    // IntermediateUnknown here "consumes" the node
                    (
                        node_name,
                        PandocNativeIntermediate::IntermediateUnknown(range.clone()),
                    )
                }
                PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                    if node_name == "language_attribute" {
                        language_attribute = Some(text);
                        // IntermediateUnknown here "consumes" the node
                        (
                            node_name,
                            PandocNativeIntermediate::IntermediateUnknown(range),
                        )
                    } else {
                        (
                            node_name,
                            PandocNativeIntermediate::IntermediateBaseText(text, range),
                        )
                    }
                }
                _ => (node_name, child),
            }
        })
        .filter(|(_, child)| {
            match child {
                PandocNativeIntermediate::IntermediateUnknown(_) => false, // skip unknown nodes
                _ => true,                                                 // keep other nodes
            }
        })
        .collect();
    if inlines.len() == 0 {
        writeln!(
            buf,
            "Warning: Expected exactly one inline in code_span, got none"
        )
        .unwrap();
        return PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
            attr,
            text: "".to_string(),
            source_info: node_source_info_with_context(node, context),
            attr_source,
        }));
    }
    let (_, child) = inlines.remove(0);
    if inlines.len() > 0 {
        writeln!(
            buf,
            "Warning: Expected exactly one inline in code_span, got {}. Will ignore the rest.",
            inlines.len() + 1
        )
        .unwrap();
    }
    let text = match child {
        PandocNativeIntermediate::IntermediateBaseText(text, _) => text.trim().to_string(),
        _ => {
            writeln!(
                buf,
                "Warning: Expected BaseText in code_span, got {:?}. Will ignore.",
                child
            )
            .unwrap();
            "".to_string()
        }
    };
    if let Some(raw) = is_raw {
        PandocNativeIntermediate::IntermediateInline(Inline::RawInline(RawInline {
            format: raw,
            text,
            source_info: node_source_info_with_context(node, context),
        }))
    } else {
        match language_attribute {
            Some(lang) => PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
                attr,
                text: lang + &" " + &text,
                source_info: node_source_info_with_context(node, context),
                attr_source: attr_source.clone(),
            })),
            None => PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
                attr,
                text,
                source_info: node_source_info_with_context(node, context),
                attr_source,
            })),
        }
    }
}
