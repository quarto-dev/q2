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

use std::collections::BTreeMap;
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
/// - `3`: `bd-o8pr`. Adds `resources` (document-level `resources:`
///   patterns from frontmatter; the post-render collector expands).
/// - `4`: `bd-n8a4` (listings epic, L0). Adds `listing_item:
///   ListingItemInfo` for the listings feature surface (curated typed
///   fields plus `extra: BTreeMap<String, ConfigValue>` for custom
///   templates) and `categories_raw: Option<ConfigValue>` so listing
///   consumers can apply tag-aware merging via `MergedConfig` when
///   combining top-level and listing-item categories.
/// - `5`: `bd-xbnf` (listings epic, L6). Adds `listing_content_globs:
///   Vec<String>` — the unresolved glob strings declared on the host
///   page's `listing.*.contents:`. The dependency-graph builder
///   expands these at graph-build time to add forward edges from each
///   listing host to its content files, so Mode B (`quarto render
///   posts/foo.qmd`) automatically pulls in listing hosts when any of
///   their content files are targeted.
/// - `6`: `bd-c1et2`. Changes `resources` from `Vec<String>` to
///   `Vec<RawResourcePattern>` so each pattern carries its YAML
///   `SourceInfo`. Diagnostics for out-of-project / invalid-glob /
///   glob-walk errors can now render an Ariadne span pointing at the
///   offending scalar. v5 serialized profiles will fail to
///   deserialize at the field level; cached frozen profiles need to
///   be regenerated.
/// - `7`: `bd-ez0hiowa` (title-block parity epic, P2). Adds
///   `authors_structured: Vec<ProfileAuthor>` — the structured author
///   model (name components, ORCID, email, url, degrees, attribute
///   flags, denormalized affiliations) that the Phase-0 note below
///   deferred. The flat `authors: Vec<String>` field is unchanged in
///   type and now derives its literals from the same normalization
///   (`metadata::authors::parse_authors_model`), so the two fields
///   always agree.
/// - `8`: `bd-v7ixzsp5` (GH #456). Changes `listing_content_globs`
///   from `Vec<String>` (raw patterns, expanded dual-view at
///   graph-build time) to `Vec<GlobPattern>` — patterns are
///   now **resolved to project-relative form at profile-extraction
///   time** against the directory of the file each glob was written
///   in (front matter → host dir, `_metadata.yml` → its dir,
///   `_quarto.yml` → project root), and carry a `negated` flag for
///   `!`-prefixed exclusion patterns. Consumers match single-view
///   via [`crate::glob::PatternSet`].
/// - `9`: `bd-mt7a6uc4`. Adds `resource_globs: Vec<GlobPattern>` — the
///   document's `resources:` patterns resolved to project-relative
///   form at profile-extraction time, against the directory of the
///   file each was written in. The raw `resources` field stays as-is
///   because `ResourceReportStage` compares raw pattern strings to
///   detect filter-added entries; the two answer different questions.
/// - `10`: `bd-aliases-redirects-missing-sch7cd1g`. Adds
///   `aliases: Vec<String>` and the index-aligned
///   `alias_sources: Vec<SourceInfo>` — the document's `aliases:`
///   front-matter entries, kept **raw**. Unlike `resource_globs`,
///   these are deliberately *not* resolved at extraction time:
///   resolving an alias needs only the page's own `output_href`
///   (already on the profile) and no filesystem, while *validating*
///   one needs every other page in the project. Resolution and
///   collision detection therefore both live in the post-render pass,
///   which is also the only place a diagnostic survives profile
///   caching (see `rejected_resources` for the same lesson learned
///   the hard way).
/// - `11`: `bd-toc-smart-quotes-6nro57ed`. Changes `outline`'s entry
///   titles from `String` to `Inlines` (`TocEntry::title`). The
///   outline now carries the heading's inline markup — emphasis,
///   code, math, and the quoted-span delimiters a flattened title
///   silently dropped — so it is a faithful semantic outline rather
///   than a lossy projection. **Serialized shape changes**: a title
///   that was `"Top"` is now an array of inline nodes, so v10
///   profiles fail to deserialize at the field level and cached
///   profiles must be regenerated. Consumers wanting plain text
///   project it themselves with
///   `pampa::writers::plaintext::inlines_to_string`.
/// - `12`: Plan 6 Phase 5
///   (`claude-notes/plans/2026-06-29-plan6-pass1-engine-resolution.md`).
///   Adds `engine_resolution: Option<ProfileEngineResolution>` — the
///   Pass-1-resolved engine sequence/ownership (names only; configs stay
///   in merged metadata), stamped by `DocumentProfileStage` from
///   `resolve_engines_pass1`. `None` means the document could not be
///   resolved load-free at index time — advisory, not an error; Pass-2
///   always re-resolves via the full loading resolver regardless.
pub const DOCUMENT_PROFILE_VERSION: u32 = 12;

/// Reduced, serializable form of [`crate::engine::EngineResolution`] for the
/// profile (names only — configs stay in merged metadata; Plan 6 decision 6).
///
/// Stamped by `DocumentProfileStage` from `resolve_engines_pass1`'s result
/// when a document resolves load-free at index time. Consumers (LSP
/// language→engine ownership, freeze key, engine pooling) read this instead
/// of re-running resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEngineResolution {
    /// Ordered, distinct engine owners (mirrors
    /// `EngineResolution::sequence`, names only).
    pub sequence: Vec<String>,
    /// Per-language ownership: language → owning engine name, insertion
    /// order (mirrors `EngineResolution::ownership`, a `LinkedHashMap` in
    /// the unreduced form).
    pub ownership: Vec<(String, String)>,
}

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

/// Per-document advertisement for listings consumers (`bd-n8a4`,
/// listings epic L0).
///
/// **Scoped feature surface — listings only.** No other Quarto
/// feature reads from this field. Non-listing consumers must use the
/// top-level [`DocumentProfile`] fields (`title`, `description`,
/// `image`, etc.). See the contract doc's §"Scoped feature surfaces."
///
/// # Authoring surface
///
/// Authors populate this struct via a top-level `listing-item:` key
/// in YAML frontmatter:
///
/// ```yaml
/// ---
/// title: My post
/// listing-item:
///   reading-time-minutes: 15      # author override; auto-fill skipped
///   extra:
///     status: "draft"             # custom field for a custom template
///     sponsors: [Foo, Bar]
/// ---
/// ```
///
/// Frontmatter keys are kebab-case (Quarto YAML convention); the
/// corresponding Rust fields are snake_case. Extraction maps between
/// the two with explicit lookups (e.g. `meta.get("reading-time-minutes")`
/// → `reading_time_minutes`).
///
/// # Generate / render decomposition
///
/// L0 (this version): the field exists; `DocumentProfile::extract`
/// reads it from frontmatter. Author-supplied values land here.
/// L1 (planned, `bd-izqh`) introduces `ListingItemInfoStage` to
/// auto-fill holes (description, image, word count, reading time,
/// date-modified) from the AST. Author values always win; the stage
/// only fills holes.
///
/// All fields are optional / collection-defaulted; an empty
/// `ListingItemInfo` is the legitimate default for documents that
/// don't participate in listings.
///
/// # `extra` and the open-shape exception
///
/// `extra` is the *only* open-shape field in `DocumentProfile`.
/// Adding a key to `extra` does **not** require a profile-version
/// bump: the outer struct shape is unchanged, and consumers (custom
/// listing templates) opt in to specific keys.
///
/// Reaching into `extra` from outside the listings code path is
/// forbidden by the contract doc. If a future feature finds itself
/// wanting to read from `extra`, that is a design-review trigger,
/// not a code-completion shortcut.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ListingItemInfo {
    /// Override for the title displayed in listings. Defaults to
    /// `profile.title` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Override for the subtitle displayed in listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,

    /// Listing description (text shown under the title). L0 honors
    /// an author-supplied value; L1's `ListingItemInfoStage` will
    /// fill from the first plain-text paragraph of the post-include
    /// AST when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Listing image src. L0 honors an author-supplied value; L1
    /// will fill from the first body `Image` node when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Alt text for the listing image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_alt: Option<String>,

    /// Listing date (publication / display date).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    /// Date the document was last modified. L1 will fill from
    /// filesystem mtime when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<String>,

    /// Listing categories, flattened to plain strings for the
    /// primary consumer surface. The tagged form (with `!prefer` /
    /// `!concat` merge tags preserved) lives in
    /// [`Self::categories_raw`] for tag-aware merging via
    /// [`quarto_config::MergedConfig`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,

    /// Tagged form of the `listing-item.categories:` value as
    /// written by the author. Listings consumers feeding this into
    /// `MergedConfig` alongside top-level
    /// [`DocumentProfile::categories_raw`] get tag-aware merging:
    /// default array semantics is `Concat`, and an author can write
    /// `categories: !prefer [a, b]` for override semantics. See L0
    /// sub-plan §"D7" and the contract doc's §"Scoped feature
    /// surfaces."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories_raw: Option<ConfigValue>,

    /// Estimated reading time, in minutes. L1 will fill from
    /// word-count divided by a 200 wpm constant when unset.
    /// Display formatting (e.g. "15 minutes") is a render-time
    /// concern in the listing template (L3+); this profile field
    /// is the semantic source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_time_minutes: Option<u32>,

    /// Word count of the document body. L1 will fill from a
    /// tokenized scan of the post-include AST when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u32>,

    /// Free-form fields advertised for custom listing templates.
    /// Author-declared in `listing-item.extra:`. Outer profile shape
    /// does **not** change when keys are added/removed, so no
    /// `profile_version` bump is required for `extra` mutations.
    /// `BTreeMap` over `HashMap` for deterministic serialization
    /// (see CLAUDE.md §"HashMap and Determinism").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, ConfigValue>,
}

impl ListingItemInfo {
    /// True when no author-supplied or auto-filled data is present.
    /// Used by [`DocumentProfile`]'s `serde(skip_serializing_if = …)`
    /// to keep on-disk profiles small for non-participating
    /// documents.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.subtitle.is_none()
            && self.description.is_none()
            && self.image.is_none()
            && self.image_alt.is_none()
            && self.date.is_none()
            && self.date_modified.is_none()
            && self.categories.is_empty()
            && self.categories_raw.is_none()
            && self.reading_time_minutes.is_none()
            && self.word_count.is_none()
            && self.extra.is_empty()
    }
}

/// One structured author in a [`DocumentProfile`] (v7,
/// bd-ez0hiowa). A serializable snapshot of the display-relevant
/// surface of [`crate::metadata::authors::Author`]; fields the
/// profile doesn't carry yet (roles, notes, funding) can join with a
/// `profile_version` bump.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileAuthor {
    /// Display name ("Norah Jones").
    pub name: String,
    /// Given name(s), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    /// Family name, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// ORCID identifier (bare id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
    /// Email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Home page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Academic titles displayed after the name ("PhD").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degrees: Vec<String>,
    /// Attribute flags that are true for this author
    /// (`corresponding`, `equal-contributor`, `deceased`, custom).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<String>,
    /// Denormalized affiliations, in reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affiliations: Vec<ProfileAffiliation>,
}

/// One affiliation attached to a [`ProfileAuthor`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileAffiliation {
    /// Institution name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Department.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    /// Home page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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

    /// Authors, flat list of plain-text names (the `literal` of each
    /// [`ProfileAuthor`] in `authors_structured`; kept for consumers
    /// that only need display names).
    pub authors: Vec<String>,

    /// Structured author model: name components, ORCID, email, url,
    /// degrees, attribute flags, and denormalized affiliations.
    /// Derived by the same normalization the title block renders from
    /// (`metadata::authors::parse_authors_model`). Additive in v7
    /// (title-block parity epic P2, bd-ez0hiowa).
    #[serde(default)]
    pub authors_structured: Vec<ProfileAuthor>,

    pub date: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub image: Option<String>,
    pub draft: bool,

    /// `aliases:` front-matter entries — old URLs that should redirect
    /// to this page — exactly as the author wrote them.
    ///
    /// Kept raw. An alias is resolved against this profile's own
    /// [`output_href`](Self::output_href), which needs no filesystem
    /// and no other document; but deciding whether it is *legal*
    /// needs every other page in the project. Both therefore happen
    /// in `project::website_post_render`, which is additionally the
    /// only place a diagnostic survives profile caching — a collision
    /// reported at extraction time would appear on the render that
    /// populated the cache and never again.
    ///
    /// Consumed only by website projects; other project types warn
    /// that the key is inert rather than dropping it silently.
    ///
    /// Default empty; added v9 → v10.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,

    /// Provenance for [`Self::aliases`], same order and length — the
    /// YAML scalar each alias came from.
    ///
    /// Collision diagnostics index into this with the position of the
    /// offending alias, so the two vectors are index-aligned by
    /// contract. Carried for the same reason as
    /// [`Self::resource_glob_sources`]: an alias inherited from a
    /// `_metadata.yml` and shared by many pages is the difference
    /// between a useful error and a puzzle.
    ///
    /// Default empty; added v9 → v10.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_sources: Vec<quarto_source_map::SourceInfo>,

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

    /// Document-level `resources:` patterns from YAML frontmatter
    /// (`bd-o8pr`). Raw patterns plus the YAML source location each
    /// one came from (`bd-c1et2`); expansion happens at the post-
    /// render collector. This is the *snapshot* of what the author
    /// declared at frontmatter-freeze time — engines and Lua
    /// filters that run later contribute through a separate channel
    /// (`DocumentResourceReport`) and cannot retroactively shrink
    /// this list. See plan §"Resolved design principles".
    ///
    /// Default empty; serializer omits empty lists. Bump from v2 →
    /// v3. Shape changed in v6 (`bd-c1et2`) from `Vec<String>` to
    /// `Vec<RawResourcePattern>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<crate::project_resources::RawResourcePattern>,

    /// The same `resources:` declarations, resolved to
    /// project-relative patterns against the directory of the file
    /// each was **written in** (bd-mt7a6uc4).
    ///
    /// This is what the post-render collector expands. It exists
    /// separately from [`Self::resources`] because the two answer
    /// different questions: this one is "which files does this
    /// document publish", while the raw list is the frontmatter
    /// snapshot `ResourceReportStage` diffs against post-filter
    /// metadata to catch filter-added entries.
    ///
    /// Populated by `DocumentProfileStage`, which is where the
    /// document's `SourceContext` (and therefore each pattern's
    /// declaring file) is in scope. Default empty; added v8 → v9.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_globs: Vec<crate::glob::GlobPattern>,

    /// Provenance for [`Self::resource_globs`], same order and
    /// length — the YAML scalar each resolved pattern came from.
    ///
    /// Carried so the post-render collector's diagnostics (`Q-5-16`)
    /// can point at what the author wrote. Without it the pattern
    /// would be named but not located, which for a `_metadata.yml`
    /// declaration inherited by many pages is the difference between
    /// a useful warning and a puzzle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_glob_sources: Vec<quarto_source_map::SourceInfo>,

    /// Declared `resources:` patterns resolution could not use — one
    /// that climbs above the project root, or that the glob engine
    /// rejects (bd-mt7a6uc4).
    ///
    /// These travel with the profile rather than being reported at
    /// profile-extraction time because **profiles are cached**: a
    /// diagnostic emitted in the stage would appear on the render
    /// that populated the cache and never again. The post-render
    /// collector turns each of these into its `Q-5-1` / `Q-5-2`
    /// error on every render.
    ///
    /// Default empty; added v8 → v9.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_resources: Vec<crate::project_resources::RejectedResourcePattern>,

    /// Tagged form of the top-level `categories:` value as written
    /// by the author. Mirrors [`Self::categories`] but preserves
    /// `ConfigValue` merge tags (`!prefer` / `!concat`) for
    /// listings consumers that combine top-level and
    /// `listing-item.categories` via
    /// [`quarto_config::MergedConfig`]. Most consumers should keep
    /// reading the flattened [`Self::categories`]; only listings
    /// reach for this raw form.
    ///
    /// Default `None`; serializer omits when absent.
    /// Added v3 → v4 (`bd-n8a4`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories_raw: Option<ConfigValue>,

    /// Per-document advertisement for listings that include this
    /// document. **Scoped feature surface — listings consumers only.**
    /// Non-listing consumers must use the corresponding top-level
    /// fields ([`Self::title`], [`Self::description`],
    /// [`Self::image`], …). See [`ListingItemInfo`] and the contract
    /// doc's §"Scoped feature surfaces."
    ///
    /// L0 (`bd-n8a4`) reads author-supplied values from
    /// `meta.listing-item:` at extraction time. L1's planned
    /// `ListingItemInfoStage` (`bd-izqh`) will fill holes
    /// (description, image, word count, reading time, date-modified)
    /// before the checkpoint.
    ///
    /// Default empty; serializer omits empty.
    /// Added v3 → v4 (`bd-n8a4`).
    #[serde(default, skip_serializing_if = "ListingItemInfo::is_empty")]
    pub listing_item: ListingItemInfo,

    /// Glob patterns from the host's `listing.*.contents:` config,
    /// flattened across all listings declared on the page. Each
    /// entry is a **project-relative, base-resolved** pattern (e.g.
    /// `"sub/*.qmd"`) plus a negation flag — resolved by
    /// [`crate::project::listing::glob_resolve::resolve_content_globs`]
    /// against the directory of the file the glob was written in
    /// (front matter → host dir, `_metadata.yml` → its dir,
    /// `_quarto.yml` → project root; GH #456, bd-v7ixzsp5).
    /// Populated by `DocumentProfileStage` (the resolution needs the
    /// document's `SourceContext`, which `extract` doesn't take);
    /// [`DocumentProfile::extract`] leaves it empty. Patterns whose
    /// normalization escapes the project root are dropped here (the
    /// render transform owns the `Q-12-17` diagnostic).
    ///
    /// *Expansion* against the project's file set is still **not**
    /// cached on the profile because it depends on the full project
    /// source set, which a per-doc profile cannot represent safely
    /// (a new sibling `.qmd` added to the project would not
    /// invalidate the host's profile cache, leaving the expansion
    /// stale). The dependency-graph builder
    /// ([`crate::project::dependency_graph::ProjectDependencyGraph::build`])
    /// matches these patterns at graph-build time against
    /// [`crate::project::index::ProjectIndex::profiles`] (single
    /// view, via [`crate::glob::PatternSet`] — the same rule the
    /// L3 generate transform uses at render time) and produces
    /// forward edges from each host to each match. Listing hosts
    /// with non-empty entries are also added to
    /// [`crate::project::dependency_graph::ProjectDependencyGraph::force_render`]
    /// so Mode B pulls them in when any of their content files is
    /// in the user-named target set.
    ///
    /// Default empty; serializer omits empty lists.
    /// Added v4 → v5 (`bd-xbnf`); shape changed v7 → v8
    /// (`bd-v7ixzsp5`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listing_content_globs: Vec<crate::glob::GlobPattern>,

    /// Pass-1-resolved engine sequence/ownership (names only), stamped by
    /// `DocumentProfileStage` from `resolve_engines_pass1`. `None` means the
    /// document fell through to Pass-2's loading resolver — advisory, not
    /// an error. See [`ProfileEngineResolution`] and Plan 6 decision 6.
    ///
    /// Added v12 (Plan 6 Phase 5).
    #[serde(default)]
    pub engine_resolution: Option<ProfileEngineResolution>,
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
            authors_structured: Vec::new(),
            date: None,
            categories: Vec::new(),
            keywords: Vec::new(),
            image: None,
            draft: false,
            aliases: Vec::new(),
            alias_sources: Vec::new(),
            order: None,
            outline: Vec::new(),
            includes: Vec::new(),
            nav_dependencies: Vec::new(),
            always_render: false,
            body_link_targets: Vec::new(),
            resources: Vec::new(),
            // Resolution needs the document's SourceContext, so
            // `DocumentProfileStage` fills these in; `extract` stays pure.
            resource_globs: Vec::new(),
            resource_glob_sources: Vec::new(),
            rejected_resources: Vec::new(),
            categories_raw: None,
            listing_item: ListingItemInfo::default(),
            listing_content_globs: Vec::new(),
            engine_resolution: None,
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
        let authors_structured = extract_structured_authors(meta);
        let aliases = extract_string_list_with_sources(meta, "aliases");

        Self {
            profile_version: DOCUMENT_PROFILE_VERSION,
            source_path: source_path.to_path_buf(),
            output_href: output_href.to_string(),
            format_id: format_id.to_string(),
            title: plain_text_field(meta, "title"),
            subtitle: plain_text_field(meta, "subtitle"),
            description: plain_text_field(meta, "description"),
            authors: authors_structured.iter().map(|a| a.name.clone()).collect(),
            authors_structured,
            date: plain_text_field(meta, "date"),
            categories: extract_string_list(meta, "categories"),
            keywords: extract_string_list(meta, "keywords"),
            image: plain_text_field(meta, "image"),
            draft: meta.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
            aliases: aliases.iter().map(|(s, _)| s.clone()).collect(),
            alias_sources: aliases.into_iter().map(|(_, info)| info).collect(),
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
            resources: crate::project_resources::extract_resource_patterns(meta, &["resources"]),
            // Resolution needs the document's SourceContext, so
            // `DocumentProfileStage` fills these in; `extract` stays pure.
            resource_globs: Vec::new(),
            resource_glob_sources: Vec::new(),
            rejected_resources: Vec::new(),
            // L0 (`bd-n8a4`): both fields wired into extract below.
            // Skeleton stage left explicit so TDD failure points at
            // the wiring, not the field declarations.
            categories_raw: extract_categories_raw(meta),
            listing_item: extract_listing_item(meta),
            // L6 (`bd-xbnf`) / v8 (`bd-v7ixzsp5`): populated by
            // `DocumentProfileStage`, which resolves each glob's
            // base directory from its `SourceInfo` provenance — a
            // lookup that needs the document's `SourceContext`,
            // which this pure extractor doesn't take. See the field
            // doc.
            listing_content_globs: Vec::new(),
            // Stamped by `DocumentProfileStage` from `resolve_engines_pass1`
            // (needs the registry + AST, not available to this pure
            // metadata-only extractor) — mirrors the `includes` field.
            engine_resolution: None,
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

/// Capture the originating `ConfigValue` for the top-level
/// `categories:` key — preserving any `!prefer` / `!concat` tags so a
/// listings consumer can feed it (alongside
/// `listing_item.categories_raw`) into [`MergedConfig`] for tag-aware
/// merging.
///
/// Returns `None` if the key is absent. The flattened
/// [`DocumentProfile::categories`] field is still produced from
/// `extract_string_list` for the primary consumer surface.
///
/// [`MergedConfig`]: quarto_config::MergedConfig
fn extract_categories_raw(meta: &ConfigValue) -> Option<ConfigValue> {
    meta.get("categories").cloned()
}

/// Walk the `meta.listing-item:` map and produce a [`ListingItemInfo`]
/// from author-supplied values. Unknown keys at the top level of
/// `listing-item:` are dropped silently; type mismatches at known
/// keys leave the field at its default. Strict diagnostics are L2's
/// job (see L0 sub-plan §"C5").
fn extract_listing_item(meta: &ConfigValue) -> ListingItemInfo {
    let Some(li) = meta.get("listing-item") else {
        return ListingItemInfo::default();
    };

    ListingItemInfo {
        title: plain_text_field(li, "title"),
        subtitle: plain_text_field(li, "subtitle"),
        description: plain_text_field(li, "description"),
        image: plain_text_field(li, "image"),
        image_alt: plain_text_field(li, "image-alt"),
        date: plain_text_field(li, "date"),
        date_modified: plain_text_field(li, "date-modified"),
        categories: extract_string_list(li, "categories"),
        categories_raw: li.get("categories").cloned(),
        reading_time_minutes: extract_u32_field(li, "reading-time-minutes"),
        word_count: extract_u32_field(li, "word-count"),
        extra: extract_listing_item_extra(li),
    }
}

/// Read a non-negative integer field as `u32`. Returns `None` if the
/// key is absent, the value isn't an integer, or the integer doesn't
/// fit in `u32` (negative or oversized). Matches the graceful-drop
/// pattern used elsewhere in this module.
fn extract_u32_field(meta: &ConfigValue, key: &str) -> Option<u32> {
    meta.get(key)
        .and_then(|v| v.as_int())
        .and_then(|i| u32::try_from(i).ok())
}

/// Walk `listing-item.extra:` and return its key-value entries as a
/// `BTreeMap<String, ConfigValue>`. Returns an empty map if the key
/// is absent or the value isn't a map.
///
/// Each entry's value is preserved verbatim — no flattening, no
/// type coercion. Custom listing templates handle typed access at
/// render time via `quarto-doctemplate`'s `TemplateValue` conversion.
/// `BTreeMap` over `HashMap` for deterministic serialization (see
/// CLAUDE.md §"HashMap and Determinism").
fn extract_listing_item_extra(li: &ConfigValue) -> BTreeMap<String, ConfigValue> {
    let Some(extra) = li.get("extra") else {
        return BTreeMap::new();
    };
    let Some(entries) = extra.as_map_entries() else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for entry in entries {
        out.insert(entry.key.clone(), entry.value.clone());
    }
    out
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

/// [`extract_string_list`] that also keeps each entry's source span.
///
/// Same shape rules as its span-less sibling — a YAML sequence yields
/// one entry per item, a bare scalar yields a single entry — but each
/// string is paired with the [`SourceInfo`] of the `ConfigValue` it
/// came from, so a later diagnostic can point at what the author
/// actually wrote.
///
/// The parallel implementation in
/// [`crate::project_resources::extract_resource_patterns`] does the
/// same job for `resources:`; it stays separate because it builds a
/// domain type (`RawResourcePattern`) rather than a bare pair.
///
/// [`SourceInfo`]: quarto_source_map::SourceInfo
fn extract_string_list_with_sources(
    meta: &ConfigValue,
    key: &str,
) -> Vec<(String, quarto_source_map::SourceInfo)> {
    let Some(value) = meta.get(key) else {
        return Vec::new();
    };
    if let Some(arr) = value.as_array() {
        arr.iter()
            .filter_map(|v| v.as_plain_text().map(|s| (s, v.source_info.clone())))
            .collect()
    } else if let Some(s) = value.as_plain_text() {
        vec![(s, value.source_info.clone())]
    } else {
        Vec::new()
    }
}

/// Structured-author extraction (v7, bd-ez0hiowa): runs the shared
/// author normalization (`metadata::authors::parse_authors_model` —
/// the same pass the title block renders from) and snapshots its
/// display-relevant surface into serializable [`ProfileAuthor`]s.
fn extract_structured_authors(meta: &ConfigValue) -> Vec<ProfileAuthor> {
    let model = crate::metadata::authors::parse_authors_model(meta);
    model
        .authors
        .iter()
        .map(|a| ProfileAuthor {
            name: a.name.literal.clone(),
            given: a.name.given.clone(),
            family: a.name.family.clone(),
            orcid: a.orcid.clone(),
            email: a.email.clone(),
            url: a.url.clone(),
            degrees: a.degrees.clone(),
            attributes: a.attributes.clone(),
            affiliations: a
                .affiliations
                .iter()
                .filter_map(|r| model.affiliations.iter().find(|aff| &aff.id == r))
                .map(|aff| ProfileAffiliation {
                    name: aff.name.clone(),
                    department: aff.department.clone(),
                    url: aff.url.clone(),
                })
                .collect(),
        })
        .collect()
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

    /// Project a TOC entry title to plain text for assertions. The
    /// profile's outline carries inlines (bd-toc-smart-quotes-6nro57ed);
    /// consumers that want text project it themselves, and the
    /// plain-text writer is the shared way to do that.
    fn title_text(title: &quarto_pandoc_types::Inlines) -> String {
        pampa::writers::plaintext::inlines_to_string(title).0
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
        assert_eq!(title_text(&first.title), "Top");
        assert_eq!(entry_ids(&first.children), vec!["sub"]);
        assert_eq!(first.children[0].level, 2);
        assert_eq!(entry_ids(&first.children[0].children), vec!["deep"]);
        assert_eq!(title_text(&profile.outline[1].title), "Top two");
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
    fn profile_extract_structured_authors() {
        // v7 (bd-ez0hiowa): the structured author model surfaces in
        // the profile, and the flat `authors` list carries the same
        // display literals.
        let qmd = "\
---
title: Structured
author:
  - name: Norah Jones
    orcid: 0000-0002-1825-0097
    email: norah@example.com
    url: https://example.com/norah
    corresponding: true
    degrees: [PhD]
    affiliations:
      - name: Carnegie Mellon University
        department: School of Music
  - name: Bill Malone
---

Body.
";
        let ast = parse_qmd(qmd);
        let profile = DocumentProfile::extract(&ast, Path::new("s.qmd"), "s.html", "html");

        assert_eq!(profile.authors, vec!["Norah Jones", "Bill Malone"]);
        assert_eq!(profile.authors_structured.len(), 2);
        let norah = &profile.authors_structured[0];
        assert_eq!(norah.name, "Norah Jones");
        assert_eq!(norah.given.as_deref(), Some("Norah"));
        assert_eq!(norah.family.as_deref(), Some("Jones"));
        assert_eq!(norah.orcid.as_deref(), Some("0000-0002-1825-0097"));
        assert_eq!(norah.email.as_deref(), Some("norah@example.com"));
        assert_eq!(norah.url.as_deref(), Some("https://example.com/norah"));
        assert_eq!(norah.degrees, vec!["PhD"]);
        assert_eq!(norah.attributes, vec!["corresponding"]);
        assert_eq!(norah.affiliations.len(), 1);
        assert_eq!(
            norah.affiliations[0].name.as_deref(),
            Some("Carnegie Mellon University")
        );
        assert_eq!(
            norah.affiliations[0].department.as_deref(),
            Some("School of Music")
        );

        // Round-trips through JSON at the current version.
        let json = profile.to_json().unwrap();
        let back = DocumentProfile::from_json(&json).unwrap();
        assert_eq!(back.authors_structured, profile.authors_structured);
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
    fn profile_extract_aliases_in_declaration_order() {
        // Order is load-bearing: the post-render pass reports the
        // *first* declaration of a colliding alias as the primary and
        // the rest as secondaries, so a reshuffle would move the span
        // the diagnostic points at.
        let ast = parse_qmd(
            "---\ntitle: Moved\naliases:\n  - /old.html\n  - ../previous/index.html\n---\n\nBody.\n",
        );
        let profile = DocumentProfile::extract(&ast, Path::new("m.qmd"), "m.html", "html");
        assert_eq!(profile.aliases, vec!["/old.html", "../previous/index.html"]);
    }

    #[test]
    fn profile_extract_aliases_absent_is_empty() {
        let ast = parse_qmd("---\ntitle: No aliases\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("x.qmd"), "x.html", "html");
        assert!(profile.aliases.is_empty());
        assert!(profile.alias_sources.is_empty());
    }

    #[test]
    fn profile_extract_aliases_accepts_single_scalar() {
        // Q1 requires a YAML list and silently ignores a bare scalar.
        // Accepting both matches how `resources:` already behaves here,
        // and a dropped redirect is exactly the silent failure this
        // feature exists to prevent.
        let ast = parse_qmd("---\ntitle: One\naliases: /old.html\n---\n\nBody.\n");
        let profile = DocumentProfile::extract(&ast, Path::new("o.qmd"), "o.html", "html");
        assert_eq!(profile.aliases, vec!["/old.html"]);
        assert_eq!(profile.alias_sources.len(), 1);
    }

    #[test]
    fn profile_extract_alias_sources_parallel_to_aliases() {
        // The two vectors are index-aligned by contract — collision
        // diagnostics index into `alias_sources` with the position of
        // the offending alias.
        let ast = parse_qmd(
            "---\ntitle: Moved\naliases:\n  - /a.html\n  - /b.html\n  - /c.html\n---\n\nBody.\n",
        );
        let profile = DocumentProfile::extract(&ast, Path::new("m.qmd"), "m.html", "html");
        assert_eq!(profile.aliases.len(), 3);
        assert_eq!(
            profile.alias_sources.len(),
            profile.aliases.len(),
            "alias_sources must stay index-aligned with aliases"
        );
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
        // `body_link_targets` fields. After subsequent version bumps
        // (v2 for Phase-8, v3 for bd-o8pr resources), those entries
        // become invalid and the cache silently regenerates them.
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
                assert_eq!(expected, DOCUMENT_PROFILE_VERSION);
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

    // === L0 — listings epic (`bd-n8a4`) ===============================
    //
    // Tests #1–#13 from the L0 sub-plan, plus the C6 namespace-distinct
    // test (#9b), plus three D7 categories_raw tests. See
    // `claude-notes/plans/2026-05-05-listings-L0-profile-extension.md`.

    use quarto_pandoc_types::{ConfigValue, ConfigValueKind, MergeOp};
    use quarto_source_map::SourceInfo;

    fn make_string_config_value(s: &str) -> ConfigValue {
        ConfigValue::new_string(s.to_string(), SourceInfo::for_test())
    }

    #[test]
    fn listing_item_info_default_is_empty() {
        assert!(ListingItemInfo::default().is_empty());
    }

    /// Setting any one curated field flips `is_empty()` to false. The
    /// `skip_serializing_if` guard rests on this property — if any
    /// field stops being checked, the on-disk profile silently grows.
    #[test]
    // Each block sets exactly one field on a default value to prove that field
    // alone defeats `is_empty()`; the default-then-assign shape is the point.
    #[allow(clippy::field_reassign_with_default)]
    fn listing_item_info_partial_not_empty_per_field() {
        let mut li = ListingItemInfo::default();
        li.title = Some("X".into());
        assert!(!li.is_empty(), "title set");

        let mut li = ListingItemInfo::default();
        li.subtitle = Some("X".into());
        assert!(!li.is_empty(), "subtitle set");

        let mut li = ListingItemInfo::default();
        li.description = Some("X".into());
        assert!(!li.is_empty(), "description set");

        let mut li = ListingItemInfo::default();
        li.image = Some("X".into());
        assert!(!li.is_empty(), "image set");

        let mut li = ListingItemInfo::default();
        li.image_alt = Some("X".into());
        assert!(!li.is_empty(), "image_alt set");

        let mut li = ListingItemInfo::default();
        li.date = Some("2026-01-01".into());
        assert!(!li.is_empty(), "date set");

        let mut li = ListingItemInfo::default();
        li.date_modified = Some("2026-01-02".into());
        assert!(!li.is_empty(), "date_modified set");

        let mut li = ListingItemInfo::default();
        li.categories = vec!["a".into()];
        assert!(!li.is_empty(), "categories set");

        let mut li = ListingItemInfo::default();
        li.categories_raw = Some(make_string_config_value("a"));
        assert!(!li.is_empty(), "categories_raw set");

        let mut li = ListingItemInfo::default();
        li.reading_time_minutes = Some(5);
        assert!(!li.is_empty(), "reading_time_minutes set");

        let mut li = ListingItemInfo::default();
        li.word_count = Some(100);
        assert!(!li.is_empty(), "word_count set");

        let mut li = ListingItemInfo::default();
        li.extra
            .insert("status".into(), make_string_config_value("draft"));
        assert!(!li.is_empty(), "extra entry set");
    }

    #[test]
    fn listing_item_info_serde_roundtrip_empty() {
        let li = ListingItemInfo::default();
        let json = serde_json::to_string(&li).expect("serialize");
        let restored: ListingItemInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(li, restored);
    }

    #[test]
    fn listing_item_info_serde_omits_empty_fields() {
        let li = ListingItemInfo {
            title: Some("Hello".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&li).expect("serialize");
        // Only `title` should appear; every other field is empty/None
        // and tagged `skip_serializing_if`.
        assert!(json.contains("\"title\":\"Hello\""), "title present");
        assert!(!json.contains("subtitle"));
        assert!(!json.contains("description"));
        assert!(!json.contains("image"));
        assert!(!json.contains("image_alt"));
        assert!(!json.contains("\"date\":"));
        assert!(!json.contains("date_modified"));
        assert!(!json.contains("categories"));
        assert!(!json.contains("reading_time_minutes"));
        assert!(!json.contains("word_count"));
        assert!(!json.contains("extra"));
    }

    #[test]
    fn listing_item_info_extra_roundtrip() {
        let mut extra = BTreeMap::new();
        extra.insert("status".into(), make_string_config_value("draft"));
        extra.insert(
            "sponsors".into(),
            ConfigValue {
                value: ConfigValueKind::Array(vec![
                    make_string_config_value("Foo"),
                    make_string_config_value("Bar"),
                ]),
                source_info: SourceInfo::for_test(),
                merge_op: MergeOp::Concat,
            },
        );
        let li = ListingItemInfo {
            extra,
            ..Default::default()
        };
        let json = serde_json::to_string(&li).expect("serialize");
        let restored: ListingItemInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(li, restored);
        // Spot-check the structural shape of `extra` after round-trip.
        assert_eq!(restored.extra.len(), 2);
        assert!(restored.extra.contains_key("status"));
        assert!(restored.extra.contains_key("sponsors"));
        // BTreeMap iteration order is alphabetical; verify
        // determinism via key order.
        let keys: Vec<&String> = restored.extra.keys().collect();
        assert_eq!(keys, vec![&"sponsors".to_string(), &"status".to_string()]);
    }

    #[test]
    fn profile_default_listing_item_is_empty() {
        let p = DocumentProfile::default();
        assert!(p.listing_item.is_empty());
        let json = p.to_json().expect("serialize");
        // `listing_item` is `skip_serializing_if = ListingItemInfo::is_empty`,
        // so a default profile must not emit any listing-item key.
        assert!(
            !json.contains("listing_item"),
            "default profile must omit listing_item field; got JSON: {json}"
        );
    }

    #[test]
    fn profile_extract_no_listing_item_key() {
        let ast = parse_qmd("---\ntitle: No listing\n---\n\nBody.\n");
        let p = DocumentProfile::extract(&ast, Path::new("a.qmd"), "a.html", "html");
        assert!(p.listing_item.is_empty());
    }

    #[test]
    fn profile_extract_listing_item_curated_fields() {
        let qmd = "\
---
title: Outer
listing-item:
  title: Listing title
  subtitle: Listing subtitle
  description: Listing desc
  image: cover.png
  image-alt: A cover
  date: 2026-04-01
  date-modified: 2026-04-15
  reading-time-minutes: 15
  word-count: 1234
  categories: [a, b]
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("li.qmd"), "li.html", "html");
        let li = &p.listing_item;
        assert_eq!(li.title.as_deref(), Some("Listing title"));
        assert_eq!(li.subtitle.as_deref(), Some("Listing subtitle"));
        assert_eq!(li.description.as_deref(), Some("Listing desc"));
        assert_eq!(li.image.as_deref(), Some("cover.png"));
        assert_eq!(li.image_alt.as_deref(), Some("A cover"));
        assert_eq!(li.date.as_deref(), Some("2026-04-01"));
        assert_eq!(li.date_modified.as_deref(), Some("2026-04-15"));
        assert_eq!(li.reading_time_minutes, Some(15));
        assert_eq!(li.word_count, Some(1234));
        assert_eq!(li.categories, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn profile_extract_listing_item_extra_passthrough() {
        let qmd = "\
---
title: Has extras
listing-item:
  extra:
    status: draft
    sponsors: [Foo, Bar]
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("e.qmd"), "e.html", "html");
        let extra = &p.listing_item.extra;
        assert_eq!(extra.len(), 2, "two extras expected");
        let status = extra.get("status").expect("status entry present");
        assert_eq!(status.as_plain_text().as_deref(), Some("draft"));
        let sponsors = extra.get("sponsors").expect("sponsors entry present");
        let arr = sponsors.as_array().expect("sponsors is an array");
        let names: Vec<String> = arr.iter().filter_map(|v| v.as_plain_text()).collect();
        assert_eq!(names, vec!["Foo".to_string(), "Bar".to_string()]);
    }

    /// C6: curated fields and `extra` keys live in distinct namespaces.
    /// `listing-item.title` populates `listing_item.title`; an entry
    /// also named `title` inside `listing-item.extra` populates
    /// `listing_item.extra["title"]` — they do not collide.
    #[test]
    fn profile_extract_listing_item_extra_namespace_distinct() {
        let qmd = "\
---
title: Outer
listing-item:
  title: Curated
  extra:
    title: Custom
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("c.qmd"), "c.html", "html");
        assert_eq!(p.listing_item.title.as_deref(), Some("Curated"));
        let extra_title = p
            .listing_item
            .extra
            .get("title")
            .expect("extra.title present");
        assert_eq!(extra_title.as_plain_text().as_deref(), Some("Custom"));
    }

    #[test]
    fn profile_extract_listing_item_unknown_top_key_dropped() {
        // Per L0 §"Diagnostics" / C5: unknown keys at the top level
        // of `listing-item:` are silently dropped. Strict validation
        // is L2's job.
        let qmd = "\
---
title: T
listing-item:
  not-a-known-field: 42
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("u.qmd"), "u.html", "html");
        assert!(p.listing_item.is_empty());
    }

    #[test]
    fn profile_extract_listing_item_type_mismatch_dropped() {
        // Per L0 §"Diagnostics" / C5: type mismatches at known keys
        // leave the field at default rather than panic.
        let qmd = "\
---
title: T
listing-item:
  reading-time-minutes: [bad, type]
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("tm.qmd"), "tm.html", "html");
        assert_eq!(p.listing_item.reading_time_minutes, None);
    }

    #[test]
    fn document_profile_version_is_12() {
        assert_eq!(DOCUMENT_PROFILE_VERSION, 12);
    }

    /// A v3 profile (the pre-listings shape) must be rejected by
    /// `from_json`. The cache layer treats this as a regenerate signal.
    #[test]
    fn profile_v3_json_rejected_with_version_mismatch() {
        let payload = r#"{
            "profile_version": 3,
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
                assert_eq!(expected, DOCUMENT_PROFILE_VERSION);
                assert_eq!(found, 3);
            }
            other => panic!("expected VersionMismatch from v3 payload, got {:?}", other),
        }
    }

    // --- D7: tag-preserving `categories_raw` ----------------------------

    /// When the author writes a top-level `categories:` key, the raw
    /// `ConfigValue` is preserved on `profile.categories_raw` so a
    /// listings consumer can feed it through `MergedConfig` for
    /// tag-aware merging. The flattened `Vec<String>` form on
    /// `profile.categories` is unchanged.
    #[test]
    fn profile_categories_raw_present_when_frontmatter_has_categories() {
        let ast = parse_qmd("---\ntitle: T\ncategories: [a, b]\n---\n\nBody.\n");
        let p = DocumentProfile::extract(&ast, Path::new("cr.qmd"), "cr.html", "html");
        // Flattened form preserved.
        assert_eq!(
            p.categories,
            vec!["a".to_string(), "b".to_string()],
            "flattened categories preserved"
        );
        // Tagged form preserved.
        let raw = p
            .categories_raw
            .as_ref()
            .expect("categories_raw populated when frontmatter has categories");
        let arr = raw.as_array().expect("categories raw is an array");
        let names: Vec<String> = arr.iter().filter_map(|v| v.as_plain_text()).collect();
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn profile_categories_raw_absent_when_frontmatter_has_none() {
        let ast = parse_qmd("---\ntitle: T\n---\n\nBody.\n");
        let p = DocumentProfile::extract(&ast, Path::new("nc.qmd"), "nc.html", "html");
        assert!(p.categories.is_empty());
        assert!(p.categories_raw.is_none());
    }

    /// Mirror of the top-level test: when the author writes
    /// `listing-item.categories:`, the tagged `ConfigValue` is
    /// preserved so a listings consumer can merge it with
    /// `profile.categories_raw` via `MergedConfig`.
    #[test]
    fn profile_listing_item_categories_raw_preserved() {
        let qmd = "\
---
title: T
listing-item:
  categories: [x, y]
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("lc.qmd"), "lc.html", "html");
        assert_eq!(
            p.listing_item.categories,
            vec!["x".to_string(), "y".to_string()],
            "flattened listing_item.categories preserved"
        );
        let raw = p
            .listing_item
            .categories_raw
            .as_ref()
            .expect("listing_item.categories_raw populated");
        let arr = raw.as_array().expect("raw is an array");
        let names: Vec<String> = arr.iter().filter_map(|v| v.as_plain_text()).collect();
        assert_eq!(names, vec!["x".to_string(), "y".to_string()]);
    }

    // ─────────────────────────────────────────────────────────────────
    // L6 (`bd-xbnf`): `listing_content_globs` field + v4 → v5 bump
    //
    // The dep-graph builder reads this field at graph-build time to
    // pull in listing hosts whenever any of their content files is
    // in the user-named target set (Mode B). Versioning bumps so
    // stale Phase-8 caches invalidate cleanly.
    // ─────────────────────────────────────────────────────────────────

    /// Test #9 — default profile has empty `listing_content_globs`.
    /// Default-empty + `skip_serializing_if` keeps the on-disk
    /// profile shape unchanged for documents with no listings.
    #[test]
    fn profile_v5_default_has_empty_listing_content_globs() {
        let p = DocumentProfile::default();
        assert!(p.listing_content_globs.is_empty());
    }

    /// Test #10 — `extract` leaves the field empty even when the
    /// frontmatter declares a listing: since v8 (bd-v7ixzsp5) the
    /// globs are resolved (base-directory + negation) by
    /// `DocumentProfileStage`, which has the `SourceContext` the
    /// pure extractor lacks. The stage-level population is covered
    /// by `document_profile` stage tests and the
    /// `listing_glob_resolution` integration suite.
    #[test]
    fn profile_extract_leaves_listing_content_globs_to_the_stage() {
        let qmd = "\
---
title: Host
listing:
  contents: posts/*.qmd
---

Body.
";
        let ast = parse_qmd(qmd);
        let p = DocumentProfile::extract(&ast, Path::new("idx.qmd"), "idx.html", "html");
        assert!(
            p.listing_content_globs.is_empty(),
            "resolution happens in DocumentProfileStage, not extract"
        );
    }

    /// Test #11 — a v4 profile (the pre-L6 shape) must be rejected
    /// by `from_json` with a clean error so the cache layer
    /// regenerates rather than silently reading a stale profile.
    #[test]
    fn profile_v4_json_rejected_with_clean_error() {
        let payload = r#"{
            "profile_version": 4,
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
                assert_eq!(expected, DOCUMENT_PROFILE_VERSION);
                assert_eq!(found, 4);
            }
            other => panic!("expected VersionMismatch from v4 payload, got {:?}", other),
        }
    }

    /// Test #12 — round-trip serialization preserves a non-empty
    /// `listing_content_globs`.
    #[test]
    #[allow(clippy::field_reassign_with_default)] // default-then-set keeps the test readable
    fn profile_v5_listing_content_globs_round_trip() {
        use crate::glob::GlobPattern;
        let mut p = DocumentProfile::default();
        p.source_path = PathBuf::from("idx.qmd");
        p.listing_content_globs = vec![
            GlobPattern::positive("a/*.qmd"),
            GlobPattern::negated("a/wip.qmd"),
        ];

        let json = p.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(restored.listing_content_globs, p.listing_content_globs);
    }

    /// Test #13 — `to_json` of a default profile omits the empty
    /// `listing_content_globs` field. Keeps the v4 / v5 on-disk
    /// shape identical for documents without listings.
    #[test]
    fn profile_v5_round_trip_empty_omits_field() {
        let p = DocumentProfile::default();
        let json = p.to_json().expect("serialize");
        assert!(
            !json.contains("listing_content_globs"),
            "empty listing_content_globs should be omitted from JSON; got: {json}"
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Plan 6 Phase 5: `engine_resolution` field + v6 → v7 bump.
    // ─────────────────────────────────────────────────────────────────

    /// A default profile (nothing stamped yet) carries `None` and survives
    /// a round-trip.
    #[test]
    fn profile_engine_resolution_default_is_none() {
        let p = DocumentProfile::default();
        assert_eq!(p.engine_resolution, None);
        let json = p.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(restored.engine_resolution, None);
    }

    /// A stamped `Some(ProfileEngineResolution)` survives serialize →
    /// deserialize intact (sequence + ownership order preserved).
    #[test]
    fn profile_engine_resolution_round_trip_some() {
        let p = DocumentProfile {
            engine_resolution: Some(ProfileEngineResolution {
                sequence: vec!["knitr".to_string()],
                ownership: vec![
                    ("r".to_string(), "knitr".to_string()),
                    ("python".to_string(), "knitr".to_string()),
                ],
            }),
            ..Default::default()
        };

        let json = p.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(p, restored);
        assert_eq!(
            restored.engine_resolution,
            Some(ProfileEngineResolution {
                sequence: vec!["knitr".to_string()],
                ownership: vec![
                    ("r".to_string(), "knitr".to_string()),
                    ("python".to_string(), "knitr".to_string()),
                ],
            })
        );
    }

    /// A fallen-through document's `None` also survives the round-trip
    /// (distinct from "field absent" — `None` is a legitimate, advisory
    /// value, not an error; see the field doc comment).
    #[test]
    fn profile_engine_resolution_round_trip_none() {
        let p = DocumentProfile {
            engine_resolution: None,
            ..Default::default()
        };
        let json = p.to_json().expect("serialize");
        let restored = DocumentProfile::from_json(&json).expect("deserialize");
        assert_eq!(restored.engine_resolution, None);
    }

    /// A v6 profile (pre-Phase-5 shape) must be rejected by `from_json`
    /// with a clean version-mismatch error, not a silent default-None
    /// read — the cache layer regenerates rather than trusting an old
    /// shape's missing field.
    #[test]
    fn profile_v6_json_rejected_with_clean_error() {
        let payload = r#"{
            "profile_version": 6,
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
                assert_eq!(expected, DOCUMENT_PROFILE_VERSION);
                assert_eq!(found, 6);
            }
            other => panic!("expected VersionMismatch from v6 payload, got {:?}", other),
        }
    }
}
