/*
 * glob/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! q2's shared glob API: one semantics for every consumer
//! (bd-mt7a6uc4).
//!
//! Four subsystems in the tree accept user-written glob patterns —
//! listing `contents:`, `project.render`, `resources:`, and
//! `sidebar.auto:`. Before this module each had its own matcher, its
//! own base-directory rule, and its own silent failure modes. This
//! module is the single implementation they share; the normative
//! semantics table lives in
//! `claude-notes/designs/glob-semantics.md`.
//!
//! # The three layers
//!
//! 1. [`provenance`] — a value's [`SourceInfo`](quarto_source_map::SourceInfo)
//!    to the project-relative directory of the file it was **written
//!    in**. A pattern means what it says relative to its own file:
//!    front matter → the host document's directory, `_metadata.yml` →
//!    that file's directory, `_quarto.yml` → the project root.
//! 2. [`pattern`] — a raw string to a normalized, project-relative
//!    [`GlobPattern`]: negation split off, `.`/`..` collapsed, a
//!    leading `/` re-anchored at the project root, and anything
//!    escaping the project root rejected.
//! 3. [`matcher`] — a [`PatternSet`] that answers "does this
//!    project-relative path match?".
//!
//! [`resolve::resolve_patterns`] runs layers 1–2 over a list of raw
//! patterns and hands back everything a caller needs to diagnose what
//! it could not use.
//!
//! # Matching is separable from walking
//!
//! The matcher wraps [`glob::Pattern`], whose `matches_with` is a
//! **pure** string operation — the `glob` crate's filesystem access
//! lives entirely in its `glob()` walker, which we do not use.
//! Enumeration is the caller's job: listings match against the
//! in-memory project index, and `resources:` walks through
//! [`SystemRuntime`](quarto_system_runtime::SystemRuntime), so both
//! work under WASM against the hub-client VFS.
//!
//! This is what lets q2 keep the full glob vocabulary — including
//! `[a-z]` character classes — without ever touching `std::fs` from
//! this module.

pub mod expand;
pub mod matcher;
pub mod pattern;
pub mod provenance;
pub mod resolve;

use std::path::{Component, Path};

pub use expand::{GlobExpandError, expand};
pub use matcher::{GlobCompileError, PatternSet};
pub use pattern::{GlobPattern, has_metacharacters, join_and_normalize, split_negation};
pub use provenance::BaseDirContext;
pub use resolve::{EscapedGlob, GlobResolution, InvalidGlob, RawGlob, resolve_patterns};

/// Per-consumer knobs.
///
/// The *semantics* are shared — this struct exists only for the
/// differences we can justify in the contract doc. Every field must
/// be explained there; a knob nobody can defend is a bug, not a
/// feature.
///
/// Fields are added when the consumer that needs them lands, so the
/// struct never carries a setting nothing reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobOptions {
    /// A pattern with no metacharacters that names a directory
    /// matches everything beneath it: `posts` matches
    /// `posts/welcome/index.qmd`.
    ///
    /// True for every consumer today (decision D4). It stays a knob
    /// because "this literal string is a directory prefix" is a
    /// policy choice, not a property of glob syntax — a future
    /// consumer matching exact paths would want it off.
    pub directory_rule: bool,

    /// Positive pattern injected when the caller wrote *only*
    /// negations, so `["!draft.qmd"]` means "everything except
    /// `draft.qmd`" rather than "nothing". Resolved against the
    /// fallback base directory.
    pub default_positive: Option<&'static str>,
}

impl Default for GlobOptions {
    fn default() -> Self {
        Self {
            directory_rule: true,
            default_positive: None,
        }
    }
}

impl GlobOptions {
    /// Listing `contents:` — bare directories match beneath, and a
    /// negation-only `contents:` defaults to the host directory's
    /// `*.qmd` siblings (mirroring the absent-`contents:` default).
    pub const LISTING: Self = Self {
        directory_rule: true,
        default_positive: Some("*.qmd"),
    };

    /// `resources:` — project-level (`project.resources`) and
    /// document-level.
    ///
    /// Same shape as `project.render`; the difference is what the
    /// *enumerator* does with the result. Resources expand to files
    /// of any extension, apply no hidden/underscore exclusions
    /// (`.nojekyll` and `_data/` are legitimate resources), and treat
    /// a literal path that names no existing directory as a declared
    /// file whose absence is worth reporting.
    pub const RESOURCES: Self = Self {
        directory_rule: true,
        default_positive: None,
    };

    /// `project.render` in `_quarto.yml`.
    ///
    /// `default_positive` stays `None` — a render list of only
    /// exclusions means "walk the project, minus these", which is
    /// what an empty `render:` already does, and the walk is the
    /// enumerator's job rather than a pattern default.
    pub const RENDER: Self = Self {
        directory_rule: true,
        default_positive: None,
    };
}

/// Every consumer's option set in one table.
///
/// The contract doc (`claude-notes/designs/glob-semantics.md`)
/// promises that per-consumer differences are few, deliberate, and
/// written down. This test is the enforcement: a new consumer or a
/// changed knob shows up here as a diff, which is the moment to ask
/// whether the divergence is defensible.
#[cfg(test)]
mod option_table {
    use super::GlobOptions;

    #[test]
    fn consumer_options_are_as_documented() {
        let table: &[(&str, GlobOptions)] = &[
            ("listing contents:", GlobOptions::LISTING),
            ("project.render", GlobOptions::RENDER),
            ("resources:", GlobOptions::RESOURCES),
        ];

        let rendered: Vec<String> = table
            .iter()
            .map(|(name, o)| {
                format!(
                    "{name}: directory_rule={}, default_positive={:?}",
                    o.directory_rule, o.default_positive
                )
            })
            .collect();

        assert_eq!(
            rendered,
            vec![
                "listing contents:: directory_rule=true, default_positive=Some(\"*.qmd\")",
                "project.render: directory_rule=true, default_positive=None",
                "resources:: directory_rule=true, default_positive=None",
            ]
        );
    }
}

/// Render a path as the canonical candidate string: project-relative,
/// forward slashes, no `.`/`..`/root anchors.
///
/// Every matcher input goes through here, so Windows and Unix compare
/// identically. Non-`Normal` components are dropped rather than
/// rendered, because a candidate that still carries them is not
/// project-relative and could not match a normalized pattern anyway.
pub fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}
