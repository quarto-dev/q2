/*
 * glob/resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Raw user-written patterns → resolved, validated
//! [`GlobPattern`]s, plus everything a caller needs to diagnose what
//! it could not use.
//!
//! Resolution is where the three failure modes are separated, each
//! carrying the [`SourceInfo`] of the YAML scalar it came from so the
//! caller can point at the right span:
//!
//! - [`GlobResolution::escaped`] — the pattern normalizes outside the
//!   project root (`../../*.qmd`). Matches nothing.
//! - [`GlobResolution::invalid`] — `glob::Pattern` rejects it
//!   (`a**b`, `data/[.csv`). Matches nothing.
//! - everything else lands in [`GlobResolution::globs`] and is
//!   guaranteed to compile.
//!
//! Nothing here touches the filesystem, so it runs identically on
//! native and WASM.

use quarto_source_map::SourceInfo;

use super::matcher::PatternSet;
use super::pattern::{GlobPattern, join_and_normalize, split_negation};
use super::provenance::BaseDirContext;
use super::{GlobCompileError, GlobOptions};

/// A raw pattern as the user wrote it, with its YAML provenance.
#[derive(Debug, Clone)]
pub struct RawGlob {
    /// The pattern verbatim, including any leading `!`.
    pub raw: String,
    /// Provenance of the YAML scalar, used both to pick the base
    /// directory and to place any diagnostic.
    pub source: SourceInfo,
}

impl RawGlob {
    /// Construct from a pattern string and its source info.
    pub fn new(raw: impl Into<String>, source: SourceInfo) -> Self {
        Self {
            raw: raw.into(),
            source,
        }
    }
}

/// A pattern whose normalized form escapes the project root. It
/// matches nothing; the caller reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct EscapedGlob {
    /// The pattern as the user wrote it (including any `!`).
    pub raw: String,
    /// Provenance of the offending YAML scalar.
    pub source: SourceInfo,
}

/// A pattern `glob::Pattern` refused to compile. It matches nothing;
/// the caller reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidGlob {
    /// The pattern as the user wrote it (including any `!`).
    pub raw: String,
    /// Why it was rejected, from the `glob` crate.
    pub message: String,
    /// Provenance of the offending YAML scalar.
    pub source: SourceInfo,
}

/// Result of resolving a list of raw patterns.
#[derive(Debug, Clone, Default)]
pub struct GlobResolution {
    /// Usable patterns (positive and negative), project-relative and
    /// guaranteed to compile.
    pub globs: Vec<GlobPattern>,
    /// Provenance of each entry in [`Self::globs`], same order and
    /// length — use [`Self::iter`] rather than indexing these in
    /// parallel by hand.
    ///
    /// Resolution is where a pattern stops being a string the user
    /// wrote and becomes a normalized one, so it is also the last
    /// place that can hand a downstream diagnostic the span to point
    /// at. Keeping the two lists aligned here beats having every
    /// consumer re-pair resolved patterns with raw entries — a
    /// pairing that silently goes wrong as soon as one entry is
    /// dropped or injected.
    pub sources: Vec<SourceInfo>,
    /// Patterns that escaped the project root.
    pub escaped: Vec<EscapedGlob>,
    /// Patterns that failed to compile.
    pub invalid: Vec<InvalidGlob>,
}

impl GlobResolution {
    /// Each usable pattern with the provenance it came from.
    pub fn iter(&self) -> impl Iterator<Item = (&GlobPattern, &SourceInfo)> {
        self.globs.iter().zip(self.sources.iter())
    }

    /// Each **positive** pattern with its provenance.
    pub fn positives(&self) -> impl Iterator<Item = (&GlobPattern, &SourceInfo)> {
        self.iter().filter(|(g, _)| !g.negated)
    }

    /// Compile the usable patterns.
    ///
    /// Infallible in practice: [`resolve_patterns`] only emits
    /// patterns it has already compiled once.
    pub fn compile(&self, options: &GlobOptions) -> Result<PatternSet, GlobCompileError> {
        PatternSet::compile(&self.globs, options)
    }

    /// True when nothing was usable *and* nothing was reported —
    /// i.e. the caller wrote no patterns at all, as opposed to
    /// writing patterns that all failed.
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty() && self.escaped.is_empty() && self.invalid.is_empty()
    }
}

/// Resolve raw patterns against their declaring files' directories.
///
/// Each pattern is: split on a leading `!`, joined to the base
/// directory its provenance names (unless it starts with `/`, which
/// re-anchors at the project root), lexically normalized, and
/// validated by compiling it. Failures are routed to
/// [`GlobResolution::escaped`] / [`GlobResolution::invalid`] rather
/// than dropped, so every caller can diagnose them.
///
/// When [`GlobOptions::default_positive`] is set and the caller wrote
/// *only* negations, the default positive pattern is prepended
/// (resolved against [`BaseDirContext::fallback_dir`]) so
/// `["!draft.qmd"]` means "everything except `draft.qmd`".
pub fn resolve_patterns(
    raws: impl IntoIterator<Item = RawGlob>,
    ctx: &BaseDirContext<'_>,
    options: &GlobOptions,
) -> GlobResolution {
    let mut out = GlobResolution::default();

    for entry in raws {
        let (negated, pattern) = split_negation(&entry.raw);
        let base_dir = ctx.base_dir_for(&entry.source);

        let Some(resolved) = join_and_normalize(&base_dir, pattern) else {
            out.escaped.push(EscapedGlob {
                raw: entry.raw.clone(),
                source: entry.source.clone(),
            });
            continue;
        };

        let candidate = GlobPattern {
            pattern: resolved,
            negated,
        };

        // Validate by compiling: a pattern that reaches `globs` is
        // guaranteed to compile for every downstream consumer.
        if let Err(err) = PatternSet::compile(std::slice::from_ref(&candidate), options) {
            out.invalid.push(InvalidGlob {
                raw: entry.raw.clone(),
                message: err.message,
                source: entry.source.clone(),
            });
            continue;
        }

        out.globs.push(candidate);
        out.sources.push(entry.source.clone());
    }

    inject_default_positive(&mut out, ctx, options);
    out
}

/// When only negations survived, prepend the consumer's default
/// positive pattern so the negations have something to subtract from.
fn inject_default_positive(
    out: &mut GlobResolution,
    ctx: &BaseDirContext<'_>,
    options: &GlobOptions,
) {
    let Some(default_positive) = options.default_positive else {
        return;
    };
    if out.globs.is_empty() || !out.globs.iter().all(|g| g.negated) {
        return;
    }
    // Cannot fail: `fallback_dir` is an already-normalized
    // project-relative directory and the default is a literal.
    if let Some(pattern) = join_and_normalize(ctx.fallback_dir, default_positive) {
        out.globs.insert(0, GlobPattern::positive(pattern));
        // The default has no YAML behind it — it is q2 supplying the
        // set the negations subtract from.
        out.sources.insert(
            0,
            SourceInfo::generated(quarto_source_map::By::programmatic_config()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::By;
    use std::path::Path;

    fn programmatic() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    fn raw(pattern: &str) -> RawGlob {
        RawGlob::new(pattern, programmatic())
    }

    fn ctx<'a>(project: &'a Path, fallback: &'a str) -> BaseDirContext<'a> {
        BaseDirContext {
            source_context: None,
            project_dir: project,
            fallback_dir: fallback,
        }
    }

    #[test]
    fn sources_stay_aligned_with_globs() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [
                raw("a.qmd"),
                raw("!b.qmd"),
                raw("../escapes.qmd"),
                raw("c.qmd"),
            ],
            &ctx(project, ""),
            &GlobOptions::LISTING,
        );
        assert_eq!(r.globs.len(), r.sources.len());
        // The escaping entry is not in `globs`, so a hand-rolled zip
        // against the raw list would mis-pair everything after it.
        let patterns: Vec<&str> = r.iter().map(|(g, _)| g.pattern.as_str()).collect();
        assert_eq!(patterns, vec!["a.qmd", "b.qmd", "c.qmd"]);
        assert_eq!(r.positives().count(), 2);
    }

    #[test]
    fn resolves_against_the_fallback_dir_without_provenance() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("*.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::default(),
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("sub/*.qmd")]);
        assert!(r.escaped.is_empty() && r.invalid.is_empty());
    }

    #[test]
    fn leading_slash_reanchors_at_the_project_root() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("/posts/*.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::default(),
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("posts/*.qmd")]);
    }

    #[test]
    fn negation_strips_the_bang_and_sets_the_flag() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("*.qmd"), raw("!p2.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::default(),
        );
        assert_eq!(
            r.globs,
            vec![
                GlobPattern::positive("sub/*.qmd"),
                GlobPattern::negated("sub/p2.qmd"),
            ]
        );
    }

    #[test]
    fn escaping_the_project_root_is_reported_not_matched() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("../../*.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::default(),
        );
        assert!(r.globs.is_empty());
        assert_eq!(r.escaped.len(), 1);
        assert_eq!(r.escaped[0].raw, "../../*.qmd");
    }

    #[test]
    fn uncompilable_patterns_are_reported_not_matched() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("a**b.qmd"), raw("data/[.csv"), raw("fine/*.qmd")],
            &ctx(project, ""),
            &GlobOptions::default(),
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("fine/*.qmd")]);
        assert_eq!(r.invalid.len(), 2);
        assert_eq!(r.invalid[0].raw, "a**b.qmd");
        assert!(r.invalid[0].message.contains("recursive wildcards"));
        assert!(r.invalid[1].message.contains("range"));
    }

    #[test]
    fn everything_resolved_compiles() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("posts/*.qmd"), raw("!posts/draft.qmd"), raw("data")],
            &ctx(project, ""),
            &GlobOptions::default(),
        );
        let set = r
            .compile(&GlobOptions::default())
            .expect("resolved compiles");
        assert!(set.matches("posts/a.qmd"));
        assert!(!set.matches("posts/draft.qmd"));
        assert!(set.matches("data/x.csv"));
    }

    // ── default-positive injection ──────────────────────────────

    #[test]
    fn negation_only_injects_the_default_positive() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("!p2.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::LISTING,
        );
        assert_eq!(
            r.globs,
            vec![
                GlobPattern::positive("sub/*.qmd"),
                GlobPattern::negated("sub/p2.qmd"),
            ]
        );
    }

    #[test]
    fn default_positive_is_not_injected_when_a_positive_exists() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("posts"), raw("!posts/draft.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::LISTING,
        );
        assert_eq!(
            r.globs,
            vec![
                GlobPattern::positive("sub/posts"),
                GlobPattern::negated("sub/posts/draft.qmd"),
            ]
        );
    }

    #[test]
    fn no_default_positive_leaves_negation_only_empty() {
        let project = Path::new("/proj");
        let r = resolve_patterns(
            [raw("!p2.qmd")],
            &ctx(project, "sub"),
            &GlobOptions::default(),
        );
        assert_eq!(r.globs, vec![GlobPattern::negated("sub/p2.qmd")]);
        assert!(r.compile(&GlobOptions::default()).unwrap().is_empty());
    }
}
