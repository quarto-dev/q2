/*
 * engine/knitr/preprocess.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Preprocessing for knitr engine input.
 */

//! Preprocessing utilities for knitr engine input.
//!
//! This module handles transformations of the markdown content before
//! sending it to R/knitr for execution.
//!
//! # Inline R Expressions
//!
//! Inline R expressions like `` `r 1+1` `` are transformed to use the
//! `.QuartoInlineRender()` wrapper function, which handles proper escaping
//! of special markdown characters in the output.
//!
//! ```text
//! Before: The answer is `r 1+1`.
//! After:  The answer is `r .QuartoInlineRender(1+1)`.
//! ```

use regex::Regex;
use std::sync::LazyLock;

/// Regex pattern for inline R code: `r expression`
///
/// Matches:
/// - Opening backtick
/// - Literal 'r' followed by one or more whitespace characters
/// - The expression (captured group 1) - any characters except backticks
/// - Closing backtick
///
/// This pattern intentionally avoids matching inside fenced code blocks
/// because we process the entire markdown string and inline R is not valid
/// inside code blocks.
static INLINE_R_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern breakdown:
    // `r\s+   - Opening backtick, 'r', whitespace
    // ([^`]+) - Capture the expression (anything except backticks)
    // `       - Closing backtick
    Regex::new(r"`r\s+([^`]+)`").expect("Invalid regex pattern for inline R")
});

/// Resolve inline R expressions by wrapping them with `.QuartoInlineRender()`.
///
/// Transforms `` `r expr` `` to `` `r .QuartoInlineRender(expr)` ``.
///
/// The `.QuartoInlineRender()` wrapper function (defined in execute.R) handles:
/// - Proper escaping of special markdown characters
/// - Conversion of NULL to "NULL" string
/// - Handling of `AsIs` class objects
/// - Vector formatting
///
/// # Arguments
///
/// * `markdown` - The markdown content to preprocess
///
/// # Returns
///
/// The markdown with inline R expressions wrapped.
///
/// # Examples
///
/// ```ignore
/// let input = "The answer is `r 1+1`.";
/// let output = resolve_inline_r_expressions(input);
/// assert_eq!(output, "The answer is `r .QuartoInlineRender(1+1)`.");
/// ```
pub fn resolve_inline_r_expressions(markdown: &str) -> String {
    INLINE_R_PATTERN
        .replace_all(markdown, |caps: &regex::Captures| {
            let expr = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            // Trim the expression to normalize whitespace
            let trimmed = expr.trim();
            if trimmed.is_empty() {
                // Empty expressions are left as-is (they'll produce an R error)
                caps[0].to_string()
            } else {
                format!("`r .QuartoInlineRender({})`", trimmed)
            }
        })
        .into_owned()
}

/// Check if the markdown contains any inline R expressions.
///
/// This can be used to skip preprocessing if there's nothing to process.
///
/// # Arguments
///
/// * `markdown` - The markdown content to check
///
/// # Returns
///
/// `true` if the markdown contains inline R expressions.
pub fn has_inline_r_expressions(markdown: &str) -> bool {
    INLINE_R_PATTERN.is_match(markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === resolve_inline_r_expressions tests ===

    #[test]
    fn test_simple_inline_r() {
        let input = "The answer is `r 1+1`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "The answer is `r .QuartoInlineRender(1+1)`.");
    }

    #[test]
    fn test_multiple_inline_r() {
        let input = "First `r x` then `r y` and finally `r z`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "First `r .QuartoInlineRender(x)` then `r .QuartoInlineRender(y)` and finally `r .QuartoInlineRender(z)`."
        );
    }

    #[test]
    fn test_inline_r_with_complex_expression() {
        let input = "The mean is `r mean(c(1, 2, 3))`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "The mean is `r .QuartoInlineRender(mean(c(1, 2, 3)))`."
        );
    }

    #[test]
    fn test_inline_r_with_whitespace() {
        // Extra whitespace around the expression should be trimmed
        let input = "Value: `r   x + 1   `.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r .QuartoInlineRender(x + 1)`.");
    }

    #[test]
    fn test_no_inline_r() {
        let input = "No R code here, just `code` and `more code`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_inline_code_without_r() {
        // Regular inline code (without 'r ') should not be transformed
        let input = "Use `print()` to output.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_inline_r_at_start() {
        let input = "`r x` is the value.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "`r .QuartoInlineRender(x)` is the value.");
    }

    #[test]
    fn test_inline_r_at_end() {
        let input = "The value is `r x`";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "The value is `r .QuartoInlineRender(x)`");
    }

    #[test]
    fn test_inline_r_multiline() {
        let input = "First line `r a`.\nSecond line `r b`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "First line `r .QuartoInlineRender(a)`.\nSecond line `r .QuartoInlineRender(b)`."
        );
    }

    #[test]
    fn test_inline_r_with_string_literal() {
        // String literals inside expressions should work
        let input = r#"Name: `r paste("Hello", "World")`."#;
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            r#"Name: `r .QuartoInlineRender(paste("Hello", "World"))`."#
        );
    }

    #[test]
    fn test_inline_r_preserves_surrounding_text() {
        let input = "Before `r x` middle `r y` after";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "Before `r .QuartoInlineRender(x)` middle `r .QuartoInlineRender(y)` after"
        );
    }

    #[test]
    fn test_uppercase_r_not_matched() {
        // Only lowercase 'r' should be matched
        let input = "This `R code` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_r_without_space_not_matched() {
        // 'r' must be followed by whitespace
        let input = "This `rx` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    // === has_inline_r_expressions tests ===

    #[test]
    fn test_has_inline_r_true() {
        assert!(has_inline_r_expressions("Value: `r x`."));
    }

    #[test]
    fn test_has_inline_r_false() {
        assert!(!has_inline_r_expressions("No inline R here."));
        assert!(!has_inline_r_expressions("Just `code` here."));
    }

    #[test]
    fn test_has_inline_r_empty_string() {
        assert!(!has_inline_r_expressions(""));
    }
}
