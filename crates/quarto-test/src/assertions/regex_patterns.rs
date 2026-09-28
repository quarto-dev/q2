use anyhow::{Context, Result, bail};
use regex::Regex;

/// Normalize whitespace quirks introduced by `pdf_extract` so literal-space
/// fixture assertions (ported from Q1, whose targets didn't have these
/// quirks) match reliably.
///
/// Two independent issues, both verified directly by dumping
/// `pdf_extract::extract_text` output on rendered margin-layout fixtures:
///
/// - Typst's default figure/table caption rendering joins the supplement
///   and number with a non-breaking space (e.g. `"Figure\u{a0}1"`) to keep
///   them from splitting across a line break; `pdf_extract` preserves that
///   codepoint verbatim, so a literal-space pattern like `'Figure 1'` never
///   matches unless it's replaced with a regular space first.
/// - `pdf_extract` inserts an extra space at the boundary between two text
///   runs — e.g. plain text followed by a `#ref()`-generated link — so
///   `"REF-ALPHA pointing to Figure 1"` in the source renders as `"REF-ALPHA
///   pointing to  Figure 1 ."` (note the double space before `Figure` and
///   before the period). Collapsing runs of regular spaces down to one
///   fixes this without touching newlines, which patterns' `(?m)` mode
///   relies on for `^`/`$`.
pub(super) fn normalize_pdf_text(text: &str) -> String {
    let text = text.replace('\u{a0}', " ");
    let mut normalized = String::with_capacity(text.len());
    let mut prev_was_space = false;
    for c in text.chars() {
        if c == ' ' {
            if !prev_was_space {
                normalized.push(c);
            }
            prev_was_space = true;
        } else {
            normalized.push(c);
            prev_was_space = false;
        }
    }
    normalized
}

/// Compile multiline regex patterns for an output assertion.
pub(super) fn compile_patterns(
    matches: &[String],
    no_matches: &[String],
) -> Result<(Vec<Regex>, Vec<Regex>)> {
    let compile = |patterns: &[String]| {
        patterns
            .iter()
            .map(|pattern| {
                Regex::new(&format!("(?m){pattern}"))
                    .with_context(|| format!("invalid regex pattern: {pattern}"))
            })
            .collect::<Result<Vec<_>>>()
    };

    Ok((compile(matches)?, compile(no_matches)?))
}

/// Verify required and forbidden patterns against text content.
pub(super) fn verify_patterns(
    content: &str,
    matches: &[Regex],
    no_matches: &[Regex],
    match_patterns: &[String],
    no_match_patterns: &[String],
    description: &str,
) -> Result<()> {
    let mut failures = Vec::new();

    for (regex, pattern) in matches.iter().zip(match_patterns) {
        if !regex.is_match(content) {
            failures.push(format!("Required pattern not found: {pattern}"));
        }
    }

    for (regex, pattern) in no_matches.iter().zip(no_match_patterns) {
        if regex.is_match(content) {
            failures.push(format!("Illegal pattern found: {pattern}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "{} regex mismatch(es) in {}:\n  - {}",
            failures.len(),
            description,
            failures.join("\n  - ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_pdf_text_replaces_non_breaking_space_with_regular_space() {
        assert_eq!(
            normalize_pdf_text("Figure\u{a0}1: caption text"),
            "Figure 1: caption text"
        );
    }

    #[test]
    fn normalize_pdf_text_collapses_runs_of_regular_spaces() {
        assert_eq!(
            normalize_pdf_text("REF-ALPHA pointing to  Figure 1 ."),
            "REF-ALPHA pointing to Figure 1 ."
        );
    }

    #[test]
    fn normalize_pdf_text_preserves_newlines() {
        assert_eq!(
            normalize_pdf_text("line one\nline two"),
            "line one\nline two"
        );
    }

    #[test]
    fn normalize_pdf_text_is_a_noop_on_already_normal_text() {
        assert_eq!(normalize_pdf_text("plain text"), "plain text");
    }
}
