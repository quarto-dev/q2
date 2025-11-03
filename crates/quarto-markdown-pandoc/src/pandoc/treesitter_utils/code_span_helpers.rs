/*
 * code_span_helpers.rs
 *
 * Functions for processing code span nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, AttrSourceInfo, empty_attr};
use crate::pandoc::inline::{Code, Inline, Space};
use crate::pandoc::location::node_source_info_with_context;

/// Process pandoc_code_span node
pub fn process_pandoc_code_span(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // Extract code text and optional attributes
    // Also check for spaces in delimiters (similar to emphasis handling)
    let mut code_text = String::new();
    let mut attr: Attr = empty_attr();
    let mut attr_source = AttrSourceInfo::empty();
    let mut raw_format: Option<String> = None;
    let mut language_specifier: Option<String> = None;
    let mut has_leading_space = false;
    let mut has_trailing_space = false;
    let mut first_delimiter = true;

    for (node_name, child) in &children {
        match node_name.as_str() {
            "content" => {
                // Extract text from content node
                if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                    code_text =
                        std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                            .unwrap()
                            .to_string();
                }
            }
            "code_span_delimiter" => {
                // Check if delimiter includes spaces
                if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                    let text =
                        std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                            .unwrap();
                    if first_delimiter {
                        // Opening delimiter - check for leading space
                        has_leading_space = text.starts_with(char::is_whitespace);
                        first_delimiter = false;
                    } else {
                        // Closing delimiter - check for trailing space
                        has_trailing_space = text.ends_with(char::is_whitespace);
                    }
                }
            }
            "attribute_specifier" => {
                // Process attributes, raw format, or language specifier if present
                match child {
                    PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src) => {
                        attr = attrs.clone();
                        attr_source = attrs_src.clone();
                    }
                    PandocNativeIntermediate::IntermediateRawFormat(format, _) => {
                        raw_format = Some(format.clone());
                    }
                    PandocNativeIntermediate::IntermediateBaseText(lang, _) => {
                        // This is a language specifier (e.g., "r" which we'll wrap as "{r}")
                        language_specifier = Some(format!("{{{}}}", lang));
                    }
                    _ => {}
                }
            }
            "raw_attribute" => {
                // Extract raw format (e.g., {=html}) - legacy path
                if let PandocNativeIntermediate::IntermediateRawFormat(format, _) = child {
                    raw_format = Some(format.clone());
                }
            }
            _ => {}
        }
    }

    // Trim whitespace from code text (Pandoc behavior)
    let mut trimmed_code_text = code_text.trim().to_string();

    // If there's a language specifier, prepend it to the code text
    if let Some(lang) = language_specifier {
        trimmed_code_text = format!("{} {}", lang, trimmed_code_text);
    }

    // Create Code or RawInline based on presence of raw format
    let code = if let Some(format) = raw_format {
        Inline::RawInline(crate::pandoc::inline::RawInline {
            format,
            text: trimmed_code_text,
            source_info: node_source_info_with_context(node, context),
        })
    } else {
        Inline::Code(Code {
            attr,
            text: trimmed_code_text,
            source_info: node_source_info_with_context(node, context),
            attr_source,
        })
    };

    // Build result with injected Space nodes as needed
    let mut result = Vec::new();

    if has_leading_space {
        result.push(Inline::Space(Space {
            source_info: node_source_info_with_context(node, context),
        }));
    }

    result.push(code);

    if has_trailing_space {
        result.push(Inline::Space(Space {
            source_info: node_source_info_with_context(node, context),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
