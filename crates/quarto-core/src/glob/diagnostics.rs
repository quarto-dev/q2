/*
 * glob/diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! One voice for "your pattern did not work".
//!
//! Four subsystems accept globs, and each owns its own error-code
//! family (`Q-5-*` project/resources, `Q-12-*` listings, `Q-13-*`
//! navigation). What they must *not* own is four different ways of
//! explaining the same three failures — a reader who learns what
//! "matched nothing" means for `project.render` should not have to
//! re-learn it for `contents:`.
//!
//! Each builder takes the caller's code and a label naming the key
//! the pattern was written under, and returns a
//! [`DiagnosticMessageBuilder`] the caller can add subsystem-specific
//! hints to before building.
//!
//! The three failures, and why they are separate codes rather than
//! one: they have different fixes. A pattern that escapes the project
//! is a path problem, an uncompilable pattern is a syntax problem, and
//! a pattern that matches nothing is usually a *semantics* problem —
//! most often the Quarto-1 assumption that `*` recurses.

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::SourceInfo;

/// The Quarto-1 migration hint, attached wherever a pattern compiled
/// but matched nothing.
///
/// Q1 silently rewrote an unanchored pattern to `**/<pattern>`, so
/// `*.qmd` meant "anywhere in the tree". q2 does not (decision D5),
/// which makes "matched nothing" the single most likely symptom of a
/// project moving over. Saying so here means every consumer says it.
pub const STAR_IS_ONE_LEVEL_HINT: &str = "In Quarto 2, `*` matches within one directory level — write `**/` to search \
     subdirectories (`posts/**/*.qmd`).";

/// A pattern compiled but matched nothing.
pub fn matched_nothing(
    code: &str,
    key: &str,
    pattern: &str,
    source: &SourceInfo,
) -> DiagnosticMessageBuilder {
    DiagnosticMessageBuilder::warning(format!("{key} pattern `{pattern}` matched nothing"))
        .with_code(code)
        .with_location(source.clone())
        .problem("Nothing in the project matches this pattern.")
        .add_info(STAR_IS_ONE_LEVEL_HINT)
}

/// The glob engine rejected the pattern.
pub fn invalid_pattern(
    code: &str,
    key: &str,
    pattern: &str,
    reason: &str,
    source: &SourceInfo,
) -> DiagnosticMessageBuilder {
    DiagnosticMessageBuilder::warning(format!("{key} pattern `{pattern}` is not a valid glob"))
        .with_code(code)
        .with_location(source.clone())
        .problem(reason.to_string())
        .add_info(
            "`**` must be a whole path segment (`docs/**/*.qmd`), and `[...]` character \
             classes must be closed.",
        )
}

/// The pattern's `..` segments climb above the project root.
pub fn escapes_project(
    code: &str,
    key: &str,
    pattern: &str,
    source: &SourceInfo,
) -> DiagnosticMessageBuilder {
    DiagnosticMessageBuilder::warning(format!(
        "{key} pattern `{pattern}` points outside the project directory"
    ))
    .with_code(code)
    .with_location(source.clone())
    .problem("The pattern's `..` segments climb above the project root, so it matches nothing.")
    .add_info(
        "A leading `/` is project-root-relative, not a filesystem path: `/posts/*.qmd` \
         means `<project>/posts/*.qmd`.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::By;

    fn source() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    #[test]
    fn each_builder_carries_its_code_and_quotes_the_pattern() {
        let d = matched_nothing("Q-5-13", "`project.render`", "postz/*.qmd", &source()).build();
        assert_eq!(d.code.as_deref(), Some("Q-5-13"));
        assert!(format!("{d:?}").contains("postz/*.qmd"));

        let d = invalid_pattern("Q-12-18", "listing `contents:`", "a**b", "bad", &source()).build();
        assert_eq!(d.code.as_deref(), Some("Q-12-18"));

        let d = escapes_project("Q-5-14", "`resources:`", "../x", &source()).build();
        assert_eq!(d.code.as_deref(), Some("Q-5-14"));
    }

    /// The migration hint is the reason these builders are shared:
    /// every consumer must give the same explanation of `*`.
    #[test]
    fn matched_nothing_always_explains_the_q1_divergence() {
        let d = matched_nothing("Q-5-13", "`project.render`", "x", &source()).build();
        assert!(
            format!("{d:?}").contains("one directory level"),
            "the Q1 migration hint must be present: {d:?}"
        );
    }
}
