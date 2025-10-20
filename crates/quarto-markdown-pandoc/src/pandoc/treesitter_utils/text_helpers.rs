/*
 * text_helpers.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::inline::{Inline, LineBreak, SoftBreak};
use crate::pandoc::location::{SourceInfo, node_location};
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use once_cell::sync::Lazy;
use regex::Regex;

/// Helper function to filter out delimiter nodes
pub fn filter_delimiter_children(
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
) -> Vec<(String, PandocNativeIntermediate)> {
    children
        .into_iter()
        .filter(|(node, _)| node != delimiter_name)
        .collect()
}

/// Helper function to extract text from string quotes
pub fn extract_quoted_text(text: &str) -> String {
    if text.starts_with('"') && text.ends_with('"') {
        let escaped_double_quote_re: Lazy<Regex> = Lazy::new(|| Regex::new("[\\\\][\"]").unwrap());
        let value = &text[1..text.len() - 1];
        escaped_double_quote_re.replace_all(value, "\"").to_string()
    } else if text.starts_with('\'') && text.ends_with('\'') {
        let escaped_single_quote_re: Lazy<Regex> = Lazy::new(|| Regex::new("[\\\\][']").unwrap());
        let value = &text[1..text.len() - 1];
        escaped_single_quote_re.replace_all(value, "'").to_string()
    } else {
        text.to_string()
    }
}

/// Helper function to process inline emphasis-like constructs
pub fn process_emphasis_like_inline<F>(
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    mut native_inline: F,
) -> Vec<Inline>
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
{
    filter_delimiter_children(children, delimiter_name)
        .into_iter()
        .map(|child| native_inline(child))
        .collect()
}

/// Helper function to process emphasis-like inlines with a closure to build the final result
pub fn process_emphasis_inline<F, G>(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    native_inline: F,
    build_inline: G,
) -> PandocNativeIntermediate
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
    G: FnOnce(Vec<Inline>, &tree_sitter::Node) -> Inline,
{
    let inlines = process_emphasis_like_inline(children, delimiter_name, native_inline);
    PandocNativeIntermediate::IntermediateInline(build_inline(inlines, node))
}

/// Helper function to process emphasis-like inlines with a closure that needs node access
pub fn process_emphasis_inline_with_node<F, G>(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    native_inline: F,
    build_inline: G,
) -> PandocNativeIntermediate
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
    G: FnOnce(Vec<Inline>, &tree_sitter::Node) -> Inline,
{
    let inlines = process_emphasis_like_inline(children, delimiter_name, native_inline);
    PandocNativeIntermediate::IntermediateInline(build_inline(inlines, node))
}

/// Helper function for simple text extraction nodes
pub fn create_base_text_from_node_text(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let text = node.utf8_text(input_bytes).unwrap().to_string();
    PandocNativeIntermediate::IntermediateBaseText(text, node_location(node))
}

/// Helper function for specifiers that need first character removed
pub fn create_specifier_base_text(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let mut text = node.utf8_text(input_bytes).unwrap().to_string();
    let id = if text.len() > 1 {
        text.split_off(1)
    } else {
        String::new()
    };
    PandocNativeIntermediate::IntermediateBaseText(id, node_location(node))
}

/// Helper function to convert straight apostrophes to smart quotes
/// Converts ASCII apostrophe (') to Unicode right single quotation mark (')
pub fn apply_smart_quotes(text: String) -> String {
    text.replace('\'', "\u{2019}")
}

/// Helper function to create simple line break inlines
pub fn create_line_break_inline(
    node: &tree_sitter::Node,
    is_hard: bool,
) -> PandocNativeIntermediate {
    let range = node_location(node);
    let inline = if is_hard {
        Inline::LineBreak(LineBreak {
            source_info: SourceInfo::with_range(range),
        })
    } else {
        Inline::SoftBreak(SoftBreak {
            source_info: SourceInfo::with_range(range),
        })
    };
    PandocNativeIntermediate::IntermediateInline(inline)
}
