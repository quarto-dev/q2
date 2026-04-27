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
///
/// **Version history:**
/// - `1`: Initial Phase-0 shape.
/// - `2`: Phase-8 (`bd-r82e`). Adds `includes` (transitive include set),
///   `nav_dependencies` (user-declared cross-doc edges),
///   `always_render` (per-doc Pass-2 opt-out), and
///   `body_link_targets` (Pass-1-resolved cross-doc body link set).
pub const DOCUMENT_PROFILE_VERSION: u32 = 2;

/// Depth used when extracting the heading outline at the profile
/// checkpoint.
///
/// We always extract the maximum depth (6 = all HTML heading levels).
/// Consumers can truncate to a shallower depth for their own use.
const OUTLINE_MAX_DEPTH: i32 = 6;

/// One entry in a document's transitive include set.
///
/// Recorded by `IncludeExpansionStage` for every file whose content
/// gets spliced into the parent AST via `{{< include child.qmd >}}`.
/// `path` is the resolved (canonical) path used to read the file;
/// `content_hash` is the SHA-256 of the raw bytes that were spliced
/// in, so cache invalidation can detect any byte change to a child
/// without re-reading the file.
///
/// Cycles in the include graph are pre-truncated by
/// `IncludeExpansionStage`; each unique resolved file appears at
/// most once in the recorded list.
///
/// Phase-8 introduces this type for incremental-rebuild cache key
/// invalidation (`bd-r82e`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncludeEntry {
    /// Resolved path of the included file. Canonical when possible;
    /// otherwise the join of the parent's directory + the include
    /// path as written.
    pub path: PathBuf,

    /// SHA-256 of the file's raw byte contents at splice time.
    /// 32 bytes = 256 bits.
    pub content_hash: [u8; 32],
}

impl IncludeEntry {
    /// Compute the content hash for a byte slice. Provided here so
    /// callers and tests share one hashing convention.
    pub fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Construct an entry from a path and the raw bytes that were
    /// spliced. `bytes` is hashed; the entry stores only the hash.
    pub fn new(path: PathBuf, bytes: &[u8]) -> Self {
        Self {
            path,
            content_hash: Self::hash_bytes(bytes),
        }
    }
}

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

    /// Transitive include set (`bd-r82e`). Populated by
    /// `IncludeExpansionStage` for every file whose contents were
    /// spliced into the parent AST via `{{< include child.qmd >}}`.
    /// Each entry carries the file's resolved path and a SHA-256 of
    /// its byte contents at splice time.
    ///
    /// Phase-8 incremental rebuilds use this to invalidate a parent's
    /// profile cache entry when any (transitive) include's content
    /// changes.
    ///
    /// Default empty (no includes); serializer omits empty lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<IncludeEntry>,

    /// User-declared dependencies on other project documents. Read
    /// from `meta.project.nav-dependencies` (frontmatter,
    /// `_metadata.yml`, or `_quarto.yml`). Each path is project-
    /// relative, resolved at graph-build time.
    ///
    /// Phase-8's dependency graph adds an edge from this page to
    /// every declared target. Targets that don't resolve to a
    /// project document produce a diagnostic and are dropped.
    ///
    /// Use case: a user-supplied Lua filter walks the project and
    /// reads sibling profiles in a way the automatic graph builder
    /// cannot infer; declaring those siblings here keeps incremental
    /// rebuilds correct under Mode B.
    ///
    /// Default empty; serializer omits empty lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nav_dependencies: Vec<PathBuf>,

    /// User-declared "always re-render this page" flag. Read from
    /// `meta.project.always-render`. Default `false`.
    ///
    /// Set when a page's filters introduce non-deterministic content
    /// (a random quote at the footer, the current date, etc.) or
    /// unmodelable dependencies. In Phase-8 Mode B (subset render),
    /// a page with this flag set joins the render set whenever any
    /// of its dependents is in the user-named targets.
    ///
    /// Mode A (full project render) re-renders every page anyway, so
    /// this flag has no Mode-A effect.
    #[serde(default, skip_serializing_if = "is_false")]
    pub always_render: bool,

    /// Project-relative `.qmd` paths this page links to in its body.
    /// Populated by `LinkResolutionStage` during Pass-1 — the
    /// read-only, side-effect-free counterpart to Phase 6's Pass-2
    /// `LinkRewriteTransform`. Both share the same resolution
    /// helper; an equivalence test asserts they produce the same
    /// target set for the same AST.
    ///
    /// Phase-8's dependency graph turns each entry into an edge
    /// from this page to the target. External URLs, fragment-only
    /// links, and unresolved targets are excluded.
    ///
    /// Default empty; serializer omits empty lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_link_targets: Vec<PathBuf>,
}

/// Helper for `#[serde(skip_serializing_if = ...)]` on plain bool
/// defaults. Keeping the helper local so the profile struct doesn't
/// pull in `serde_with`.
fn is_false(b: &bool) -> bool {
    !b
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

impl Default for DocumentProfile {
    /// A minimal v2 profile with empty paths and no metadata.
    /// Useful for test fixtures that build profiles by overriding
    /// only the fields a particular test cares about (`..Default::default()`).
    /// Production code should use [`DocumentProfile::extract`].
    fn default() -> Self {
        Self {
            profile_version: DOCUMENT_PROFILE_VERSION,
            source_path: PathBuf::new(),
            output_href: String::new(),
            format_id: String::new(),
            title: None,
            subtitle: None,
            description: None,
            authors: Vec::new(),
            date: None,
            categories: Vec::new(),
            keywords: Vec::new(),
            image: None,
            draft: false,
            order: None,
            outline: Vec::new(),
            includes: Vec::new(),
            nav_dependencies: Vec::new(),
            always_render: false,
            body_link_targets: Vec::new(),
        }
    }
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
    ///
    /// Phase-8 fields (`includes`, `nav_dependencies`, `always_render`,
    /// `body_link_targets`) are populated by the calling stage:
    /// - `includes` is drained from the include-expansion side-channel.
    /// - `nav_dependencies` and `always_render` come from
    ///   `meta.project.*` and *are* read here by `extract` since
    ///   they're plain metadata reads.
    /// - `body_link_targets` is populated by `LinkResolutionStage`
    ///   later in Pass-1.
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
            // Phase-8 fields that come from the AST's `meta.project.*`
            // namespace are populated here. `includes` and
            // `body_link_targets` are populated by the stage from
            // side-channels.
            includes: Vec::new(),
            nav_dependencies: extract_path_list(meta, &["project", "nav-dependencies"]),
            always_render: meta_bool_path(meta, &["project", "always-render"]).unwrap_or(false),
            body_link_targets: Vec::new(),
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

/// Walk a dotted path of keys down a `ConfigValue` map and read a
/// boolean leaf. Returns `None` if any key is absent or the leaf is
/// not a bool.
fn meta_bool_path(meta: &ConfigValue, path: &[&str]) -> Option<bool> {
    let mut cur = meta;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_bool()
}

/// Walk a dotted path of keys down a `ConfigValue` map and extract a
/// list of project-relative paths.
///
/// Accepts either a YAML array of strings or a single scalar (treated
/// as a one-element list). Each element is stored as a `PathBuf` with
/// no further normalization here — graph-build time resolves them
/// against the project root and validates existence.
///
/// Returns an empty `Vec` if the path is absent or has the wrong shape.
fn extract_path_list(meta: &ConfigValue, path: &[&str]) -> Vec<PathBuf> {
    let mut cur = meta;
    for key in path {
        let Some(next) = cur.get(key) else {
            return Vec::new();
        };
        cur = next;
    }
    if let Some(arr) = cur.as_array() {
        arr.iter()
            .filter_map(|v| v.as_plain_text().map(PathBuf::from))
            .collect()
    } else if let Some(s) = cur.as_plain_text() {
        vec![PathBuf::from(s)]
    } else {
        Vec::new()
    }
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

    // === Phase-8 sub-phase 8.0 (bd-r82e + new fields) ====================

    #[test]
    fn profile_v1_json_rejected_with_clean_error() {
        // A v1 profile (the pre-Phase-8 shape) must be rejected by
        // from_json. v1 callers wrote profiles with profile_version: 1
        // and no `includes`/`nav_dependencies`/`always_render`/
        // `body_link_targets` fields. After the v2 bump, those entries
        // become invalid and Phase-8 silently regenerates them.
        let payload = r#"{
            "profile_version": 1,
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
        }"#;

        match DocumentProfile::from_json(payload) {
            Err(DocumentProfileError::VersionMismatch { expected, found }) => {
                assert_eq!(expected, 2);
                assert_eq!(found, 1);
            }
            other => panic!("expected VersionMismatch from v1 payload, got {:?}", other),
        }
    }

    #[test]
    fn include_entry_hash_bytes_is_deterministic() {
        // Property: same bytes → same hash; different bytes → different hash.
        let h1 = IncludeEntry::hash_bytes(b"hello world");
        let h2 = IncludeEntry::hash_bytes(b"hello world");
        let h3 = IncludeEntry::hash_bytes(b"hello there");
        assert_eq!(h1, h2, "hash is deterministic for identical input");
        assert_ne!(h1, h3, "hash differs for different input");
        // Sanity: SHA-256 of empty string is a known value.
        let empty = IncludeEntry::hash_bytes(b"");
        let empty_hex: String = empty.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            empty_hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "SHA-256(empty) matches the canonical value"
        );
    }

    #[test]
    fn include_entry_new_constructs_from_path_and_bytes() {
        let entry = IncludeEntry::new(PathBuf::from("child.qmd"), b"included body");
        assert_eq!(entry.path, PathBuf::from("child.qmd"));
        assert_eq!(
            entry.content_hash,
            IncludeEntry::hash_bytes(b"included body")
        );
    }

    #[test]
    fn profile_v2_round_trip_with_includes() {
        let mut p =
            DocumentProfile::extract(&Pandoc::default(), Path::new("a.qmd"), "a.html", "html");
        p.includes
            .push(IncludeEntry::new(PathBuf::from("inc/header.qmd"), b"H"));
        p.includes
            .push(IncludeEntry::new(PathBuf::from("inc/footer.qmd"), b"F"));

        let json = p.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(p, restored);
        assert_eq!(restored.includes.len(), 2);
        assert_eq!(restored.includes[0].path, PathBuf::from("inc/header.qmd"));
    }

    #[test]
    fn profile_records_nav_dependencies_from_frontmatter() {
        let qmd = "\
---
title: Foo
project:
  nav-dependencies:
    - a.qmd
    - sub/b.qmd
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("foo.qmd"), "foo.html", "html");
        assert_eq!(
            p.nav_dependencies,
            vec![PathBuf::from("a.qmd"), PathBuf::from("sub/b.qmd")]
        );
    }

    #[test]
    fn profile_nav_dependencies_default_empty() {
        let ast = parse_qmd("---\ntitle: No deps\n---\n\nBody.\n");
        let p = DocumentProfile::extract(&ast, Path::new("nd.qmd"), "nd.html", "html");
        assert!(p.nav_dependencies.is_empty());
    }

    #[test]
    fn profile_nav_dependencies_accepts_single_scalar() {
        let qmd = "\
---
title: One dep
project:
  nav-dependencies: a.qmd
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("d.qmd"), "d.html", "html");
        assert_eq!(p.nav_dependencies, vec![PathBuf::from("a.qmd")]);
    }

    #[test]
    fn profile_records_always_render_true() {
        let qmd = "\
---
title: Volatile
project:
  always-render: true
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("v.qmd"), "v.html", "html");
        assert!(p.always_render);
    }

    #[test]
    fn profile_always_render_default_false() {
        let ast = parse_qmd("---\ntitle: Stable\n---\n\nBody.\n");
        let p = DocumentProfile::extract(&ast, Path::new("s.qmd"), "s.html", "html");
        assert!(!p.always_render);
    }

    #[test]
    fn profile_body_link_targets_default_empty() {
        // body_link_targets is populated by LinkResolutionStage post-extract,
        // so a fresh extract produces an empty vec.
        let ast = parse_qmd("---\ntitle: T\n---\n\nText with [link](sibling.qmd).\n");
        let p = DocumentProfile::extract(&ast, Path::new("t.qmd"), "t.html", "html");
        assert!(p.body_link_targets.is_empty());
    }

    #[test]
    fn profile_v2_skip_serializing_empty_optional_fields() {
        // The new collection fields are tagged
        // `skip_serializing_if = "Vec::is_empty"`, and `always_render`
        // skips when false. A profile with all defaults should serialize
        // without any of those keys appearing in the JSON, so v2 cache
        // entries stay compact.
        let p = DocumentProfile::extract(&Pandoc::default(), Path::new("a.qmd"), "a.html", "html");
        let json = p.to_json().unwrap();
        assert!(!json.contains("\"includes\""));
        assert!(!json.contains("\"nav_dependencies\""));
        assert!(!json.contains("\"always_render\""));
        assert!(!json.contains("\"body_link_targets\""));
        // And it round-trips.
        let restored = DocumentProfile::from_json(&json).unwrap();
        assert_eq!(p, restored);
    }
}
