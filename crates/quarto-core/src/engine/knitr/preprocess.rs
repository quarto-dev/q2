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
//!
//! # This pass shifts byte offsets
//!
//! It runs on the output of `serialize_ast_to_qmd`, *after* the `SourceInfo`
//! handed to `ExecutionContext` was built from that string
//! (`stage/stages/engine_execution.rs:432,467`). Every wrapped expression
//! makes the text 21 bytes longer, so offsets past it no longer agree with
//! that `SourceInfo`. This is harmless today — `ctx.source_info` is read by
//! the jupyter/ts engines and never by knitr — but anyone giving knitr real
//! source locations must reconcile the two first, or the locations will be
//! silently wrong in exactly the documents that use inline R.

use regex::Regex;
use std::sync::LazyLock;

/// Regex pattern for inline R code: `` `r expression` ``
///
/// Matches:
/// - Start of input, or a single character that is neither a backtick nor a
///   backslash (captured group 1, re-emitted verbatim by the replacement)
/// - Opening backtick
/// - Literal 'r' followed by exactly one space or tab
/// - The expression (captured group 2) — any characters except backticks,
///   which may span lines
/// - Closing backtick
///
/// # Why the guard on group 1, and why `[ \t]` rather than `\s+`
///
/// This pass runs over the **entire** serialized document — front matter
/// included — so it must not mistake a fenced code block for an inline
/// expression. Without both guards it does exactly that: against a display
/// fence `` ```r `` the match anchors on the fence's *third* backtick, `\s+`
/// consumes the newline, and `[^`]+` swallows the block body up to the
/// closing fence, producing `` ```r .QuartoInlineRender(<entire body>)`` ``
/// and a fatal parse error that costs the whole page
/// (bd-knitr-inline-r-eats-fence-2ofk91x1).
///
/// That is not avoidable upstream: the qmd writer collapses `` ``` r ``,
/// `` ```{.r} `` and `` ```r `` into the single spelling `` ```r ``, so
/// every author spelling arrives here as the dangerous one.
///
/// The two guards are independent, and they are **not** interchangeable —
/// each defends cases the other does not:
///
/// - `(^|[^`\\])` — neither a backtick nor a backslash may precede the match.
///   The backtick half is Quarto 1's guard from `src/core/execute-inline.ts`.
///   **This is the load-bearing fence defense, and it covers every fence
///   shape**, three backticks or forty: the backtick that would anchor a
///   match is always itself preceded by a backtick. The backslash half is
///   ours, for the case where a fence spelling does *not* reach us with its
///   backticks adjacent — see below. Removing this guard reddens
///   `test_fence_inside_yaml_scalar_not_matched`,
///   `test_escaped_backtick_fence_not_matched` and
///   `test_backtick_prefixed_not_matched`.
/// - `[ \t]` — a single space or tab, so a newline can never open an
///   expression. This is knitr parity (its class is `[ #]`) plus
///   defense-in-depth; it is *not* what stops a fence. Its own regression
///   case is a mid-prose `` `r\nx` ``, which no fence guard would catch:
///   removing it reddens `test_newline_after_r_not_matched` and nothing else.
///
/// knitr implements the same idea with two negative lookbehinds plus a
/// `[ #]` class (`knitr::all_patterns$md$inline.code`). Rust's `regex` crate
/// has no lookbehind, and the prefix-capture form above is in fact *stronger*
/// than knitr's: knitr anchors its lookbehinds to line starts, so a fence
/// spelling inside a front-matter scalar is mid-line and knitr still eats it,
/// while `(^|[^`])` rejects it.
///
/// # Why a backslash also disqualifies
///
/// An escaped backtick cannot open a code span, so it cannot open an inline R
/// expression. That would be reason enough, but there is a concrete case:
/// when a YAML scalar fails to parse as markdown, the
/// `.yaml-markdown-syntax-error` fallback re-serializes it with every
/// backtick escaped, so
///
/// ```text
/// title: "In the title: ```r blocks"
/// ```
///
/// reaches this pass as
///
/// ```text
/// title: "[In the title: \`\`\`r blocks]{.yaml-markdown-syntax-error}"
/// ```
///
/// The three backticks are no longer adjacent — each is preceded by a
/// backslash — so a guard that only excludes backticks lets the third one
/// anchor a match that runs into the resolved YAML. Neither Quarto 1 nor
/// knitr excludes the backslash, and neither survives this input.
///
/// The known cost is a false negative on `` \\`r x` `` — an escaped
/// *backslash* followed by a genuine inline expression — which this pattern
/// declines to evaluate. Distinguishing that from `` \`r x` `` needs a count
/// of preceding backslashes, which a regex of this shape cannot express;
/// declining a vanishingly rare expression is much the cheaper failure than
/// losing the page.
///
/// Note `^` is start-of-input, not start-of-line — deliberately, matching
/// Quarto 1. A line-initial expression is preceded by `\n`, which the
/// `[^`]` branch accepts and the replacement re-emits.
///
/// **Do not make this case-insensitive.** `` ```R `` currently renders,
/// unhighlighted: the highlight registry resolves a language by exact map
/// hit and knows only `r` (a separate, deliberately unfiled defect — see
/// `claude-notes/plans/2026-08-25-r-display-fence-parse-error.md` §(a)).
/// A case-insensitive pattern here would make that spelling fatal too.
static INLINE_R_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern breakdown:
    // (^|[^`\\]) - Start of input, or one character that is neither a
    //              backtick nor a backslash (re-emitted verbatim)
    // `r[ \t]    - Opening backtick, 'r', exactly one space or tab
    // ([^`]+)    - Capture the expression (anything except backticks)
    // `          - Closing backtick
    Regex::new(r"(^|[^`\\])`r[ \t]([^`]+)`").expect("Invalid regex pattern for inline R")
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
            // Group 1 is the guard character (empty at start of input); it is
            // part of the match only so that a backtick or backslash can be
            // excluded, so it must be re-emitted verbatim.
            let prefix = caps.get(1).map_or("", |m| m.as_str());
            let expr = caps.get(2).map_or("", |m| m.as_str());
            // Trim the expression to normalize whitespace
            let trimmed = expr.trim();
            if trimmed.is_empty() {
                // Empty expressions are left as-is (they'll produce an R error)
                caps[0].to_string()
            } else {
                format!("{}`r .QuartoInlineRender({})`", prefix, trimmed)
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

    // === fenced-code-block guard tests (bd-knitr-inline-r-eats-fence-2ofk91x1) ===
    //
    // A fence's third backtick must never anchor an inline-R match. The qmd
    // writer collapses `` ``` r ``, `` ```{.r} `` and `` ```r `` to the same
    // `` ```r ``, so this is the only spelling the preprocessor ever sees and
    // there is no source form an author could migrate to.

    #[test]
    fn test_display_fence_not_matched() {
        let input = "```r\npak::pak(c(\"usethis\", \"cli\"))\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input, "a display fence must be left untouched");
    }

    #[test]
    fn test_display_fence_with_following_text_not_matched() {
        // The realistic shape: a fence, then prose. Without the guard the
        // match runs from the fence's third backtick to the closing fence's
        // first, swallowing the whole body.
        let input = "Before.\n\n```r\n1 + 1\n```\n\nAfter.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_four_backtick_display_fence_not_matched() {
        // The writer widens the fence when the body contains a backtick.
        // knitr's own lookbehinds miss this shape — they are anchored to
        // ``^`` `` / ``\n`` ``, which a *fourth* backtick does not satisfy.
        // Our prefix guard is not anchored, so it covers this like any other
        // fence; the case is here as a recurrence guard, not to pin a
        // particular guard.
        let input = "````r\nx <- `y`\n````";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fence_inside_yaml_scalar_not_matched() {
        // The pass runs over the whole serialized document, front matter
        // included. A fence spelling inside a scalar is mid-line, so knitr's
        // line-anchored lookbehinds would miss it — the non-backtick prefix
        // guard does not.
        // Needs a later backtick in the document for the runaway match to
        // find a closing delimiter — the executable cell every such document
        // has, which is what makes this reachable in practice.
        let input = "---\ntitle: \"In the title: ```r blocks\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_escaped_backtick_fence_not_matched() {
        // The shape the preprocessor actually receives when a YAML scalar
        // fails to parse as markdown: the `.yaml-markdown-syntax-error`
        // fallback re-serializes the text with every backtick
        // backslash-escaped, so the third backtick is preceded by `\` rather
        // than by a backtick. An escaped backtick cannot open a code span, so
        // it cannot open an inline R expression either.
        let input = "---\ntitle: \"[In the title: \\`\\`\\`r blocks]{.yaml-markdown-syntax-error}\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_backtick_prefixed_not_matched() {
        // A backtick immediately before `` `r `` can never open a legitimate
        // inline expression. Matches Quarto 1's `(^|[^`])` guard.
        let input = "Text ``r x` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_newline_after_r_not_matched() {
        // `\s+` let a newline open the expression; a single space or tab
        // cannot. This is the defect's proximate cause.
        let input = "Text `r\nx` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_inline_r_still_matched_next_to_a_fence() {
        // The guard must not cost us a real expression that shares a
        // document with a display fence.
        let input = "```r\n1 + 1\n```\n\nThe answer is `r 1+1`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "```r\n1 + 1\n```\n\nThe answer is `r .QuartoInlineRender(1+1)`."
        );
    }

    #[test]
    fn test_inline_r_with_tab_separator_still_matched() {
        // `[ \t]`, not `[ ]`: a tab keeps working, so the change is a pure
        // narrowing of the old `\s+`.
        let input = "Value: `r\tx`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r .QuartoInlineRender(x)`.");
    }

    #[test]
    fn test_multiline_expression_body_still_matched() {
        // Only the *first* character after `r` is constrained; the body may
        // still span lines.
        let input = "Value: `r sum(\n  c(1, 2)\n)`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "Value: `r .QuartoInlineRender(sum(\n  c(1, 2)\n))`."
        );
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

    #[test]
    fn test_has_inline_r_false_for_display_fence() {
        // The fast path must agree with the replacement pass, or a document
        // whose only "match" is a fence still pays for a full scan.
        assert!(!has_inline_r_expressions("```r\n1 + 1\n```"));
        assert!(!has_inline_r_expressions("````r\nx <- `y`\n````"));
    }
}
