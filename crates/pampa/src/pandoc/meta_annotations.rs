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
//! - A trailing `**` matches the node at the preceding prefix and
//!   its whole subtree (zero or more further segments): `brand.**`
//!   covers `brand` itself (the path-string form) and every leaf of
//!   an inline block, at any depth. Use it for keys whose entire
//!   value is machine-facing; explicit tags still win inside the
//!   subtree.
//!
//! ## See also: the *other* key-path table, and when to pick which
//!
//! There are two registries in the tree that decide how a config
//! string is interpreted, and picking the wrong one is a mistake that
//! has already been made once (bd-qzn1azon; the
//! `bd-page-footer-items-f4th80mj` handoff located its bug correctly
//! and then proposed fixing it here):
//!
//! | | this table (`ANNOTATIONS`) | `MARKDOWN_CONFIG_PATHS` |
//! |---|---|---|
//! | when | **load time**, per untagged scalar | **transform time**, over merged metadata |
//! | for | values that are **not** markdown — globs, paths | website *presentation* strings that **are** markdown |
//! | effect | picks a non-markdown [`Interpretation`] | re-parses `Scalar(String)` as qmd |
//! | honours `!str` | yes — explicit tags win | no — the tag is gone by then |
//!
//! Rule of thumb: **adding markdown semantics to a presentation key
//! goes in `MARKDOWN_CONFIG_PATHS`; protecting a machine-facing key
//! from markdown goes here.** Load-time parsing was considered and
//! rejected for the presentation class — see
//! `claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`.
//!
//! `MARKDOWN_CONFIG_PATHS` lives in
//! `crates/quarto-core/src/transforms/config_markdown.rs` (no intra-doc
//! link: `quarto-core` depends on `pampa`, not the other way round).
//!
//! Both tables are documented as temporary, pending schema-driven
//! interpretation.

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
    // Brand values are machine-facing configuration defined by the
    // brand-yml spec as plain YAML; the same block must mean the same
    // thing in `_brand.yml`, `_quarto.yml`, and front matter
    // (bd-vk4olgv6, GH #581). The subtree form covers the
    // `brand: _brand.yml` path string and every inline-block leaf —
    // including custom fields under `meta`, which the spec allows.
    (&["brand", "**"], Interpretation::PlainString),
];

/// Interpretation annotated for the value at `path`, if any.
/// `path` is the map-key chain from the metadata root (arrays are
/// transparent — see module docs).
pub(crate) fn annotated_interpretation(path: &[String]) -> Option<Interpretation> {
    ANNOTATIONS
        .iter()
        .find(|(pattern, _)| pattern_matches(pattern, path))
        .map(|(_, interp)| *interp)
}

/// Whether `pattern` matches `path`. `*` matches exactly one segment.
/// A trailing `**` matches the node at the preceding prefix *and* any
/// of its descendants (zero or more further segments); it is only
/// meaningful as the final pattern element. Patterns without `**`
/// match exact-length only — never a suffix or prefix.
fn pattern_matches(pattern: &[&str], path: &[String]) -> bool {
    let (min_len, exact) = match pattern.split_last() {
        Some((&"**", prefix)) => (prefix.len(), false),
        _ => (pattern.len(), true),
    };
    if path.len() < min_len || (exact && path.len() != min_len) {
        return false;
    }
    pattern[..min_len]
        .iter()
        .zip(path.iter())
        .all(|(p, seg)| *p == "*" || *p == seg.as_str())
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

    // ── brand subtree (bd-vk4olgv6, GH #581) ─────────────────────

    #[test]
    fn brand_subtree_matches_root_and_descendants() {
        // `["brand", "**"]` covers the `brand` node itself (the
        // `brand: _brand.yml` path form) and every descendant leaf
        // (inline block values), at any depth — including custom
        // fields under `meta`, which the brand-yml spec allows.
        for p in [
            &["brand"][..],
            &["brand", "color", "background"][..],
            &["brand", "light"][..],
            &["brand", "meta", "name"][..],
            &["brand", "meta", "some-unknown-program", "notice"][..],
        ] {
            assert_eq!(
                annotated_interpretation(&path(p)),
                Some(Interpretation::PlainString),
                "path {p:?} must be plain YAML, never markdown"
            );
        }
    }

    #[test]
    fn brand_subtree_no_false_positives() {
        // Subtree matching is anchored at the metadata root: user
        // keys that merely contain or resemble `brand` are not
        // captured.
        assert_eq!(annotated_interpretation(&path(&["my", "brand"])), None);
        assert_eq!(annotated_interpretation(&path(&["brandx"])), None);
        assert_eq!(
            annotated_interpretation(&path(&["format", "html", "brand"])),
            None,
            "no consumer reads format.*.brand; keep the table minimal"
        );
    }

    #[test]
    fn exact_length_entries_unaffected_by_subtree_support() {
        // The pre-existing exact-length semantics must not loosen:
        // `listing.contents` still refuses descendants.
        assert_eq!(
            annotated_interpretation(&path(&["listing", "contents", "title"])),
            None
        );
    }
}
