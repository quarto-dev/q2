//! Static document snapshot extracted at the pipeline checkpoint.
//!
//! See `claude-notes/designs/document-profile-contract.md` for the
//! full contract: what each field guarantees, what is explicitly not
//! guaranteed, and when [`DOCUMENT_PROFILE_VERSION`] must be bumped.
//!
//! High level: after [`MetadataMergeStage`] and before any AST
//! mutation, the `DocumentProfileStage` extracts a typed,
//! `serde`-serializable snapshot of the document. Project-scoped
//! features (sidebars, cross-document link rewriting, incremental
//! rebuild caching, eventual `freeze`) consume this snapshot without
//! needing to re-run the engine or user filters.
//!
//! [`MetadataMergeStage`]: crate::stage::MetadataMergeStage

use std::path::{Path, PathBuf};

use pampa::toc::{TocConfig, TocEntry, generate_toc};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version tag written into every serialized profile.
///
/// Consumers reading a profile from disk must check this and reject on
/// mismatch. Bump whenever the serialized shape changes in a way that
/// a v1 consumer would misread — including adding a required field,
/// renaming a field, or changing the semantics of an existing field.
pub const DOCUMENT_PROFILE_VERSION: u32 = 1;

/// Depth used when extracting the heading outline at the profile
/// checkpoint.
///
/// We always extract the maximum depth (6 = all HTML heading levels).
/// Consumers can truncate to a shallower depth for their own use.
const OUTLINE_MAX_DEPTH: i32 = 6;

/// Static snapshot of a document, extracted at the pipeline
/// checkpoint after metadata merge and before any AST mutation.
///
/// See the module-level docs and the contract document at
/// `claude-notes/designs/document-profile-contract.md` for the
/// guarantees on each field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentProfile {
    /// Shape-version tag. Always equals [`DOCUMENT_PROFILE_VERSION`]
    /// when extracted in-process; checked on deserialization.
    pub profile_version: u32,

    /// Source path, project-relative, forward-slash separated. See
    /// §"Project root invariant" in the Phase-0 plan — a bare file
    /// is a single-file project rooted at its directory, so this is
    /// just the file name in that case.
    pub source_path: PathBuf,

    /// URL path other pages should use to link to this document
    /// (e.g. `"about.html"` or `"docs/api.html"`). Relative to the
    /// project's output directory, forward-slash separated.
    pub output_href: String,

    /// Target format identifier (e.g. `"html"`, `"acm-html"`). Mirrors
    /// `ctx.format.target_format` at the checkpoint.
    pub format_id: String,

    /// Document title, plain text. `None` when the frontmatter has no
    /// title and no first-heading fallback was applied.
    pub title: Option<String>,

    pub subtitle: Option<String>,
    pub description: Option<String>,

    /// Authors, flat list of plain-text names. See the Phase-0 plan:
    /// a structured author model is deliberately deferred.
    pub authors: Vec<String>,

    pub date: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub image: Option<String>,
    pub draft: bool,

    /// Author-supplied sort key from `order:` frontmatter. Consumed by
    /// Phase-2 auto-sidebar expansion to order sibling entries. `None`
    /// when the key is absent or non-integer. Additive on top of
    /// v1 (see the contract doc's change log); an older v1 consumer
    /// silently ignores it, so `profile_version` is not bumped.
    #[serde(default)]
    pub order: Option<i32>,

    /// Heading outline. Always un-numbered: `TocEntry::number` is
    /// `None` on every entry. Consumers needing numbered outlines
    /// must read them from the post-render AST, not from the profile.
    pub outline: Vec<TocEntry>,
}

/// Errors from loading a serialized [`DocumentProfile`].
#[derive(Debug, Error)]
pub enum DocumentProfileError {
    #[error("failed to parse DocumentProfile JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error(
        "DocumentProfile version mismatch: expected {expected}, got {found}. \
         This profile was produced by a different version of Quarto and must \
         be regenerated."
    )]
    VersionMismatch { expected: u32, found: u32 },
}

impl DocumentProfile {
    /// The version tag emitted by this build.
    pub const VERSION: u32 = DOCUMENT_PROFILE_VERSION;

    /// Extract a profile from a `Pandoc` AST at the pipeline
    /// checkpoint.
    ///
    /// Pure: no I/O, no runtime calls. Takes already-computed
    /// project-relative paths so the caller (the stage) decides the
    /// project-root math in one place.
    pub fn extract(ast: &Pandoc, source_path: &Path, output_href: &str, format_id: &str) -> Self {
        let meta = &ast.meta;
        let outline = extract_outline(&ast.blocks);

        Self {
            profile_version: DOCUMENT_PROFILE_VERSION,
            source_path: source_path.to_path_buf(),
            output_href: output_href.to_string(),
            format_id: format_id.to_string(),
            title: plain_text_field(meta, "title"),
            subtitle: plain_text_field(meta, "subtitle"),
            description: plain_text_field(meta, "description"),
            authors: extract_authors(meta),
            date: plain_text_field(meta, "date"),
            categories: extract_string_list(meta, "categories"),
            keywords: extract_string_list(meta, "keywords"),
            image: plain_text_field(meta, "image"),
            draft: meta.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
            order: meta
                .get("order")
                .and_then(|v| v.as_int())
                .and_then(|i| i32::try_from(i).ok()),
            outline,
        }
    }

    /// Serialize the profile to a JSON string.
    pub fn to_json(&self) -> Result<String, DocumentProfileError> {
        serde_json::to_string(self).map_err(Into::into)
    }

    /// Deserialize a profile from JSON, rejecting version mismatches.
    pub fn from_json(s: &str) -> Result<Self, DocumentProfileError> {
        let profile: DocumentProfile = serde_json::from_str(s)?;
        if profile.profile_version != DOCUMENT_PROFILE_VERSION {
            return Err(DocumentProfileError::VersionMismatch {
                expected: DOCUMENT_PROFILE_VERSION,
                found: profile.profile_version,
            });
        }
        Ok(profile)
    }
}

/// Pull a plain-text field out of the document metadata, flattening
/// markdown-inline values to text when necessary.
fn plain_text_field(meta: &ConfigValue, key: &str) -> Option<String> {
    meta.get(key).and_then(|v| v.as_plain_text())
}

/// Extract a list of plain-text strings. Accepts either a YAML array
/// of strings or a single scalar (treated as a one-element list).
fn extract_string_list(meta: &ConfigValue, key: &str) -> Vec<String> {
    let Some(value) = meta.get(key) else {
        return Vec::new();
    };
    if let Some(arr) = value.as_array() {
        arr.iter().filter_map(|v| v.as_plain_text()).collect()
    } else if let Some(s) = value.as_plain_text() {
        vec![s]
    } else {
        Vec::new()
    }
}

/// Authors extraction. Accepts:
///
/// - A single scalar string → `["Jane Doe"]`
/// - An array of strings → `["Jane Doe", "John Smith"]`
/// - An array of maps with a `name` key → `[{name: "Jane Doe", …}, …]`
/// - A single map with a `name` key → `[name]`
///
/// Structured author metadata (affiliation, email, ORCID, etc.) is
/// deliberately dropped in Phase 0. A dedicated author-model pass is
/// planned as a separate epic.
fn extract_authors(meta: &ConfigValue) -> Vec<String> {
    // Quarto historically accepts both `author` and `authors`.
    for key in ["author", "authors"] {
        if let Some(value) = meta.get(key) {
            if let Some(arr) = value.as_array() {
                let names: Vec<String> = arr.iter().filter_map(author_entry_name).collect();
                if !names.is_empty() {
                    return names;
                }
            } else if let Some(name) = author_entry_name(value) {
                return vec![name];
            }
        }
    }
    Vec::new()
}

fn author_entry_name(value: &ConfigValue) -> Option<String> {
    value
        .as_plain_text()
        .or_else(|| value.get("name").and_then(|v| v.as_plain_text()))
}

/// Extract the heading outline from the document body.
///
/// Always un-numbered: the profile runs pre-sugar, so the TOC builder
/// naturally produces `TocEntry::number == None`, but we scrub the
/// tree defensively so the invariant holds even if `generate_toc`
/// grows new numbering behavior.
fn extract_outline(blocks: &[quarto_pandoc_types::block::Block]) -> Vec<TocEntry> {
    let toc = generate_toc(
        blocks,
        &TocConfig {
            depth: OUTLINE_MAX_DEPTH,
            title: None,
        },
    );
    let mut entries = toc.entries;
    strip_numbers(&mut entries);
    entries
}

fn strip_numbers(entries: &mut Vec<TocEntry>) {
    for entry in entries.iter_mut() {
        entry.number = None;
        strip_numbers(&mut entry.children);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pampa::readers::qmd;

    /// Parse a qmd fragment directly with pampa and return the raw
    /// Pandoc AST. Used by tests that exercise `DocumentProfile::extract`
    /// in isolation (no metadata merge, no transforms).
    ///
    /// NOTE: this bypasses `MetadataMergeStage`, so `ast.meta` here is
    /// the *raw* frontmatter, not the merged metadata. For Phase-0
    /// unit tests this is fine — the profile extractor reads individual
    /// keys that have the same shape in raw and merged metadata.
    /// Merged-metadata scenarios are covered by the pipeline
    /// integration tests.
    fn parse_qmd(qmd: &str) -> Pandoc {
        let mut output = Vec::<u8>::new();
        let (ast, _ast_context, _warnings) =
            qmd::read(qmd.as_bytes(), false, "test.qmd", &mut output, true, None)
                .expect("parse qmd fixture");
        ast
    }

    fn entry_ids(entries: &[TocEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.id.as_str()).collect()
    }

    fn all_unnumbered(entries: &[TocEntry]) -> bool {
        entries
            .iter()
            .all(|e| e.number.is_none() && all_unnumbered(&e.children))
    }

    #[test]
    fn profile_extract_minimal_document() {
        let ast = parse_qmd("---\ntitle: Hello\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("doc.qmd"), "doc.html", "html");

        assert_eq!(profile.profile_version, DOCUMENT_PROFILE_VERSION);
        assert_eq!(profile.source_path, PathBuf::from("doc.qmd"));
        assert_eq!(profile.output_href, "doc.html");
        assert_eq!(profile.format_id, "html");
        assert_eq!(profile.title.as_deref(), Some("Hello"));
        assert!(!profile.draft);
        assert!(profile.outline.is_empty(), "no headings → empty outline");
    }

    #[test]
    fn profile_extract_with_headings() {
        let qmd = "\
---
title: Outline test
---

# Top

Intro text.

## Sub

Sub text.

### Deep

Deep text.

# Top two
";
        let ast = parse_qmd(qmd);
        let profile =
            DocumentProfile::extract(&ast, Path::new("outline.qmd"), "outline.html", "html");

        assert_eq!(profile.outline.len(), 2, "two top-level headings");
        let first = &profile.outline[0];
        assert_eq!(first.level, 1);
        assert_eq!(first.title, "Top");
        assert_eq!(entry_ids(&first.children), vec!["sub"]);
        assert_eq!(first.children[0].level, 2);
        assert_eq!(entry_ids(&first.children[0].children), vec!["deep"]);
        assert_eq!(profile.outline[1].title, "Top two");
        assert!(
            all_unnumbered(&profile.outline),
            "profile outline must be un-numbered"
        );
    }

    #[test]
    fn profile_extract_with_full_frontmatter() {
        let qmd = "\
---
title: Big doc
subtitle: With everything
description: A thorough example.
author:
  - Alice Example
  - Bob Example
date: 2026-04-23
categories: [tutorial, intro]
keywords: [quarto, rust]
image: cover.png
draft: true
---

Body.
";
        let ast = parse_qmd(qmd);
        let profile = DocumentProfile::extract(&ast, Path::new("big.qmd"), "big.html", "html");

        assert_eq!(profile.title.as_deref(), Some("Big doc"));
        assert_eq!(profile.subtitle.as_deref(), Some("With everything"));
        assert_eq!(profile.description.as_deref(), Some("A thorough example."));
        assert_eq!(
            profile.authors,
            vec!["Alice Example".to_string(), "Bob Example".to_string()]
        );
        assert_eq!(profile.date.as_deref(), Some("2026-04-23"));
        assert_eq!(
            profile.categories,
            vec!["tutorial".to_string(), "intro".to_string()]
        );
        assert_eq!(
            profile.keywords,
            vec!["quarto".to_string(), "rust".to_string()]
        );
        assert_eq!(profile.image.as_deref(), Some("cover.png"));
        assert!(profile.draft);
    }

    #[test]
    fn profile_extract_carries_order_when_present() {
        // `order:` is used by Phase-2's auto-sidebar sort. The field is
        // additive on DocumentProfile; Phase-2 is the first consumer.
        let ast = parse_qmd("---\ntitle: Ordered\norder: 3\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("o.qmd"), "o.html", "html");
        assert_eq!(profile.order, Some(3));
    }

    #[test]
    fn profile_extract_order_absent_is_none() {
        let ast = parse_qmd("---\ntitle: No order\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("x.qmd"), "x.html", "html");
        assert_eq!(profile.order, None);
    }

    #[test]
    fn profile_extract_rejects_non_integer_order() {
        // A string `order:` is not a valid sort key; the profile drops it
        // rather than guessing.  (A diagnostic would be nice; that's a
        // follow-up that belongs in the metadata-validator, not here.)
        let ast = parse_qmd("---\ntitle: Bad order\norder: \"abc\"\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("bad.qmd"), "bad.html", "html");
        assert_eq!(profile.order, None);
    }

    #[test]
    fn profile_extract_handles_missing_title() {
        let ast = parse_qmd("Just a paragraph.\n");
        let profile =
            DocumentProfile::extract(&ast, Path::new("untitled.qmd"), "untitled.html", "html");

        assert_eq!(profile.title, None);
        assert!(profile.outline.is_empty());
        assert!(profile.authors.is_empty());
        assert!(profile.categories.is_empty());
    }

    #[test]
    fn profile_roundtrip_json() {
        let ast = parse_qmd("---\ntitle: Roundtrip\n---\n\n# One\n\n## Two\n");
        let profile = DocumentProfile::extract(&ast, Path::new("rt.qmd"), "rt.html", "html");

        let json = profile.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(profile, restored);
    }

    #[test]
    fn profile_version_mismatch_rejected() {
        let payload = format!(
            r#"{{
                "profile_version": {},
                "source_path": "x.qmd",
                "output_href": "x.html",
                "format_id": "html",
                "title": null,
                "subtitle": null,
                "description": null,
                "authors": [],
                "date": null,
                "categories": [],
                "keywords": [],
                "image": null,
                "draft": false,
                "outline": []
            }}"#,
            DOCUMENT_PROFILE_VERSION + 1
        );

        match DocumentProfile::from_json(&payload) {
            Err(DocumentProfileError::VersionMismatch { expected, found }) => {
                assert_eq!(expected, DOCUMENT_PROFILE_VERSION);
                assert_eq!(found, DOCUMENT_PROFILE_VERSION + 1);
            }
            other => panic!("expected VersionMismatch, got {:?}", other),
        }
    }
}
