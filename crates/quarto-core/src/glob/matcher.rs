/*
 * glob/matcher.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Compiled matching for q2 globs.
//!
//! # Why the `glob` crate, and which half of it
//!
//! The `glob` crate ships two independent halves: `glob::glob()`,
//! a filesystem walker, and [`glob::Pattern`], a **pure** matcher
//! whose `matches_with` never touches the filesystem. q2 uses only
//! the second half. Enumeration is the caller's job — the in-memory
//! project index for listings, a
//! [`SystemRuntime`](quarto_system_runtime::SystemRuntime) walk for
//! resources — which is what makes the same semantics available
//! under WASM against the hub-client VFS.
//!
//! Using a real glob implementation rather than a hand-rolled one
//! also means every consumer gets the whole vocabulary: `*`, `?`,
//! `**`, `[abc]`, `[a-z]`, `[!abc]`, and `[*]` to escape a literal
//! asterisk. Before bd-mt7a6uc4 character classes worked in
//! `resources:` (which used the walker) and silently matched nothing
//! everywhere else.
//!
//! # The options are load-bearing
//!
//! [`MATCH_OPTIONS`] is what pins q2's semantics; the crate's
//! defaults are *not* what we want. In particular
//! `require_literal_separator: true` is what makes `*` mean "one path
//! segment" (decision D5) — with the default, `*.qmd` would match
//! `sub/about.qmd` and every pattern in the tree would silently
//! widen. The walker hides this difference (it descends
//! directory-by-directory), so the option only becomes visible once
//! you use `Pattern` directly. Do not change these without changing
//! `claude-notes/designs/glob-semantics.md`.

// The external `glob` crate, spelled `::glob` so it is never confused
// with `crate::glob` (this module).
use ::glob::{MatchOptions, Pattern, PatternError};

use super::pattern::{GlobPattern, has_metacharacters};
use super::{GlobOptions, path_to_forward_slashes};

/// q2's glob semantics, as `glob::MatchOptions`.
///
/// - `case_sensitive: true` — patterns mean what they say, on every
///   platform. (Windows filesystems are case-insensitive, but the
///   pattern-to-path comparison is ours to define, and a project that
///   renders differently on macOS than on Linux is worse than one
///   that requires the author to match case.)
/// - `require_literal_separator: true` — `*` and `?` do not cross
///   `/`; `**` is how you cross directories (D5).
/// - `require_literal_leading_dot: false` — `data/*` matches
///   `data/.nojekyll`. This is the pre-migration behavior of both the
///   `glob()` walker and the hand-rolled matcher; dotfiles are
///   excluded (where they are excluded at all) by the *discovery*
///   layer, not by pattern semantics.
const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

/// A glob pattern that `glob::Pattern` refused to compile.
///
/// The two ways to get here are a `**` that is not a whole path
/// component (`a**b`) and a malformed character class (`data/[.csv`).
/// Both are user errors that used to match nothing silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobCompileError {
    /// The normalized pattern that failed to compile.
    pub pattern: String,
    /// The underlying `glob` crate message.
    pub message: String,
}

impl std::fmt::Display for GlobCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid glob pattern `{}`: {}",
            self.pattern, self.message
        )
    }
}

impl GlobCompileError {
    fn new(pattern: &str, err: PatternError) -> Self {
        Self {
            pattern: pattern.to_string(),
            message: err.msg.to_string(),
        }
    }
}

/// A compiled set of positive and negative patterns.
///
/// Compile once, match many: building this is the only step that can
/// fail, and callers that match in a loop over candidates should hoist
/// it out of the loop.
#[derive(Debug, Clone)]
pub struct PatternSet {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    /// The pattern, plus `<pattern>/**` when the directory rule
    /// applies. An entry matches if *any* of these do.
    alternatives: Vec<Pattern>,
    negated: bool,
}

impl Entry {
    fn is_match(&self, candidate: &str) -> bool {
        self.alternatives
            .iter()
            .any(|p| p.matches_with(candidate, MATCH_OPTIONS))
    }
}

impl PatternSet {
    /// Compile a resolved pattern list.
    ///
    /// Fails on the first pattern `glob::Pattern` rejects; the caller
    /// reports it against the pattern's source span (resolution
    /// already validated the patterns it emitted, so a failure here
    /// means the list came from somewhere else — a stale profile
    /// cache, say).
    pub fn compile(globs: &[GlobPattern], options: &GlobOptions) -> Result<Self, GlobCompileError> {
        let mut entries = Vec::with_capacity(globs.len());
        for glob in globs {
            entries.push(Entry {
                alternatives: compile_alternatives(&glob.pattern, options)?,
                negated: glob.negated,
            });
        }
        Ok(Self { entries })
    }

    /// True iff `candidate` matches at least one positive pattern and
    /// no negative one.
    ///
    /// `candidate` must be a project-relative, forward-slash path —
    /// use [`path_to_forward_slashes`] to build one. Negation is
    /// order-independent: an excluded path stays excluded no matter
    /// where the `!` entry appears.
    pub fn matches(&self, candidate: &str) -> bool {
        self.entries
            .iter()
            .any(|e| !e.negated && e.is_match(candidate))
            && !self
                .entries
                .iter()
                .any(|e| e.negated && e.is_match(candidate))
    }

    /// [`Self::matches`] for a `Path`, normalizing it first.
    pub fn matches_path(&self, candidate: &std::path::Path) -> bool {
        self.matches(&path_to_forward_slashes(candidate))
    }

    /// True when there is nothing to match against — no positive
    /// pattern survived resolution.
    pub fn is_empty(&self) -> bool {
        !self.entries.iter().any(|e| !e.negated)
    }
}

/// Compile one pattern into the alternatives an entry matches on.
///
/// The directory rule (D4) is expressed by compiling a second
/// pattern rather than by string-prefix comparison: a literal
/// `posts` also matches as `posts/**`. Trailing slashes are trimmed
/// first, so `posts/` and `posts` behave identically.
fn compile_alternatives(
    pattern: &str,
    options: &GlobOptions,
) -> Result<Vec<Pattern>, GlobCompileError> {
    let literal = pattern.trim_end_matches('/');
    let mut alternatives =
        vec![Pattern::new(literal).map_err(|e| GlobCompileError::new(literal, e))?];

    if options.directory_rule && !literal.is_empty() && !has_metacharacters(literal) {
        let beneath = format!("{literal}/**");
        alternatives.push(Pattern::new(&beneath).map_err(|e| GlobCompileError::new(&beneath, e))?);
    }

    Ok(alternatives)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(patterns: &[GlobPattern]) -> PatternSet {
        PatternSet::compile(patterns, &GlobOptions::default()).expect("compiles")
    }

    fn matches(pattern: &str, candidate: &str) -> bool {
        set(&[GlobPattern::positive(pattern)]).matches(candidate)
    }

    // ── the semantics table (claude-notes/designs/glob-semantics.md) ──

    #[test]
    fn literal_patterns_match_exactly() {
        assert!(matches("index.qmd", "index.qmd"));
        assert!(!matches("index.qmd", "sub/index.qmd"));
        assert!(matches("docs/api.qmd", "docs/api.qmd"));
    }

    #[test]
    fn star_is_one_segment() {
        assert!(matches("*.qmd", "about.qmd"));
        assert!(!matches("*.qmd", "sub/about.qmd"));
        assert!(matches("sub/*.qmd", "sub/about.qmd"));
        assert!(!matches("sub/*.qmd", "sub/deep/about.qmd"));
    }

    #[test]
    fn double_star_crosses_segments() {
        assert!(matches("**/*.qmd", "sub/about.qmd"));
        // `**` matches zero segments too.
        assert!(matches("**/*.qmd", "about.qmd"));
        assert!(matches("docs/**/*.qmd", "docs/a/b.qmd"));
        assert!(!matches("docs/**/*.qmd", "other/a.qmd"));
    }

    #[test]
    fn question_mark_is_one_char_and_does_not_cross() {
        assert!(matches("docs/?pi.qmd", "docs/api.qmd"));
        assert!(!matches("a?c.qmd", "a/c.qmd"));
    }

    /// The capability that motivated D1: classes worked in
    /// `resources:` (which used the `glob()` walker) and silently
    /// matched nothing in every other consumer.
    #[test]
    fn character_classes_work() {
        assert!(matches("data/fig-[0-9].csv", "data/fig-3.csv"));
        assert!(!matches("data/fig-[0-9].csv", "data/fig-x.csv"));
        assert!(matches("posts/p[a-z].qmd", "posts/pq.qmd"));
        assert!(!matches("data/fig-[!0-9].csv", "data/fig-3.csv"));
        assert!(matches("data/fig-[!0-9].csv", "data/fig-x.csv"));
    }

    #[test]
    fn bracket_escape_matches_literal_asterisk() {
        assert!(matches("a[*]b.qmd", "a*b.qmd"));
        assert!(!matches("a[*]b.qmd", "axb.qmd"));
    }

    /// Hazard 2: dotfiles are matched by `*`. Excluding them is the
    /// discovery layer's job, not the matcher's.
    #[test]
    fn dotfiles_match_star() {
        assert!(matches("data/*", "data/.nojekyll"));
        assert!(matches("*", ".hidden"));
    }

    // ── directory rule (D4) ─────────────────────────────────────

    #[test]
    fn bare_directory_matches_beneath() {
        assert!(matches("posts", "posts/welcome/index.qmd"));
        assert!(matches("posts/", "posts/a.qmd"));
        // Segment-exact, never a bare string prefix.
        assert!(!matches("posts", "posts-archive/old.qmd"));
        // A literal file entry still matches itself.
        assert!(matches("posts/a.qmd", "posts/a.qmd"));
    }

    #[test]
    fn directory_rule_does_not_apply_to_metacharacter_patterns() {
        assert!(!matches("posts/*.qmd", "posts/deep/a.qmd"));
        // …including character-class patterns: `[` makes it a pattern.
        assert!(!matches("posts/[ab]", "posts/a/deep.qmd"));
    }

    #[test]
    fn directory_rule_can_be_disabled() {
        let opts = GlobOptions {
            directory_rule: false,
            ..GlobOptions::default()
        };
        let s = PatternSet::compile(&[GlobPattern::positive("posts")], &opts).unwrap();
        assert!(!s.matches("posts/a.qmd"));
        assert!(s.matches("posts"));
    }

    // ── negation (D3) ───────────────────────────────────────────

    #[test]
    fn negation_excludes() {
        let s = set(&[
            GlobPattern::positive("sub/*.qmd"),
            GlobPattern::negated("sub/p2.qmd"),
        ]);
        assert!(s.matches("sub/p1.qmd"));
        assert!(!s.matches("sub/p2.qmd"));
    }

    #[test]
    fn negation_is_order_independent() {
        let s = set(&[
            GlobPattern::negated("sub/p2.qmd"),
            GlobPattern::positive("sub/*.qmd"),
        ]);
        assert!(s.matches("sub/p1.qmd"));
        assert!(!s.matches("sub/p2.qmd"));
    }

    #[test]
    fn negated_directory_excludes_beneath() {
        let s = set(&[
            GlobPattern::positive("posts"),
            GlobPattern::negated("posts/drafts"),
        ]);
        assert!(s.matches("posts/welcome/index.qmd"));
        assert!(!s.matches("posts/drafts/wip.qmd"));
    }

    #[test]
    fn no_positive_matches_nothing() {
        assert!(!set(&[]).matches("sub/p1.qmd"));
        let only_negative = set(&[GlobPattern::negated("sub/p2.qmd")]);
        assert!(!only_negative.matches("sub/p1.qmd"));
        assert!(only_negative.is_empty());
    }

    // ── compile errors ──────────────────────────────────────────

    #[test]
    fn recursive_wildcard_must_be_a_whole_component() {
        let err = PatternSet::compile(
            &[GlobPattern::positive("a**b.qmd")],
            &GlobOptions::default(),
        )
        .unwrap_err();
        assert_eq!(err.pattern, "a**b.qmd");
        assert!(err.message.contains("recursive wildcards"), "{err}");
    }

    #[test]
    fn malformed_character_class_is_an_error() {
        let err = PatternSet::compile(
            &[GlobPattern::positive("data/[.csv")],
            &GlobOptions::default(),
        )
        .unwrap_err();
        assert!(err.message.contains("range"), "{err}");
    }

    // ── candidate normalization ─────────────────────────────────

    #[test]
    fn matches_path_normalizes_separators() {
        let s = set(&[GlobPattern::positive("sub/*.qmd")]);
        assert!(s.matches_path(std::path::Path::new("sub/a.qmd")));
    }
}
