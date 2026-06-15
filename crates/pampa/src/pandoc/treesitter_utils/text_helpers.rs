/*
 * text_helpers.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, LineBreak, SoftBreak, Space};
use crate::pandoc::location::node_location;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

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

/// Helper function to extract text from string quotes.
///
/// Strips surrounding `"..."` or `'...'` and applies CommonMark/Pandoc-style
/// backslash escapes: `\X` collapses to `X` when `X` is ASCII punctuation,
/// otherwise the backslash is preserved literally. Bare (unquoted) values
/// are returned unchanged.
pub fn extract_quoted_text(text: &str) -> String {
    let is_double = text.starts_with('"') && text.ends_with('"') && text.len() >= 2;
    let is_single = text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2;
    if is_double || is_single {
        unescape_punctuation(&text[1..text.len() - 1])
    } else {
        text.to_string()
    }
}

/// Collapse `\X` to `X` when `X` is ASCII punctuation. Otherwise the
/// backslash is preserved. A trailing backslash with no following
/// character is preserved as-is.
fn unescape_punctuation(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next) if next.is_ascii_punctuation() => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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

/// Apply Pandoc "smart" typography to a prose text run: straight apostrophes
/// become curly (`'` → `’`), runs of hyphens become en/em dashes, and runs of
/// dots become ellipses.
///
/// Dash runs follow Pandoc's default `dash` parser
/// (`Text/Pandoc/Parsing/Smart.hs`): consumed left-to-right, greedily taking
/// three hyphens as an EM DASH (—) while at least three remain, then a trailing
/// pair as an EN DASH (–), leaving a lone hyphen literal. Dot runs take three
/// at a time as a HORIZONTAL ELLIPSIS (…), leaving a remainder of one or two
/// dots literal.
///
/// **Must be applied per prose-str node, before merging adjacent strings.**
/// Escaped punctuation (`\-`, `\.`) arrives from tree-sitter as its own
/// single-character node, so a single node never contains an escaped hyphen or
/// dot run; converting here (rather than after `merge_strs`) is what keeps
/// `a\-\-b` literal instead of collapsing it to an en dash.
pub fn apply_smart_typography(text: String) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\'' => {
                out.push('\u{2019}');
                i += 1;
            }
            '-' => {
                let start = i;
                while i < chars.len() && chars[i] == '-' {
                    i += 1;
                }
                let mut run = i - start;
                while run >= 3 {
                    out.push('\u{2014}'); // — em dash
                    run -= 3;
                }
                if run == 2 {
                    out.push('\u{2013}'); // – en dash
                } else if run == 1 {
                    out.push('-');
                }
            }
            '.' => {
                let start = i;
                while i < chars.len() && chars[i] == '.' {
                    i += 1;
                }
                let mut run = i - start;
                while run >= 3 {
                    out.push('\u{2026}'); // … ellipsis
                    run -= 3;
                }
                for _ in 0..run {
                    out.push('.');
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }

    out
}

/// Process backslash escapes in text according to Pandoc rules.
///
/// - A backslash before any ASCII punctuation character is treated as an
///   escape and the backslash is removed, leaving only the escaped
///   character. Pandoc-escapable characters: `!"#$%&'()*+,-./:;<=>?@[\]^_\`{|}~`.
/// - A backslash followed by an ASCII space is Pandoc's non-breaking-space
///   shorthand: the pair collapses to a single U+00A0 (NO-BREAK SPACE).
///   See <https://pandoc.org/MANUAL.html#non-breaking-spaces>.
/// - A backslash before one of the "smart typography" output characters —
///   EM DASH (—, U+2014), EN DASH (–, U+2013), or HORIZONTAL ELLIPSIS
///   (…, U+2026) — is also an escape: the backslash is dropped, leaving the
///   character literal. This is how the QMD writer round-trips an em dash that
///   would otherwise land on an all-dash line and be misread as a thematic
///   break (it emits `\—`). A deliberate, narrow divergence from Pandoc, which
///   keeps `\—` literal.
/// - Any other `\X` is left as the literal two characters.
pub fn process_backslash_escapes(text: String) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Check if next character is ASCII punctuation, an ASCII space,
            // or anything else.
            if let Some(&next_ch) = chars.peek() {
                if is_escapable_punctuation(next_ch) || is_escapable_smart_char(next_ch) {
                    // Backslash escape for a punctuation or smart-typography
                    // char: drop the backslash, emit the character.
                    chars.next();
                    result.push(next_ch);
                } else if next_ch == ' ' {
                    // Pandoc non-breaking-space shorthand: `\<space>` collapses
                    // to U+00A0. The original space is consumed.
                    chars.next();
                    result.push('\u{00A0}');
                } else {
                    // Not an escape sequence - keep the backslash.
                    result.push(ch);
                }
            } else {
                // Backslash at end of string - keep it.
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

/// Smart-typography output characters that a backslash may escape (D5): the em
/// dash, en dash, and horizontal ellipsis. See `process_backslash_escapes`.
fn is_escapable_smart_char(ch: char) -> bool {
    matches!(ch, '\u{2014}' | '\u{2013}' | '\u{2026}')
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

/// Information about whitespace captured in delimiter tokens.
/// Used for injecting Space nodes around inline elements.
#[derive(Debug, Clone)]
pub struct DelimiterSpaceInfo {
    /// Range of leading whitespace in the opening delimiter, if any
    pub leading_space_range: Option<quarto_source_map::Range>,
    /// Range of trailing whitespace in the closing delimiter, if any
    pub trailing_space_range: Option<quarto_source_map::Range>,
    /// The adjusted source range for the inline element (excluding delimiter spaces)
    pub adjusted_range: quarto_source_map::Range,
}

/// Extract whitespace information from delimiter nodes.
///
/// This function scans the children for delimiter nodes and detects any leading
/// or trailing whitespace that was captured by the delimiter tokens. This is used
/// for emphasis, strong, strikeout, superscript, subscript, and editorial marks
/// which may capture spaces in their delimiters that need to be injected as Space nodes.
///
/// # Parameters
/// - `children`: The children of the node (borrowed for scanning)
/// - `delimiter_name`: The name of the delimiter node to scan (e.g., "emphasis_delimiter", "delete_delimiter")
/// - `input_bytes`: The input source bytes (needed to extract delimiter text)
/// - `fallback_range`: Range to use if delimiters are not found
///
/// # Returns
/// DelimiterSpaceInfo containing the space ranges and adjusted element range
pub fn extract_delimiter_space_info(
    children: &[(String, PandocNativeIntermediate)],
    delimiter_name: &str,
    input_bytes: &[u8],
    fallback_range: quarto_source_map::Range,
) -> DelimiterSpaceInfo {
    let mut leading_space_range: Option<quarto_source_map::Range> = None;
    let mut trailing_space_range: Option<quarto_source_map::Range> = None;
    let mut first_delimiter_range: Option<quarto_source_map::Range> = None;
    let mut last_delimiter_range: Option<quarto_source_map::Range> = None;
    let mut leading_ws_count = 0;
    let mut trailing_ws_count = 0;
    let mut first_delimiter = true;

    for (node_name, child) in children {
        if node_name == delimiter_name
            && let PandocNativeIntermediate::IntermediateUnknown(range) = child
        {
            let text =
                std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset]).unwrap();

            if first_delimiter {
                first_delimiter_range = Some(range.clone());
                // Opening delimiter - check for leading space
                if text.starts_with(|c: char| c.is_ascii_whitespace()) {
                    // Count leading whitespace characters
                    leading_ws_count = text
                        .chars()
                        .take_while(|c: &char| c.is_ascii_whitespace())
                        .count();
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
                if text.ends_with(|c: char| c.is_ascii_whitespace()) {
                    // Count trailing whitespace characters
                    trailing_ws_count = text
                        .chars()
                        .rev()
                        .take_while(|c: &char| c.is_ascii_whitespace())
                        .count();
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
        fallback_range
    };

    DelimiterSpaceInfo {
        leading_space_range,
        trailing_space_range,
        adjusted_range,
    }
}

/// Wrap an inline element with Space nodes based on delimiter space info.
///
/// # Parameters
/// - `inline`: The inline element to wrap
/// - `space_info`: The delimiter space information
/// - `context`: The AST context for creating SourceInfo
///
/// # Returns
/// IntermediateInlines containing the inline element, potentially wrapped with Space nodes
pub fn wrap_inline_with_delimiter_spaces(
    inline: Inline,
    space_info: &DelimiterSpaceInfo,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut result = Vec::new();

    if let Some(space_range) = &space_info.leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range.clone(),
            ),
        }));
    }

    result.push(inline);

    if let Some(space_range) = &space_info.trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range.clone(),
            ),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
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
    node: &tree_sitter::Node,
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
    // Extract delimiter space information
    let space_info =
        extract_delimiter_space_info(&children, delimiter_name, input_bytes, node_location(node));

    // Build the inline element using existing helper
    let inlines = process_emphasis_like_inline(children, delimiter_name, native_inline);
    let adjusted_source_info = quarto_source_map::SourceInfo::from_range(
        context.current_file_id(),
        space_info.adjusted_range.clone(),
    );
    let inline = create_inline(inlines, adjusted_source_info);

    // Wrap with Space nodes as needed
    wrap_inline_with_delimiter_spaces(inline, &space_info, context)
}

#[cfg(test)]
mod tests {
    use super::extract_quoted_text;

    #[test]
    fn double_quoted_escape_punctuation() {
        // CommonMark/Pandoc rule: backslash before ASCII punctuation collapses.
        assert_eq!(extract_quoted_text(r#""\[1,2\]""#), "[1,2]");
        assert_eq!(extract_quoted_text(r#""a\"b""#), "a\"b");
        assert_eq!(extract_quoted_text(r#""a\\b""#), "a\\b");
    }

    #[test]
    fn double_quoted_no_escape_nonpunctuation() {
        // Backslash before a non-punctuation char is preserved.
        assert_eq!(extract_quoted_text(r#""a\bc""#), "a\\bc");
    }

    #[test]
    fn single_quoted_escape_punctuation() {
        assert_eq!(extract_quoted_text(r"'a\'b'"), "a'b");
        assert_eq!(extract_quoted_text(r"'a\\b'"), "a\\b");
    }

    #[test]
    fn unquoted_text_passthrough() {
        // Bare values get no escape processing.
        assert_eq!(extract_quoted_text(r"a\b"), "a\\b");
        assert_eq!(extract_quoted_text("plain"), "plain");
    }

    #[test]
    fn trailing_backslash_preserved() {
        // A dangling backslash with nothing after it stays literal.
        assert_eq!(extract_quoted_text(r#""abc\""#), "abc\\");
    }
}

#[cfg(test)]
mod smart_typography_tests {
    use super::{apply_smart_typography, process_backslash_escapes};

    const EN: &str = "\u{2013}"; // –
    const EM: &str = "\u{2014}"; // —
    const ELL: &str = "\u{2026}"; // …
    const RSQUO: &str = "\u{2019}"; // ’

    #[test]
    fn dash_runs_match_pandoc() {
        // Greedy: 3=em while >=3 remain, trailing 2=en, lone 1=hyphen.
        assert_eq!(apply_smart_typography("-".into()), "-");
        assert_eq!(apply_smart_typography("--".into()), EN);
        assert_eq!(apply_smart_typography("---".into()), EM);
        assert_eq!(apply_smart_typography("----".into()), format!("{EM}-"));
        assert_eq!(apply_smart_typography("-----".into()), format!("{EM}{EN}"));
        assert_eq!(apply_smart_typography("------".into()), format!("{EM}{EM}"));
        assert_eq!(
            apply_smart_typography("-------".into()),
            format!("{EM}{EM}-")
        );
    }

    #[test]
    fn dash_mid_word_converts() {
        assert_eq!(
            apply_smart_typography("un---spaced".into()),
            format!("un{EM}spaced")
        );
        assert_eq!(
            apply_smart_typography("en--dash".into()),
            format!("en{EN}dash")
        );
    }

    #[test]
    fn single_intraword_hyphen_preserved() {
        assert_eq!(apply_smart_typography("well-known".into()), "well-known");
    }

    #[test]
    fn ellipsis_runs() {
        assert_eq!(apply_smart_typography("...".into()), ELL);
        assert_eq!(
            apply_smart_typography("Wait...".into()),
            format!("Wait{ELL}")
        );
        assert_eq!(apply_smart_typography("....".into()), format!("{ELL}."));
        assert_eq!(
            apply_smart_typography("......".into()),
            format!("{ELL}{ELL}")
        );
        // Only two dots: not an ellipsis, left literal.
        assert_eq!(apply_smart_typography("..".into()), "..");
    }

    #[test]
    fn apostrophe_becomes_smart_quote() {
        assert_eq!(
            apply_smart_typography("don't".into()),
            format!("don{RSQUO}t")
        );
    }

    #[test]
    fn backslash_escapes_smart_chars() {
        // D5: backslash strips before em/en-dash/ellipsis so `\—` → `—`.
        assert_eq!(process_backslash_escapes(format!("\\{EM}")), EM);
        assert_eq!(process_backslash_escapes(format!("\\{EN}")), EN);
        assert_eq!(process_backslash_escapes(format!("\\{ELL}")), ELL);
    }

    #[test]
    fn backslash_escapes_ascii_punct_still_work() {
        assert_eq!(process_backslash_escapes("\\*".into()), "*");
        assert_eq!(process_backslash_escapes("a\\-b".into()), "a-b");
        assert_eq!(process_backslash_escapes("\\ ".into()), "\u{00A0}");
    }
}
