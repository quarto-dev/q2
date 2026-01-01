/*
 * text_helpers.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, LineBreak, SoftBreak, Space};
use crate::pandoc::location::node_location;
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
/// Handles IntermediateInlines by flattening them into the result instead of
/// wrapping them in a Span. This is important for nested emphasis where the
/// inner emphasis may return multiple inlines (e.g., Space + Strong when the
/// delimiter captures a leading space).
pub fn process_emphasis_like_inline<F>(
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    mut native_inline: F,
) -> Vec<Inline>
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
{
    let mut result = Vec::new();
    for (node_name, child) in filter_delimiter_children(children, delimiter_name) {
        match child {
            // Flatten IntermediateInlines instead of passing through native_inline
            // which would wrap multiple inlines in a Span
            PandocNativeIntermediate::IntermediateInlines(inlines) => {
                result.extend(inlines);
            }
            other => {
                result.push(native_inline((node_name, other)));
            }
        }
    }
    result
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

/// Process backslash escapes in text according to Pandoc rules
/// A backslash before any ASCII punctuation character is treated as an escape
/// and the backslash is removed, leaving only the escaped character.
///
/// According to Pandoc spec, these characters can be escaped:
/// !"#$%&'()*+,-./:;<=>?@[\]^_`{|}~
pub fn process_backslash_escapes(text: String) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Check if next character is ASCII punctuation
            if let Some(&next_ch) = chars.peek() {
                if is_escapable_punctuation(next_ch) {
                    // This is an escape sequence - skip the backslash and include the character
                    chars.next(); // consume the next character
                    result.push(next_ch);
                } else {
                    // Not an escape sequence - keep the backslash
                    result.push(ch);
                }
            } else {
                // Backslash at end of string - keep it
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Check if a character is ASCII punctuation that can be escaped
fn is_escapable_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | '+'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

/// Helper function to create simple line break inlines
pub fn create_line_break_inline(
    node: &tree_sitter::Node,
    is_hard: bool,
) -> PandocNativeIntermediate {
    let range = node_location(node);
    let inline = if is_hard {
        Inline::LineBreak(LineBreak {
            source_info: quarto_source_map::SourceInfo::from_range(
                quarto_source_map::FileId(0),
                range,
            ),
        })
    } else {
        Inline::SoftBreak(SoftBreak {
            source_info: quarto_source_map::SourceInfo::from_range(
                quarto_source_map::FileId(0),
                range,
            ),
        })
    };
    PandocNativeIntermediate::IntermediateInline(inline)
}

/// Helper function to process inline nodes with delimiter-based space handling.
/// This is used for emphasis, strong, strikeout, superscript, and subscript nodes
/// which may capture spaces in their delimiters that need to be injected as Space nodes.
///
/// # Parameters
/// - `node`: The tree-sitter node being processed
/// - `children`: The children of the node
/// - `delimiter_name`: The name of the delimiter node to scan (e.g., "emphasis_delimiter")
/// - `input_bytes`: The input source bytes (needed to extract delimiter text)
/// - `context`: The AST context
/// - `native_inline`: Function to recursively process inline nodes
/// - `create_inline`: Closure to create the final inline element from processed inlines
///
/// # Returns
/// IntermediateInlines containing the inline element, potentially wrapped with Space nodes
pub fn process_inline_with_delimiter_spaces<F, G>(
    _node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    input_bytes: &[u8],
    context: &ASTContext,
    native_inline: F,
    create_inline: G,
) -> PandocNativeIntermediate
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
    G: FnOnce(Vec<Inline>, quarto_source_map::SourceInfo) -> Inline,
{
    // Scan delimiters to check for captured spaces and save their ranges
    let mut leading_space_range: Option<quarto_source_map::Range> = None;
    let mut trailing_space_range: Option<quarto_source_map::Range> = None;
    let mut first_delimiter_range: Option<quarto_source_map::Range> = None;
    let mut last_delimiter_range: Option<quarto_source_map::Range> = None;
    let mut leading_ws_count = 0;
    let mut trailing_ws_count = 0;
    let mut first_delimiter = true;

    for (node_name, child) in &children {
        if node_name == delimiter_name
            && let PandocNativeIntermediate::IntermediateUnknown(range) = child
        {
            let text =
                std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset]).unwrap();

            if first_delimiter {
                first_delimiter_range = Some(range.clone());
                // Opening delimiter - check for leading space
                if text.starts_with(char::is_whitespace) {
                    // Count leading whitespace characters
                    leading_ws_count = text.chars().take_while(|c| c.is_whitespace()).count();
                    // Calculate the range for just the leading whitespace
                    let ws_end_offset = range.start.offset + leading_ws_count;
                    leading_space_range = Some(quarto_source_map::Range {
                        start: quarto_source_map::Location {
                            offset: range.start.offset,
                            row: range.start.row,
                            column: range.start.column,
                        },
                        end: quarto_source_map::Location {
                            offset: ws_end_offset,
                            row: range.start.row,
                            column: range.start.column + leading_ws_count,
                        },
                    });
                }
                first_delimiter = false;
            } else {
                last_delimiter_range = Some(range.clone());
                // Closing delimiter - check for trailing space
                if text.ends_with(char::is_whitespace) {
                    // Count trailing whitespace characters
                    trailing_ws_count =
                        text.chars().rev().take_while(|c| c.is_whitespace()).count();
                    // Calculate the range for just the trailing whitespace
                    let ws_start_offset = range.end.offset - trailing_ws_count;
                    trailing_space_range = Some(quarto_source_map::Range {
                        start: quarto_source_map::Location {
                            offset: ws_start_offset,
                            row: range.end.row,
                            column: range.end.column - trailing_ws_count,
                        },
                        end: quarto_source_map::Location {
                            offset: range.end.offset,
                            row: range.end.row,
                            column: range.end.column,
                        },
                    });
                }
            }
        }
    }

    // Calculate the adjusted range for the inline element (excluding delimiter spaces)
    let adjusted_range = if let (Some(first_delim), Some(last_delim)) =
        (&first_delimiter_range, &last_delimiter_range)
    {
        quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: first_delim.start.offset + leading_ws_count,
                row: first_delim.start.row,
                column: first_delim.start.column + leading_ws_count,
            },
            end: quarto_source_map::Location {
                offset: last_delim.end.offset - trailing_ws_count,
                row: last_delim.end.row,
                column: last_delim.end.column - trailing_ws_count,
            },
        }
    } else {
        // Fallback to node range if we don't have delimiter ranges
        crate::pandoc::location::node_location(_node)
    };

    // Build the inline element using existing helper
    let inlines = process_emphasis_like_inline(children, delimiter_name, native_inline);
    let adjusted_source_info =
        quarto_source_map::SourceInfo::from_range(context.current_file_id(), adjusted_range);
    let inline = create_inline(inlines, adjusted_source_info);

    // Build result with injected Space nodes as needed
    let mut result = Vec::new();

    if let Some(space_range) = leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    result.push(inline);

    if let Some(space_range) = trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
