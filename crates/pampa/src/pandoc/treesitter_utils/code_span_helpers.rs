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

/// Extract the code-span text from a `content` tree-sitter node.
///
/// The grammar (bd-ilv8p) allows multi-line code spans: the content
/// node may contain `pandoc_soft_break` children whose byte range
/// covers the line ending plus any block-continuation gutter (e.g.
/// `> ` for blockquotes, the indent for list items). To recover the
/// pandoc-equivalent code text:
///
///   1. The text between soft_break children is appended verbatim
///      (preserving doubled spaces — pandoc does not collapse them).
///   2. Each `pandoc_soft_break` range collapses to a single space.
///
/// On the single-line path (no children), the function falls back to
/// reading the content node's raw byte range, identical to the old
/// behavior.
fn extract_code_span_text(content_node: &tree_sitter::Node, input_bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut cursor = content_node.walk();
    if !cursor.goto_first_child() {
        // No named children — single-line content (just text segments,
        // which are anonymous regex tokens not exposed in the tree).
        let bytes = &input_bytes[content_node.start_byte()..content_node.end_byte()];
        return std::str::from_utf8(bytes).unwrap().to_string();
    }
    let mut byte_cursor = content_node.start_byte();
    loop {
        let child = cursor.node();
        // Append any text from byte_cursor up to the child's start.
        if child.start_byte() > byte_cursor {
            let bytes = &input_bytes[byte_cursor..child.start_byte()];
            text.push_str(std::str::from_utf8(bytes).unwrap());
        }
        match child.kind() {
            "pandoc_soft_break" => {
                // Newline + block-continuation gutter → single space.
                text.push(' ');
            }
            _ => {
                // Unexpected named child — preserve its bytes to stay
                // forward-compatible with grammar additions.
                let bytes = &input_bytes[child.start_byte()..child.end_byte()];
                text.push_str(std::str::from_utf8(bytes).unwrap());
            }
        }
        byte_cursor = child.end_byte();
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    // Append any trailing text after the last child.
    if byte_cursor < content_node.end_byte() {
        let bytes = &input_bytes[byte_cursor..content_node.end_byte()];
        text.push_str(std::str::from_utf8(bytes).unwrap());
    }
    text
}

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
    let mut checked_opening_delimiter = false;

    // Find the content child via a direct tree walk: we can't rely on
    // the intermediate `child` value, because the bottom-up visitor
    // collapses the content node's anonymous text-segment regexes and
    // the multi-line case has structural pandoc_soft_break children
    // whose surrounding text segments are lost in the intermediate
    // form.
    {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "content" {
                    code_text = extract_code_span_text(&child, input_bytes);
                    break;
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    for (node_name, child) in &children {
        match node_name.as_str() {
            "content" => {
                // Handled above via direct tree walk.
            }
            "code_span_delimiter" => {
                // Check if opening delimiter includes leading space
                // (The closing delimiter never includes trailing space in the grammar)
                if !checked_opening_delimiter {
                    if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                        let text =
                            std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                                .unwrap();
                        // ASCII-only by intent (Pandoc-compat policy —
                        // see plan doc bd-rmx3/bd-8oe4): non-ASCII
                        // whitespace is content, not delimiter padding.
                        has_leading_space = text.starts_with(|c: char| c.is_ascii_whitespace());
                    }
                    checked_opening_delimiter = true;
                }
            }
            "attribute_specifier" => {
                // Process attributes, raw format, or language specifier if present
                match child {
                    PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src, _) => {
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
            _ => {
                // Skip unknown node types (shouldn't happen in practice)
            }
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

    PandocNativeIntermediate::IntermediateInlines(result)
}
