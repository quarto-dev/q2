/*
 * fenced_code_block.rs
 *
 * Functions for processing fenced code block nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, empty_attr};
use crate::pandoc::block::{Block, CodeBlock, RawBlock};
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_fenced_code_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content: String = String::new();
    let mut attr: Attr = empty_attr();
    let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
    let mut raw_format: Option<String> = None;
    for (node, child) in children {
        if node == "block_continuation" {
            continue; // skip block continuation nodes
        }
        if node == "code_fence_content" {
            let PandocNativeIntermediate::IntermediateBaseText(text, _) = child else {
                panic!("Expected BaseText in code_fence_content, got {:?}", child)
            };
            content = text;
        } else if node == "attribute_specifier" {
            // Handle attribute_specifier which can contain IntermediateAttr, IntermediateRawFormat, or IntermediateBaseText (language specifier)
            match child {
                PandocNativeIntermediate::IntermediateAttr(a, as_) => {
                    attr = a;
                    attr_source = as_;
                }
                PandocNativeIntermediate::IntermediateRawFormat(format, _) => {
                    raw_format = Some(format);
                }
                PandocNativeIntermediate::IntermediateBaseText(lang, range) => {
                    // This is a language specifier (e.g., "r" from {r})
                    // Wrap in braces to preserve the syntax for roundtripping
                    attr.1.push(format!("{{{}}}", lang));

                    // Track source location for the language specifier
                    let lang_source = crate::pandoc::source_map_compat::range_to_source_info_with_context(
                        &range, context,
                    );
                    attr_source.classes.push(Some(lang_source));
                }
                _ => {
                    panic!(
                        "Expected Attr, RawFormat, or BaseText in attribute_specifier, got {:?}",
                        child
                    )
                }
            }
        } else if node == "commonmark_attribute" {
            let PandocNativeIntermediate::IntermediateAttr(a, as_) = child else {
                panic!("Expected Attr in commonmark_attribute, got {:?}", child)
            };
            attr = a;
            attr_source = as_;
        } else if node == "raw_attribute" {
            let PandocNativeIntermediate::IntermediateRawFormat(format, _) = child else {
                panic!("Expected RawFormat in raw_attribute, got {:?}", child)
            };
            raw_format = Some(format);
        } else if node == "language_attribute" {
            let PandocNativeIntermediate::IntermediateBaseText(lang, range) = child else {
                panic!("Expected BaseText in language_attribute, got {:?}", child)
            };
            attr.1.push(lang); // set the language

            // Track source location for the language specifier
            let lang_source = crate::pandoc::source_map_compat::range_to_source_info_with_context(
                &range, context,
            );
            attr_source.classes.push(Some(lang_source));
        } else if node == "info_string" {
            let PandocNativeIntermediate::IntermediateAttr(inner_attr, inner_as_) = child else {
                panic!("Expected Attr in info_string, got {:?}", child)
            };
            attr = inner_attr;
            attr_source = inner_as_;
        }
    }
    let location = node_source_info_with_context(node, context);

    // it might be the case (because of tree-sitter error recovery)
    // that the content does not end with a newline, so we ensure it does before popping
    if content.ends_with('\n') {
        content.pop(); // remove the trailing newline
    }

    if let Some(format) = raw_format {
        PandocNativeIntermediate::IntermediateBlock(Block::RawBlock(RawBlock {
            format,
            text: content,
            source_info: location,
        }))
    } else {
        PandocNativeIntermediate::IntermediateBlock(Block::CodeBlock(CodeBlock {
            attr,
            text: content,
            source_info: location,
            attr_source,
        }))
    }
}
