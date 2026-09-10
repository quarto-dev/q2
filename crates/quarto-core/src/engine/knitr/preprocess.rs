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
//! # The two inline spellings
//!
//! Quarto has two spellings for an inline executable expression, and they
//! carry **deliberately different** defaults for how the resulting value is
//! inserted into the document. `docs/computations/inline-code.qmd` on
//! quarto.org specifies both.
//!
//! ```text
//! `{r} expr`   the cross-engine brace spelling; markdown specials in the
//!              value are escaped, so a value of `**bold**` renders as the
//!              literal text `**bold**`
//!
//! `r expr`     knitr's native rmarkdown spelling, which predates Quarto;
//!              the value is inserted as live markdown, so `**bold**`
//!              renders bold
//! ```
//!
//! The documentation states the relationship between them as an exact
//! equivalence: `` `r radius` `` is equivalent to `` `{r} I(radius)` ``,
//! `I()` being knitr's opt-in for "treat this as markdown".
//!
//! This pass implements the brace spelling and stays out of the way of the
//! classic one:
//!
//! ```text
//! Before: The answer is `{r} 1+1`, and so is `r 1+1`.
//! After:  The answer is `r .QuartoInlineRender(1+1)`, and so is `r 1+1`.
//! ```
//!
//! `.QuartoInlineRender()` (defined in `resources/rmd/execute.R`) is what
//! escapes markdown specials, and it is applied to the brace spelling only.
//! The classic spelling is handed to knitr unwrapped, where knitr's own
//! default inline hook inserts the value as markdown. That split is the whole
//! contract, and it is how Quarto 1 is built as well: `src/core/execute-inline.ts`
//! matches the brace form and nothing else, and `src/execute/rmd.ts` calls it
//! with the wrapper above.
//!
//! Wrapping the classic spelling as `.QuartoInlineRender(I(expr))` — the
//! documented equivalence taken literally — would express the same intent,
//! but `I(NULL)` is an error in R ("attempt to set an attribute on NULL"), so
//! it would turn `` `r NULL` `` from a rendered value into a failed render.
//! Leaving the expression alone reaches the same markdown-passthrough
//! semantics through knitr with no such edge.
//!
//! The two routes are not identical on every value: `NULL` renders as the
//! empty string through knitr's hook, where the wrapper yields the literal
//! text `NULL`. Quarto 1 splits the same way, so `` `r NULL` `` is empty and
//! `` `{r} NULL` `` is `NULL` in both implementations; `null_values_render_per_spelling`
//! in `tests/integration/knitr_inline_expressions.rs` pins both halves.
//!
//! # Two consequences of the classic spelling's markdown passthrough
//!
//! A value inserted through `` `r expr` `` reaches the document as live
//! markdown — and, since markdown admits inline HTML, as live HTML. That is
//! the documented contract and it matches Quarto 1, but it means a value
//! built from untrusted input should use the brace spelling, whose escaping
//! is the default for exactly this reason.
//!
//! Separately, and shared with Quarto 1's identical handler: two *adjacent*
//! spans like `` `{r} a``{r} b` `` evaluate only the first. The pattern
//! consumes the character before a match as its guard, so the second span has
//! no anchor left. This is inherent to the prefix-capture approach (Rust's
//! `regex` has no lookbehind) and is not a regression from either direction.
//!
//! # Why the classic spelling is rewritten at all, then
//!
//! Only to normalize its separator and trim its body. knitr's own inline
//! pattern accepts `[ #]` between the `r` and the expression, so a
//! tab-separated `` `r<TAB>x` `` would never be evaluated if it reached knitr
//! verbatim. Re-emitting it with a single space keeps that spelling working.
//! The expression itself is untouched.
//!
//! # This pass shifts byte offsets
//!
//! It runs on the output of `serialize_ast_to_qmd`, *after* the `SourceInfo`
//! handed to `ExecutionContext` was built from that string
//! (`stage/stages/engine_execution.rs:447,482`). A wrapped expression makes
//! the text longer and a trimmed one can make it shorter, so offsets past
//! either no longer agree with that `SourceInfo`. This is harmless today — `ctx.source_info` is read by the
//! jupyter/ts engines and never by knitr — but anyone giving knitr real
//! source locations must reconcile the two first, or the locations will be
//! silently wrong in exactly the documents that use inline R.

use regex::Regex;
use std::sync::LazyLock;

/// Regex pattern for an inline R expression in either spelling:
/// `` `{r} expression` `` or `` `r expression` ``.
///
/// Matches:
/// - Start of input, or a single character that is neither a backtick nor a
///   backslash (captured group 1, re-emitted verbatim by the replacement)
/// - Opening backtick
/// - The spelling marker (captured group 2): either `{r}` or a bare `r`
/// - Exactly one space or tab
/// - The expression (captured group 3) — any characters except backticks,
///   which may span lines
/// - Closing backtick
///
/// Group 2 is what the replacement branches on; see the module docs for why
/// the two spellings are rewritten differently.
///
/// # Why the guard on group 1, and why `[ \t]` rather than `\s+`
///
/// This pass runs over the **entire** serialized document — front matter
/// included — so it must not mistake a fenced code block for an inline
/// expression. Both spellings have a fence shape that would otherwise anchor
/// a match on the fence's *last* opening backtick and let `[^`]+` swallow the
/// block body up to the closing fence, producing
/// `` ```r .QuartoInlineRender(<entire body>)`` `` and a fatal parse error
/// that costs the whole page (bd-knitr-inline-r-eats-fence-2ofk91x1):
///
/// - `` ```r `` — a display fence, for the classic branch.
/// - `` ```{r} `` — an executable cell, for the brace branch. This one is in
///   *every* document the knitr engine runs, and it needs only a trailing
///   space after the `{r}` to satisfy the `[ \t]` separator.
///
/// Neither is avoidable upstream: the qmd writer collapses `` ``` r ``,
/// `` ```{.r} `` and `` ```r `` into the single spelling `` ```r ``, so every
/// author spelling arrives here as the dangerous one.
///
/// The two guards are independent, and they are **not** interchangeable —
/// each defends cases the other does not:
///
/// - `(^|[^`\\])` — neither a backtick nor a backslash may precede the match.
///   The backtick half is Quarto 1's guard from `src/core/execute-inline.ts`.
///   **This is the load-bearing fence defense, and it covers every fence
///   shape**, three backticks or forty, in both spellings: the backtick that
///   would anchor a match is always itself preceded by a backtick. The
///   backslash half is ours, for the case where a fence spelling does *not*
///   reach us with its backticks adjacent — see below. Removing this guard
///   reddens `test_fence_inside_yaml_scalar_not_matched`,
///   `test_escaped_backtick_fence_not_matched`,
///   `test_backtick_prefixed_not_matched`,
///   `test_executable_cell_fence_with_trailing_space_not_matched` and their
///   brace-spelling counterparts.
/// - `[ \t]` — a single space or tab, so a newline can never open an
///   expression. This is knitr parity (its class is `[ #]`) plus
///   defense-in-depth; it is *not* what stops a fence. Its own regression
///   cases are a mid-prose `` `r\nx` `` / `` `{r}\nx` ``, which no fence guard
///   would catch: removing it reddens `test_newline_after_r_not_matched` and
///   `test_newline_after_brace_not_matched` and nothing else.
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
    // `          - Opening backtick
    // (\{r\}|r)  - The spelling marker: brace form or classic form
    // [ \t]      - Exactly one space or tab
    // ([^`]+)    - Capture the expression (anything except backticks)
    // `          - Closing backtick
    Regex::new(r"(^|[^`\\])`(\{r\}|r)[ \t]([^`]+)`").expect("Invalid regex pattern for inline R")
});

/// The brace spelling's marker, as it appears in group 2 of the pattern.
const BRACE_MARKER: &str = "{r}";

/// Resolve inline R expressions into the form knitr will evaluate.
///
/// - `` `{r} expr` `` becomes `` `r .QuartoInlineRender(expr)` ``, so markdown
///   specials in the value are escaped.
/// - `` `r expr` `` stays `` `r expr` `` — knitr evaluates it natively and
///   inserts the value as markdown. Only the separator is normalized to a
///   single space and the expression trimmed.
///
/// See the module docs for why the two differ.
///
/// The `.QuartoInlineRender()` wrapper function (defined in execute.R) handles:
/// - Proper escaping of special markdown characters
/// - Conversion of NULL to the string "NULL"
/// - Handling of `AsIs` class objects — which is what makes the documented
///   `` `{r} I(expr)` `` opt-out work
///
/// It does *not* format vectors: it returns a non-character value unchanged,
/// and the `paste(as.character(x), collapse = ", ")` collapse is knitr's
/// default inline hook, which runs for both spellings. Numeric rounding is
/// knitr's too. The wrapper's only effect is the three bullets above.
///
/// Because the brace spelling rewrites *into* the classic spelling, this
/// function is idempotent: a second pass leaves an already-rewritten
/// expression alone rather than escaping its value twice.
///
/// # Arguments
///
/// * `markdown` - The markdown content to preprocess
///
/// # Returns
///
/// The markdown with inline R expressions resolved.
///
/// # Examples
///
/// ```ignore
/// let input = "The answer is `{r} 1+1`.";
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
            let marker = caps.get(2).map_or("", |m| m.as_str());
            let expr = caps.get(3).map_or("", |m| m.as_str());
            // Trim the expression to normalize whitespace
            let trimmed = expr.trim();
            if marker == BRACE_MARKER {
                // Wrapped even when the expression is empty, matching Quarto
                // 1's handler. `.QuartoInlineRender()` with no argument is an
                // R error ("argument \"v\" is missing"), and a loud failure is
                // what we want: leaving `` `{r}   ` `` alone renders it as a
                // literal code span with no diagnostic and exit 0, because
                // knitr's own pattern requires a literal `` `r `` and never
                // claims it. Silent non-evaluation of the brace spelling is
                // the defect this module was fixed for.
                //
                // Reachable only where the trailing whitespace survives to
                // this pass — an attribute value, where the text is written
                // through verbatim (verified: the render fails with Q1's
                // error). In prose it is not reachable, and cannot be from
                // here: the reader normalizes `` `{r}   ` `` to `` `{r}` ``
                // before serialization, so no separator remains and the
                // pattern correctly declines it. Quarto 1 errors there too,
                // because `execute-inline.ts` scans the raw source ahead of
                // any AST round-trip; that residual divergence is a property
                // of where the two passes sit, not of this branch.
                format!("{}`r .QuartoInlineRender({})`", prefix, trimmed)
            } else if trimmed.is_empty() {
                // The classic branch keeps the opposite treatment, for the
                // same reason: knitr *does* claim `` `r   ` `` and errors on
                // it, so leaving the match untouched preserves a loud failure.
                // Re-emitting it as `` `r ` `` would fall below knitr's
                // `([^`]+)` and silence it.
                caps[0].to_string()
            } else {
                format!("{}`r {}`", prefix, trimmed)
            }
        })
        .into_owned()
}

/// Check if the markdown contains any inline R expressions, in either
/// spelling.
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

    // === the brace spelling: `{r} expr` -> wrapped in .QuartoInlineRender ===

    #[test]
    fn test_brace_spelling_is_wrapped() {
        let input = "The answer is `{r} 1+1`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "The answer is `r .QuartoInlineRender(1+1)`.");
    }

    #[test]
    fn test_brace_spelling_at_start() {
        let input = "`{r} x` is the value.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "`r .QuartoInlineRender(x)` is the value.");
    }

    #[test]
    fn test_brace_spelling_at_end() {
        let input = "The value is `{r} x`";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "The value is `r .QuartoInlineRender(x)`");
    }

    #[test]
    fn test_brace_spelling_in_attribute_value() {
        // The shape that motivated the strand: an inline expression inside a
        // fenced-div attribute value.
        let input = r#"::: {#hero data-version="`{r} release_version`"}"#;
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            r#"::: {#hero data-version="`r .QuartoInlineRender(release_version)`"}"#
        );
    }

    #[test]
    fn test_brace_spelling_with_complex_expression() {
        let input = "The mean is `{r} mean(c(1, 2, 3))`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "The mean is `r .QuartoInlineRender(mean(c(1, 2, 3)))`."
        );
    }

    #[test]
    fn test_brace_spelling_with_whitespace_is_trimmed() {
        let input = "Value: `{r}   x + 1   `.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r .QuartoInlineRender(x + 1)`.");
    }

    #[test]
    fn test_brace_spelling_with_tab_separator() {
        let input = "Value: `{r}\tx`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r .QuartoInlineRender(x)`.");
    }

    #[test]
    fn test_multiple_brace_expressions() {
        let input = "First `{r} x` then `{r} y`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "First `r .QuartoInlineRender(x)` then `r .QuartoInlineRender(y)`."
        );
    }

    #[test]
    fn test_brace_spelling_with_as_is_opt_out() {
        // The documented markdown opt-in for the brace form. The wrapper must
        // be applied around it, not instead of it — `.QuartoInlineRender`
        // passes `AsIs` through unescaped, which is what makes I() work.
        let input = "Bold: `{r} I(b)`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Bold: `r .QuartoInlineRender(I(b))`.");
    }

    #[test]
    fn test_brace_spelling_with_string_literal() {
        // String literals inside the expression must survive the rewrite.
        let input = r#"Name: `{r} paste("Hello", "World")`."#;
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            r#"Name: `r .QuartoInlineRender(paste("Hello", "World"))`."#
        );
    }

    #[test]
    fn test_brace_spelling_multiline_body() {
        // Only the character right after the marker is constrained; the body
        // may still span lines.
        let input = "Value: `{r} sum(\n  c(1, 2)\n)`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "Value: `r .QuartoInlineRender(sum(\n  c(1, 2)\n))`."
        );
    }

    #[test]
    fn test_empty_brace_expression_is_wrapped_so_r_errors() {
        // `.QuartoInlineRender()` with no argument is an R error, which is
        // what Quarto 1 produces. Leaving the span alone would render it as a
        // literal code span with no diagnostic — silent non-evaluation of the
        // brace spelling, the defect this module was fixed for.
        //
        // The render-level case this protects is an attribute value, the one
        // position whose text reaches this pass with its whitespace intact;
        // see the branch's comment for why prose cannot get here.
        let input = "Empty: `{r}   `.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Empty: `r .QuartoInlineRender()`.");
    }

    #[test]
    fn test_empty_classic_expression_is_left_alone() {
        // The opposite treatment, for the same reason: knitr claims
        // `` `r   ` `` itself and errors on it, so an untouched match stays
        // loud. Re-emitting it as `` `r ` `` would fall below knitr's
        // `([^`]+)` and silence it.
        let input = "Empty: `r   `.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_uppercase_brace_r_not_matched() {
        let input = "This `{R} x` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_brace_without_separator_not_matched() {
        let input = "This `{r}x` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    // === the classic spelling: `r expr` -> left unwrapped for knitr ===

    #[test]
    fn test_classic_spelling_is_not_wrapped() {
        // Quarto 1 never touches this spelling: knitr matches it itself and
        // its default inline hook inserts the value as live markdown. Wrapping
        // it would impose the brace form's escaping default on it.
        let input = "The answer is `r 1+1`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "The answer is `r 1+1`.");
    }

    #[test]
    fn test_classic_spelling_whitespace_is_normalized() {
        // The one edit the classic branch does make. knitr's own separator
        // class is `[ #]`, so a tab-separated expression would not survive to
        // be evaluated if we handed it through untouched.
        let input = "Value: `r   x + 1   `.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r x + 1`.");
    }

    #[test]
    fn test_classic_spelling_with_tab_separator_is_normalized() {
        let input = "Value: `r\tx`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r x`.");
    }

    #[test]
    fn test_classic_spelling_multiline_body_preserved() {
        let input = "Value: `r sum(\n  c(1, 2)\n)`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, "Value: `r sum(\n  c(1, 2)\n)`.");
    }

    #[test]
    fn test_both_spellings_in_one_document() {
        // The whole contract in one line: same document, same value, two
        // deliberately different escaping defaults.
        let input = "Escaped `{r} x` and markdown `r x`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "Escaped `r .QuartoInlineRender(x)` and markdown `r x`."
        );
    }

    #[test]
    fn test_rewrite_is_idempotent() {
        // A brace expression rewrites to the classic spelling, so a second
        // pass must not wrap it again — that would re-escape a value the
        // wrapper already escaped.
        let input = "Escaped `{r} x` and markdown `r x`.";
        let once = resolve_inline_r_expressions(input);
        let twice = resolve_inline_r_expressions(&once);
        assert_eq!(once, twice);
    }

    // === neither spelling ===

    #[test]
    fn test_no_inline_r() {
        let input = "No R code here, just `code` and `more code`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_inline_code_without_r() {
        let input = "Use `print()` to output.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_uppercase_r_not_matched() {
        let input = "This `R code` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_r_without_space_not_matched() {
        let input = "This `rx` is not inline R.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    // === fenced-code-block guard tests (bd-knitr-inline-r-eats-fence-2ofk91x1) ===
    //
    // A fence's last opening backtick must never anchor an inline-R match.
    // Both spellings have a fence shape that would otherwise do exactly that:
    // the display fence `` ```r `` for the classic branch, and the executable
    // cell `` ```{r} `` — which every knitr document contains — for the brace
    // branch.

    #[test]
    fn test_display_fence_not_matched() {
        let input = "```r\npak::pak(c(\"usethis\", \"cli\"))\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input, "a display fence must be left untouched");
    }

    #[test]
    fn test_display_fence_with_following_text_not_matched() {
        let input = "Before.\n\n```r\n1 + 1\n```\n\nAfter.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_four_backtick_display_fence_not_matched() {
        let input = "````r\nx <- `y`\n````";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_executable_cell_fence_not_matched() {
        // The brace branch's own fence shape, and the one that appears in
        // every document the knitr engine runs.
        let input = "Before.\n\n```{r}\n1 + 1\n```\n\nAfter.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_executable_cell_fence_at_start_of_input_not_matched() {
        // `^` is start-of-input, so the first backtick of a document-initial
        // fence is reachable by the alternation's first branch.
        let input = "```{r}\n1 + 1\n```\n\nAfter.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_executable_cell_fence_with_trailing_space_not_matched() {
        // The acute case for the brace branch: a trailing space after `{r}`
        // satisfies the `[ \t]` separator, so the prefix guard is the only
        // thing standing between this fence and a match that swallows the
        // cell body up to the closing fence.
        let input = "Before.\n\n```{r} \n1 + 1\n```\n\nAfter.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_four_backtick_executable_cell_fence_not_matched() {
        let input = "````{r} \nx <- `y`\n````";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_fence_inside_yaml_scalar_not_matched() {
        let input = "---\ntitle: \"In the title: ```r blocks\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_brace_fence_inside_yaml_scalar_not_matched() {
        let input = "---\ntitle: \"In the title: ```{r} blocks\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_escaped_backtick_fence_not_matched() {
        let input = "---\ntitle: \"[In the title: \\`\\`\\`r blocks]{.yaml-markdown-syntax-error}\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_escaped_backtick_brace_fence_not_matched() {
        let input = "---\ntitle: \"[In the title: \\`\\`\\`{r} blocks]{.yaml-markdown-syntax-error}\"\n---\n\n```{r}\n1 + 1\n```";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_backtick_prefixed_not_matched() {
        let input = "Text ``r x` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_backtick_prefixed_brace_not_matched() {
        let input = "Text ``{r} x` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_newline_after_r_not_matched() {
        let input = "Text `r\nx` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_newline_after_brace_not_matched() {
        let input = "Text `{r}\nx` more.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_inline_r_still_matched_next_to_a_fence() {
        let input = "```r\n1 + 1\n```\n\nThe answer is `{r} 1+1`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "```r\n1 + 1\n```\n\nThe answer is `r .QuartoInlineRender(1+1)`."
        );
    }

    #[test]
    fn test_inline_r_still_matched_next_to_an_executable_cell() {
        let input = "```{r}\nx <- 1\n```\n\nThe answer is `{r} x`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "```{r}\nx <- 1\n```\n\nThe answer is `r .QuartoInlineRender(x)`."
        );
    }

    // === has_inline_r_expressions tests ===

    #[test]
    fn test_has_inline_r_true() {
        assert!(has_inline_r_expressions("Value: `{r} x`."));
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
        assert!(!has_inline_r_expressions("```r\n1 + 1\n```"));
        assert!(!has_inline_r_expressions("````r\nx <- `y`\n````"));
        assert!(!has_inline_r_expressions("```{r}\n1 + 1\n```"));
        assert!(!has_inline_r_expressions("```{r} \n1 + 1\n```"));
    }
}
