/*
 * text.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Text tokenization: convert comrak's single Text nodes into
 * Pandoc's Str + Space token sequence.
 */

use crate::empty_source_info;
use quarto_pandoc_types::{Inline, Inlines, Space, Str};

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
}
