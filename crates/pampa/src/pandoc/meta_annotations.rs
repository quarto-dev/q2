/*
 * pandoc/meta_annotations.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! **Temporary** key-path interpretation annotations for metadata
//! conversion (bd-v7ixzsp5, GH #456).
//!
//! Some metadata keys hold values whose semantics are known a
//! priori — `listing.contents` entries are glob patterns, never
//! markdown. Interpreting them with the untagged-string default
//! (markdown, in document-metadata context) either warns (`Q-1-20`
//! when the glob fails to parse as markdown) or silently corrupts
//! the pattern (`p*osts*.qmd` parses as emphasis and flattens back
//! to `posts.qmd`).
//!
//! This module is the **annotation source** consulted by
//! [`yaml_to_config_value`](super::meta::yaml_to_config_value) for
//! untagged scalars: a declarative table mapping key paths to the
//! [`Interpretation`] the value should get. Explicit tags (`!str`,
//! `!md`, …) always win; the annotation only replaces the
//! *untagged* default.
//!
//! ## Lifecycle: delete me when schemas land
//!
//! The long-term design is for YAML schemas to drive default
//! interpretations once full schema validation is wired in. When
//! that happens, this hand-written table is superseded: delete this
//! module and the single `annotated_interpretation` consult in
//! `meta.rs`, and derive the same information from the schema.
//! Keep the table small and boring until then — one entry per key
//! whose misinterpretation is an actual observed bug, not a
//! speculative registry.
//!
//! ## Path semantics
//!
//! - A path is the chain of map keys from the metadata root to the
//!   value, e.g. `listing.contents`.
//! - **Arrays are transparent**: items of the array at
//!   `listing.contents` have path `listing.contents`.
//! - **Maps are not**: an inline record `{title: …}` inside
//!   `contents` puts its fields at `listing.contents.title`, which
//!   no entry matches — record fields keep the normal default
//!   (markdown), so authors still get markdown diagnostics there.
//! - `*` in a pattern matches exactly one segment
//!   (`format.*.listing.contents` covers per-format nesting).
//!   Matching is exact-length, never a suffix match, so a user's
//!   unrelated `my.listing.contents` key is not captured.

use quarto_config::Interpretation;

/// The annotation table: key-path pattern → interpretation for
/// untagged scalars at that path. See the module docs for the path
/// semantics and for why this stays deliberately small.
const ANNOTATIONS: &[(&[&str], Interpretation)] = &[
    (&["listing", "contents"], Interpretation::Glob),
    (
        &["format", "*", "listing", "contents"],
        Interpretation::Glob,
    ),
];

/// Interpretation annotated for the value at `path`, if any.
/// `path` is the map-key chain from the metadata root (arrays are
/// transparent — see module docs).
pub(crate) fn annotated_interpretation(path: &[String]) -> Option<Interpretation> {
    ANNOTATIONS
        .iter()
        .find(|(pattern, _)| {
            pattern.len() == path.len()
                && pattern
                    .iter()
                    .zip(path.iter())
                    .all(|(p, seg)| *p == "*" || *p == seg.as_str())
        })
        .map(|(_, interp)| *interp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn listing_contents_matches() {
        assert_eq!(
            annotated_interpretation(&path(&["listing", "contents"])),
            Some(Interpretation::Glob)
        );
    }

    #[test]
    fn format_nested_listing_contents_matches_any_format() {
        assert_eq!(
            annotated_interpretation(&path(&["format", "html", "listing", "contents"])),
            Some(Interpretation::Glob)
        );
        assert_eq!(
            annotated_interpretation(&path(&["format", "revealjs", "listing", "contents"])),
            Some(Interpretation::Glob)
        );
    }

    #[test]
    fn exact_length_no_suffix_matching() {
        // A user's unrelated nested key is NOT captured.
        assert_eq!(
            annotated_interpretation(&path(&["my", "listing", "contents"])),
            None
        );
        // Fields of inline records under contents are NOT captured.
        assert_eq!(
            annotated_interpretation(&path(&["listing", "contents", "title"])),
            None
        );
        // Prefixes are not captured either.
        assert_eq!(annotated_interpretation(&path(&["listing"])), None);
    }

    #[test]
    fn unrelated_paths_do_not_match() {
        assert_eq!(annotated_interpretation(&path(&["title"])), None);
        assert_eq!(annotated_interpretation(&path(&[])), None);
    }
}
