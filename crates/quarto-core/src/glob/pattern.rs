/*
 * glob/pattern.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Raw pattern string → normalized, project-relative
//! [`GlobPattern`].
//!
//! Normalization is **lexical**: no filesystem is consulted, so this
//! runs identically on native and WASM, and against candidates that
//! do not exist on disk (a listing item, a not-yet-created resource).

use serde::{Deserialize, Serialize};

/// One resolved glob: a normalized project-relative pattern plus its
/// negation flag.
///
/// This is the serialized shape stored on
/// [`DocumentProfile::listing_content_globs`](crate::document_profile::DocumentProfile)
/// — resolution happens once, at profile-extraction / parse time, and
/// consumers only match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobPattern {
    /// Normalized project-relative pattern (forward slashes, no
    /// `.`/`..` segments), e.g. `"sub/*.qmd"`.
    pub pattern: String,
    /// True for `!`-prefixed patterns: matches are excluded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub negated: bool,
}

impl GlobPattern {
    /// A positive (non-negated) pattern.
    pub fn positive(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            negated: false,
        }
    }

    /// A negated pattern — matches are excluded.
    pub fn negated(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            negated: true,
        }
    }
}

/// The characters that make a pattern a *pattern* rather than a
/// literal path.
///
/// `[` counts: a literal-looking `data/[0-9].csv` is a character
/// class, and treating it as a literal path would silently change
/// what it matches.
const METACHARACTERS: [char; 3] = ['*', '?', '['];

/// True if `pattern` contains any glob metacharacter.
pub fn has_metacharacters(pattern: &str) -> bool {
    pattern.contains(METACHARACTERS)
}

/// Split a leading `!` off a raw pattern.
///
/// Returns `(negated, rest)`. Only a `!` in first position negates;
/// one anywhere else is an ordinary filename character.
pub fn split_negation(raw: &str) -> (bool, &str) {
    match raw.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, raw),
    }
}

/// Join `pattern` onto `base_dir` and lexically normalize: `.`
/// segments drop, `..` segments pop.
///
/// Returns `None` when a `..` would climb above the project root —
/// the caller reports that as a diagnostic and matches nothing.
///
/// A pattern beginning with `/` is **project-root-relative**
/// (decision D2, Quarto YAML convention): `/posts/*.qmd` means
/// `<project>/posts/*.qmd` regardless of which file declared it, and
/// never the filesystem path `/posts/`. This is the escape hatch for
/// "I really do mean the project root, not my own directory".
///
/// Both inputs use forward slashes; backslashes are normalized first
/// so a Windows-authored pattern behaves the same everywhere.
pub fn join_and_normalize(base_dir: &str, pattern: &str) -> Option<String> {
    let pattern = pattern.replace('\\', "/");

    // Leading `/` re-anchors at the project root, discarding the
    // declaring file's base directory.
    let (base_dir, pattern) = match pattern.strip_prefix('/') {
        Some(rest) => ("", rest.to_string()),
        None => (base_dir, pattern),
    };

    let joined = if base_dir.is_empty() {
        pattern
    } else {
        format!("{base_dir}/{pattern}")
    };

    let mut segments: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_empty_base_is_identity() {
        assert_eq!(join_and_normalize("", "*.qmd"), Some("*.qmd".into()));
        assert_eq!(
            join_and_normalize("", "posts/**/*.qmd"),
            Some("posts/**/*.qmd".into())
        );
    }

    #[test]
    fn join_prefixes_base_dir() {
        assert_eq!(join_and_normalize("sub", "*.qmd"), Some("sub/*.qmd".into()));
        assert_eq!(
            join_and_normalize("blog", "deep/*.qmd"),
            Some("blog/deep/*.qmd".into())
        );
    }

    #[test]
    fn join_normalizes_dot_and_dotdot() {
        assert_eq!(
            join_and_normalize("sub", "../rootpost.qmd"),
            Some("rootpost.qmd".into())
        );
        assert_eq!(
            join_and_normalize("sub", "./*.qmd"),
            Some("sub/*.qmd".into())
        );
        assert_eq!(
            join_and_normalize("a/b", "../c/*.qmd"),
            Some("a/c/*.qmd".into())
        );
        assert_eq!(
            join_and_normalize("", "posts/../notes/*.qmd"),
            Some("notes/*.qmd".into())
        );
    }

    #[test]
    fn join_escaping_project_root_is_none() {
        assert_eq!(join_and_normalize("", "../*.qmd"), None);
        assert_eq!(join_and_normalize("sub", "../../*.qmd"), None);
        assert_eq!(join_and_normalize("a/b", "../../../x.qmd"), None);
    }

    // ── leading `/` = project root (D2) ─────────────────────────

    #[test]
    fn leading_slash_anchors_at_project_root() {
        // The pre-migration behavior silently folded the leading `/`
        // into the base dir, so `/posts/*.qmd` in `sub/index.qmd`
        // became `sub/posts/*.qmd` and matched nothing.
        assert_eq!(
            join_and_normalize("sub", "/posts/*.qmd"),
            Some("posts/*.qmd".into())
        );
        assert_eq!(
            join_and_normalize("blog/deep", "/index.qmd"),
            Some("index.qmd".into())
        );
        // Already at the root: unchanged.
        assert_eq!(join_and_normalize("", "/*.qmd"), Some("*.qmd".into()));
        // Redundant slashes collapse.
        assert_eq!(
            join_and_normalize("sub", "//posts//a.qmd"),
            Some("posts/a.qmd".into())
        );
    }

    #[test]
    fn leading_slash_still_clamps_to_project_root() {
        assert_eq!(join_and_normalize("sub", "/../outside.qmd"), None);
    }

    #[test]
    fn backslashes_normalize_before_anchoring() {
        assert_eq!(
            join_and_normalize("sub", "\\posts\\a.qmd"),
            Some("posts/a.qmd".into())
        );
    }

    // ── negation split ──────────────────────────────────────────

    #[test]
    fn negation_only_leads() {
        assert_eq!(split_negation("!draft.qmd"), (true, "draft.qmd"));
        assert_eq!(split_negation("draft.qmd"), (false, "draft.qmd"));
        // A `!` elsewhere is an ordinary filename character.
        assert_eq!(split_negation("hey!.qmd"), (false, "hey!.qmd"));
    }

    // ── metacharacter detection ─────────────────────────────────

    #[test]
    fn metacharacters_include_brackets() {
        assert!(has_metacharacters("data/*.csv"));
        assert!(has_metacharacters("img/?.png"));
        assert!(has_metacharacters("data/[0-9].csv"));
        assert!(!has_metacharacters("data/plain.csv"));
        assert!(!has_metacharacters("posts"));
    }
}
