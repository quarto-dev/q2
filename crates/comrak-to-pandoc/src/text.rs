/*
 * text.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Text tokenization: convert comrak's single Text nodes into
 * Pandoc's Str + Space token sequence.
 */

use crate::empty_source_info;
use quarto_pandoc_types::{Inline, Inlines, Space, Str};
use quarto_source_map::{FileId, SourceInfo};

/// Tokenize a text string into Pandoc inlines.
///
/// Comrak represents "hello world" as a single Text node.
/// Pandoc expects: [Str("hello"), Space, Str("world")]
///
/// This function:
/// - Splits on whitespace
/// - Collapses multiple whitespace to single Space
/// - Preserves leading/trailing whitespace as Space inlines
///   (important when text is adjacent to other inlines like Emph)
/// - Pure whitespace text (e.g., " ") produces [Space]
///   (important when whitespace is between inlines like Code spans)
pub fn tokenize_text(text: &str) -> Inlines {
    let mut result = Vec::new();
    let mut current_word = String::new();
    let mut in_whitespace = false;
    let mut seen_non_whitespace = false;
    let mut seen_whitespace = false;

    for c in text.chars() {
        if c.is_whitespace() {
            // Emit accumulated word
            if !current_word.is_empty() {
                result.push(Inline::Str(Str {
                    text: std::mem::take(&mut current_word),
                    source_info: empty_source_info(),
                }));
            }
            // Mark that we're in whitespace (will emit Space)
            in_whitespace = true;
            seen_whitespace = true;
        } else {
            // Emit space if we were in whitespace
            // (either between words, or leading space at start)
            if in_whitespace {
                result.push(Inline::Space(Space {
                    source_info: empty_source_info(),
                }));
            }
            in_whitespace = false;
            seen_non_whitespace = true;
            current_word.push(c);
        }
    }

    // Emit final word
    if !current_word.is_empty() {
        result.push(Inline::Str(Str {
            text: current_word,
            source_info: empty_source_info(),
        }));
    }

    // Emit trailing space if we ended in whitespace and had content before
    if in_whitespace && seen_non_whitespace {
        result.push(Inline::Space(Space {
            source_info: empty_source_info(),
        }));
    }

    // Special case: pure whitespace (no words) should produce a single Space
    // This handles Text(" ") nodes between inlines like [Code, Text(" "), Code]
    if result.is_empty() && seen_whitespace {
        result.push(Inline::Space(Space {
            source_info: empty_source_info(),
        }));
    }

    result
}

/// Tokenize a text string into Pandoc inlines with source tracking.
///
/// This version tracks byte offsets for each resulting inline element.
///
/// - `text`: The text content to tokenize
/// - `base_offset`: Byte offset where this text starts in the source file
/// - `file_id`: File identifier for SourceInfo
pub fn tokenize_text_with_source(text: &str, base_offset: usize, file_id: FileId) -> Inlines {
    let mut result = Vec::new();
    let mut current_word = String::new();
    let mut current_word_start: Option<usize> = None;
    let mut whitespace_start: Option<usize> = None;
    let mut seen_non_whitespace = false;

    for (byte_idx, c) in text.char_indices() {
        let abs_offset = base_offset + byte_idx;

        if c.is_whitespace() {
            // Emit accumulated word
            if !current_word.is_empty() {
                let start = current_word_start.unwrap();
                let end = abs_offset;
                result.push(Inline::Str(Str {
                    text: std::mem::take(&mut current_word),
                    source_info: SourceInfo::original(file_id, start, end),
                }));
                current_word_start = None;
            }
            // Track whitespace start
            if whitespace_start.is_none() {
                whitespace_start = Some(abs_offset);
            }
        } else {
            // Emit space if we were in whitespace
            if let Some(ws_start) = whitespace_start {
                result.push(Inline::Space(Space {
                    source_info: SourceInfo::original(file_id, ws_start, abs_offset),
                }));
                whitespace_start = None;
            }
            // Track word start
            if current_word_start.is_none() {
                current_word_start = Some(abs_offset);
            }
            seen_non_whitespace = true;
            current_word.push(c);
        }
    }

    // Handle remaining content at end of string
    let end_offset = base_offset + text.len();

    if !current_word.is_empty() {
        let start = current_word_start.unwrap();
        result.push(Inline::Str(Str {
            text: current_word,
            source_info: SourceInfo::original(file_id, start, end_offset),
        }));
    } else if let Some(ws_start) = whitespace_start {
        // Trailing whitespace
        if seen_non_whitespace {
            result.push(Inline::Space(Space {
                source_info: SourceInfo::original(file_id, ws_start, end_offset),
            }));
        } else {
            // Pure whitespace text
            result.push(Inline::Space(Space {
                source_info: SourceInfo::original(file_id, base_offset, end_offset),
            }));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_str_text(inline: &Inline) -> Option<&str> {
        match inline {
            Inline::Str(s) => Some(&s.text),
            _ => None,
        }
    }

    fn is_space(inline: &Inline) -> bool {
        matches!(inline, Inline::Space(_))
    }

    #[test]
    fn test_single_word() {
        let result = tokenize_text("hello");
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
    }

    #[test]
    fn test_two_words() {
        let result = tokenize_text("hello world");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world"));
    }

    #[test]
    fn test_multiple_spaces() {
        let result = tokenize_text("hello   world");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world"));
    }

    #[test]
    fn test_leading_space() {
        // Leading space should become Space inline
        // (important when text follows other inlines like Emph)
        let result = tokenize_text(" hello");
        assert_eq!(result.len(), 2);
        assert!(is_space(&result[0]));
        assert_eq!(get_str_text(&result[1]), Some("hello"));
    }

    #[test]
    fn test_trailing_space() {
        // Trailing space should become Space inline
        // (important when text precedes other inlines like Emph)
        let result = tokenize_text("hello ");
        assert_eq!(result.len(), 2);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
    }

    #[test]
    fn test_empty_string() {
        let result = tokenize_text("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_only_spaces() {
        // Pure whitespace should produce a single Space inline
        // (important for Text(" ") nodes between inlines like Code spans)
        let result = tokenize_text("   ");
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
    }

    #[test]
    fn test_single_space() {
        // Single space produces one Space inline
        let result = tokenize_text(" ");
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
    }

    #[test]
    fn test_punctuation() {
        let result = tokenize_text("hello, world!");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello,"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world!"));
    }

    // Tests for tokenize_text_with_source

    fn get_source_offsets(inline: &Inline) -> (usize, usize) {
        match inline {
            Inline::Str(s) => (s.source_info.start_offset(), s.source_info.end_offset()),
            Inline::Space(sp) => (sp.source_info.start_offset(), sp.source_info.end_offset()),
            _ => panic!("Unexpected inline type"),
        }
    }

    #[test]
    fn test_source_single_word() {
        let result = tokenize_text_with_source("hello", 10, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        // "hello" at base offset 10, length 5
        assert_eq!(get_source_offsets(&result[0]), (10, 15));
    }

    #[test]
    fn test_source_two_words() {
        // "hello world" at offset 0
        let result = tokenize_text_with_source("hello world", 0, FileId(0));
        assert_eq!(result.len(), 3);

        // "hello" at 0..5
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert_eq!(get_source_offsets(&result[0]), (0, 5));

        // Space at 5..6
        assert!(is_space(&result[1]));
        assert_eq!(get_source_offsets(&result[1]), (5, 6));

        // "world" at 6..11
        assert_eq!(get_str_text(&result[2]), Some("world"));
        assert_eq!(get_source_offsets(&result[2]), (6, 11));
    }

    #[test]
    fn test_source_utf8() {
        // "héllo" - é is 2 bytes, total 6 bytes
        let result = tokenize_text_with_source("héllo", 0, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("héllo"));
        assert_eq!(get_source_offsets(&result[0]), (0, 6));
    }

    #[test]
    fn test_source_with_base_offset() {
        // "world" at base offset 100
        let result = tokenize_text_with_source("world", 100, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_source_offsets(&result[0]), (100, 105));
    }

    #[test]
    fn test_source_leading_space() {
        // " hello" at offset 0
        let result = tokenize_text_with_source(" hello", 0, FileId(0));
        assert_eq!(result.len(), 2);

        // Space at 0..1
        assert!(is_space(&result[0]));
        assert_eq!(get_source_offsets(&result[0]), (0, 1));

        // "hello" at 1..6
        assert_eq!(get_str_text(&result[1]), Some("hello"));
        assert_eq!(get_source_offsets(&result[1]), (1, 6));
    }

    #[test]
    fn test_source_trailing_space() {
        // "hello " at offset 0
        let result = tokenize_text_with_source("hello ", 0, FileId(0));
        assert_eq!(result.len(), 2);

        // "hello" at 0..5
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert_eq!(get_source_offsets(&result[0]), (0, 5));

        // Space at 5..6
        assert!(is_space(&result[1]));
        assert_eq!(get_source_offsets(&result[1]), (5, 6));
    }

    #[test]
    fn test_source_pure_whitespace() {
        // "   " at offset 10
        let result = tokenize_text_with_source("   ", 10, FileId(0));
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
        assert_eq!(get_source_offsets(&result[0]), (10, 13));
    }
}
