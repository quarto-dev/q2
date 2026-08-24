# Listing inline `contents:` records — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A listing whose `contents:` entries are inline metadata records renders one item per record (Q1 parity), including records that overlay a project document via `path:`, with rich diagnostics instead of the current silent empty listing.

**Architecture:** `ListingItem` stops assuming a document behind every item: its link becomes an explicit `ItemTarget { Document | Href | None }` plus an `ItemOrigin` marker. One map→`ListingItemInfo` parser (`ListingItemInfo::from_map`) serves both front-matter `listing-item:` and inline records, the record variant routing unrecognized keys into `extra`. The generate transform gains a second item source beside glob matching; records keep their declared position; record `path:` values ride the same provenance-based base-directory resolver as globs, and feed the dependency graph.

**Tech Stack:** Rust (`quarto-core`), `quarto-doctemplate` built-in templates, `quarto-error-catalog` + `docs/errors/` pages, nextest.

**Spec:** this file — §"Design decisions" and §"Investigation record" below. Strand: `bd-listing-inline-contents-tyy446ze` (p1 bug, parent epic bd-61cd). Follow-up already filed: `bd-hj1ehfn8` (YAML-file item source).

**Braid:** bd-listing-inline-contents-tyy446ze
**Worktree:** `.worktrees/workspace-5` (branch `braid/bd-listing-inline-contents-tyy446ze-listing-inline-contents`, based on `main` @ `596ceb572`)
**Status:** Plan written 2026-08-24 after design alignment with the user; **awaiting go-ahead to execute.**

## Global Constraints

- TDD: every behavioural task writes its failing test first and runs it before implementing (`CLAUDE.md` §"CRITICAL - TEST-DRIVEN DEVELOPMENT").
- Per-task gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p quarto-core`. Per-phase gate and before any push: `cargo nextest run --workspace` (~3 min; report the delta against the live baseline **13130 passed / 199 skipped** at `596ceb572`).
- `cargo xtask verify` **full** (not `--skip-hub-build`) before the final commit: `quarto-core` feeds the WASM leg.
- Pre-commit checklist: `claude-notes/instructions/review.md`. Plan-driven execution the user has approved → commit-and-continue at clean phase boundaries. **Never push without explicit approval.**
- Determinism: no `std::collections::HashMap` in anything serialized or iterated for output; `BTreeMap` for `extra` (already the convention).
- Every new error code gets its catalog entry, its `docs/errors/listing/Q-12-N.qmd` page, and its `docs/_quarto.yml` sidebar entry (ascending code order) **in the same commit** (lints `error-docs-page-missing`, `error-docs-sidebar-unlisted`). Run `cargo xtask lint` before each such commit.
- `DOCUMENT_PROFILE_VERSION` does **not** change: `ListingItemInfo`'s serialized shape and `listing_content_globs`'s type are untouched.
- Path resolution: a config path resolves relative to the file that declared it (`claude-notes/designs/path-resolution-model.md`). Records use `BaseDirContext::base_dir_for(&value.source_info)` for both `path:` and `image:`; never the host dir by assumption.
- Cross-platform: `PathBuf` for source paths; forward-slash strings for project-relative comparisons (`path_to_forward_slashes`).
- Braid is for out-of-plan work only. Work in this plan goes in this checklist.

---

## Design decisions (the spec)

Settled with the user on 2026-08-24. Each decision names its rationale so an executor can tell a deliberate choice from an accident.

**D1. Link target is an explicit enum.** `ListingItem` replaces `source_path: PathBuf` + `output_href: String` with

```rust
pub enum ItemTarget {
    Document { source_path: PathBuf, output_href: String },
    Href(String),
    None,
}
pub enum ItemOrigin { Document, Record, RecordOverDocument }
```

Q1 has all three link states (glob/`path:`-to-document, `path:` to a non-document such as a URL or PDF, no `path:`). Options on the old fields would admit meaningless states; empty-string sentinels are the hack the feed already half-uses. Consumer behaviour per variant:

| Consumer | `Document` | `Href` | `None` |
|---|---|---|---|
| template `path` | host-relative `.qmd` (rewritten to `.html` later) | the literal, emitted as written | **absent** (so `$if(path)$` works) |
| template `outputHref` | rendered href | the literal | absent |
| template/table `filename` | source file name | last href segment, query/fragment stripped | absent |
| filter/sort `path`, `output-href`, `filename` | as today | the literal href (`path` included — Q1's `item.path` *is* the link) | missing field |
| title fallback | file stem | stem of last segment | `""` + Q-12-21 |
| L7 description/image placeholders | emitted (origin `Document` only) | not emitted | not emitted |
| feed `<link>` | absolute URL | absolute URL (remote URLs pass through `absolute_url` untouched) | item skipped |
| feed `<description>` | today (inline for `Metadata`, placeholder for `Partial`/`Full`) | **always inline** — there is no rendered sibling to substitute from | — (skipped) |
| dependency graph | edge (also from `path:` records) | — | — |

**D2. A record is the item.** Curated keys map to typed fields; every other key goes to `extra` automatically (`UnknownKeyPolicy::IntoExtra`). Front-matter `listing-item:` keeps `UnknownKeyPolicy::Drop` (bd-0t4e07jk owns the document side). Templates see `extra` keys **both** nested (`$item.extra.link$`, today's convention) and **flat** (`$item.link$`), curated names winning on collision — the table built-in already resolves bare names through `extra`, so flattening removes an inconsistency and lets Q1 templates port as `<%= item.link %>` → `$item.link$`.

**D3. Ordering: declared position.** A record occupies its own slot in `contents:`; glob items keep the index of their first matching pattern. Mechanically the sort key is `(index, is_glob)`: a glob item's index is the first *positive pattern* that matched it (unchanged from today), a record's is the count of `Glob` entries declared before it, and `is_glob` breaks the tie so a record written before a glob sorts before that glob's items. Deliberately **not** derived from provenance — `SourceInfo::for_test()` and `By::programmatic_config()` are constants, so equal-comparing sources would collapse every glob to index 0. Differs from Q1 (records appended after all glob matches) only under `sort: false` with a record written before a glob, and diverges toward what the YAML says. Documented in the guide. A record whose `path:` names a document a glob also matches yields **two items** (Q1 parity, no dedupe).

**D4. Record `path:` semantics (Q1 `listItemFromMeta`).** *Remote* (`http://`, `https://`, `data:`, protocol-relative `//`) → `Href` verbatim. **Not** `is_external_src`: that helper also treats a leading `/` as external (`helpers.rs:88-94`, correct for image `src` values), but a leading `/` on a config-authored path means *the project root* (`claude-notes/designs/path-resolution-model.md`, normative), and `join_and_normalize` already implements that re-anchor. Records therefore use a narrower `is_remote_src`. Otherwise resolve against the declaring file's dir via `join_and_normalize`; escaping the project → Q-12-17 + `Href`. Markdown extension (`qmd|md|rmd|ipynb`) → `ProjectIndex::lookup_by_source`; found → `hydrate_item(profile)` overlaid by the record (record wins per field; `categories` **replaces**, not tag-merges), origin `RecordOverDocument`; not found → Q-12-20 + `Href(raw)` so the page keeps its content. Non-markdown extension → `Href(raw)`. A `path:` record is a dependency edge: `flatten_content_globs` emits it as a literal pattern with the value's provenance.

**D5. Drafts: a seam, not a feature.** Listings do not filter drafts today (`listing_generate.rs` never reads `profile.draft`; `sidebar_auto`, `aliases`, `llms` do). Both the glob path and the record `path:` path go through one `item_visible(&DocumentProfile) -> bool` that returns `true`, documented as the seam bd-zeormbsa's `is_linkable` replaces. No draft behaviour is added here.

**D6a. Feed descriptions for non-document items.** The `Partial`/`Full`
feed description is a `{B4F502887207:<output_href>}` placeholder that the
post-render step resolves by reading the *sibling document's* rendered
HTML (`feed/binding.rs:295-320`). An `ItemTarget::Href` item has no such
sibling, so emitting the placeholder would leave a dangling token keyed on
an empty href. Non-`Document` items therefore always take the `Metadata`
branch — the record's own `description:`, inlined — whatever the feed's
`type:` is. Same reasoning as D6.

**D6. L7 placeholders only for origin `Document`.** A `RecordOverDocument` item's description/image are final strings; the post-render substitution must not overwrite them (the document-side version of this problem is the open bd-listing-description-precedence-x4bh6w3m).

**D7. Diagnostics** (codes continue from the current maximum Q-12-19):

| Code | Situation | Severity | Location |
|---|---|---|---|
| Q-12-20 | record `path:` with a markdown extension names no project document | warning | the `path:` value; `.problem` gives the resolved path and the base dir; `.add_hint` "Did you mean `…`?" when a project document has the same file name |
| Q-12-21 | record has neither `title:` nor `path:` | warning | the record |
| Q-12-22 | record key is a near-miss of a curated key (`descripton`, `titel`, `Title`) | warning | the key |
| Q-12-23 | literal `.yml`/`.yaml` `contents:` entry (Q1's YAML-file source, bd-hj1ehfn8) | warning | the entry; must **not** also raise Q-12-19 |
| Q-12-17 | record `path:` escapes the project | (existing) | reuse `glob::diagnostics::escapes_project` with key `"Listing record"`. Its message is templated (`glob/diagnostics.rs:72-88`) and says "pattern", which reads acceptably for a literal path; its **docs page currently describes `contents:` globs only** and must gain a sentence about record paths in the same commit |

Near-miss rule: optimal-string-alignment distance ≤ 1 for keys of ≤ 5 characters, ≤ 2 otherwise, compared case-insensitively (so `name` vs `date` = 2 is **not** flagged; `titel` = 1 is). No diagnostic for a record without `path:` (the Positron shape) or for record+glob duplicates.

**D8. Q-12-2 retirement.** Catalog is append-only for shipped codes (`docs/errors/README.md:89-95`); the page flips to `status: deprecated` and its body says records are supported from the next release. No emitter remains.

**D9. Built-in templates with no target.** `item-default.template` / `item-grid.template` wrap title, subtitle, thumbnail and (grid) description in `$if(path)$…$else$…$endif$` — a heading/span with the same classes and no link. Q1's `<a href="">` is not reproduced. `table_row` already unlinks when `path` is empty.

**D10. Docs.** New `docs/guides/projects/listings.qmd` seeded with a `contents:` section (globs summary pointing at Q-12-19's rules, records, `path:` overlay, declared ordering). bd-2nb6i1qv owns the rest of the guide.

**Out of scope, deliberately:** YAML-file item source (bd-hj1ehfn8); draft filtering (bd-zeormbsa); `field-required` enforcement (q2 parses it at `config.rs:492` and never enforces it for any item kind — bd-0mggxqx5); document-side auto-`extra` (bd-0t4e07jk); walker unification (bd-bqf2 — leave a comment there that both `Inline` arms now agree).

## Investigation record

**Symptom.** A listing whose `contents:` entries are maps renders an empty container; `parse_contents` (`config.rs:687-699`) builds `ListingContents::Inline(map)` *and* emits Q-12-2; `resolve_content_globs` (`glob_resolve.rs:45-65`) keeps only `Glob`; the generate loop (`transforms/listing_generate.rs:111-205`) only iterates `index.profiles()` through `hydrate_item(profile)` — the sole `ListingItem` constructor. Every eight Positron landing-page grids hit this (42 × Q-12-2). Positron's records carry `title`, `description`, `icon`, `link` (never `path:`) read by a custom template as `item.link` / `item.icon`.

**Q1 reference** (`external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts`, `readContents` @700, `listItemFromMeta` @1001): records are deep-cloned as items; `path:` with markdown extension merges the file's item under the record; drafts dropped; YAML files in `contents:` are a third source; glob items first, records appended; schema `website-listing-contents-object` is an open object.

**Consumers of `source_path`/`output_href` today:** `binding.rs:380-395,592` (`path`/`outputHref`/`filename`, table `filename`), `filter.rs:95-96`, `sort.rs:112-118`, `item.rs:60-70` (title fallback), `helpers.rs:126-165` (L7 placeholders keyed by href), `feed/binding.rs:307,316`, `feed/stage.rs:318`. Test-only literal constructors of `ListingItem` live in `binding.rs`,
`feed/binding.rs`, `feed/link_inject.rs`, `feed/stage.rs`, `filter.rs`,
`helpers.rs`, `item.rs`, `sort.rs`, `transforms/categories_sidebar.rs` and
`transforms/listing_render.rs` — the file list is what matters; let the
compiler enumerate the sites inside each.

**Pre-flight** at `596ceb572`: `cargo xtask verify --skip-hub-build --skip-hub-tests` green — 13130 passed / 199 skipped. (A first run failed 2/601 grammar tests from a stale compiled tree-sitter grammar in this worktree; `tree-sitter generate && tree-sitter build` fixed it.)

**Repro at HEAD** — fixtures in `claude-notes/plans/listing-inline-contents-investigation/` (see its README), `target/debug/q2 render` inside each, `_site/index.html` inspected:

| Fixture | Q-12-2 | items | rendered |
|---|---|---|---|
| `control/` (two globs) | 0 | 2 | `download.html` Download stub, `features.html` Features stub |
| `repro/` (two records with `path:`) | 2 | **0** | empty `<div class="list quarto-listing-default">` |
| `mixed/` (record + glob) | 1 | **1** | glob item only |
| `linkonly/` (Positron shape, no `path:`) | 2 | **0** | — |

## File structure

| File | Responsibility after this plan |
|---|---|
| `crates/quarto-core/src/project/listing/item.rs` | `ItemTarget`, `ItemOrigin`, `ListingItem`, `hydrate_item`, image rebasing (dir-based) |
| `crates/quarto-core/src/project/listing/record.rs` (**new**) | inline record → `ListingRecord` → item (`parse_record`, `record_item`, `overlay_record`), near-miss + no-title diagnostics |
| `crates/quarto-core/src/document_profile.rs` | `ListingItemInfo::from_map` + `UnknownKeyPolicy` (shared map parser); `extract_listing_item` delegates |
| `crates/quarto-core/src/project/listing/config.rs` | `ListingContents::Inline(ConfigValue)`; no Q-12-2; `flatten_content_globs` emits record `path:` edges; `is_markdown_document_path` |
| `crates/quarto-core/src/transforms/listing_generate.rs` | second item source, declared-position ordering, `path:` resolution (Q-12-20/Q-12-17), YAML literal (Q-12-23), `item_visible` seam |
| `crates/quarto-core/src/project/listing/binding.rs` | target-aware `path`/`outputHref`/`filename`; flattened `extra`; placeholder gating |
| `filter.rs`, `sort.rs`, `helpers.rs`, `feed/binding.rs`, `feed/stage.rs` | target-aware reads |
| `crates/quarto-core/src/project/listing/templates/item-{default,grid}.template` | `$if(path)$` link-or-plain |
| `crates/quarto-core/tests/integration/listing_inline_records.rs` (**new**) + `main.rs` | end-to-end fixtures through `ProjectPipeline` |
| `crates/quarto-error-catalog/error_catalog.json`, `docs/errors/listing/Q-12-{2,20,21,22,23}.qmd`, `docs/_quarto.yml` | diagnostics + pages |
| `docs/guides/projects/listings.qmd` (**new**), `claude-notes/designs/document-profile-contract.md` | user docs; contract row for `listing_content_globs` |

---

## Phase 1 — Item target model (pure refactor, no behaviour change)

### Task 1: `ItemTarget` / `ItemOrigin` on `ListingItem`

**Files:**
- Modify: `crates/quarto-core/src/project/listing/item.rs`
- Modify: `crates/quarto-core/src/project/listing/binding.rs:380-395`, `:592-597`, `:465-468`
- Modify: `crates/quarto-core/src/project/listing/filter.rs:95-96`
- Modify: `crates/quarto-core/src/project/listing/sort.rs:112-118`
- Modify: `crates/quarto-core/src/project/listing/helpers.rs:126-131`, `:152-165`
- Modify: `crates/quarto-core/src/project/listing/feed/binding.rs:307` (`link`) and `:308-320` (the description arm — see D6a)
- Modify: `crates/quarto-core/src/project/listing/feed/stage.rs:318`
- Modify: `crates/quarto-core/src/project/listing/mod.rs` (re-exports)
- Modify (tests only — literal constructors): `binding.rs`, `feed/binding.rs`, `feed/link_inject.rs`, `feed/stage.rs`, `filter.rs`, `helpers.rs`, `item.rs`, `sort.rs`, `transforms/categories_sidebar.rs`, `transforms/listing_render.rs`

**Interfaces:**
- Produces: `pub enum ItemTarget { Document { source_path: PathBuf, output_href: String }, Href(String), None }` with `ItemTarget::document(source, href)`, `source_path() -> Option<&Path>`, `href() -> Option<&str>`, `output_href() -> Option<&str>`, `filename() -> Option<String>`, `filter_path() -> Option<String>`; `pub enum ItemOrigin { Document, Record, RecordOverDocument }`; `ListingItem { …, pub target: ItemTarget, pub origin: ItemOrigin, … }` (fields `source_path`/`output_href` removed); `pub(crate) fn join_authors(&[String]) -> Option<String>`; `pub(crate) fn rebase_image_from_dir(src: &str, dir: &str) -> String`.

- [ ] **Step 1: Write the failing unit tests** — append to the `tests` module of `item.rs`:

```rust
    #[test]
    fn target_document_exposes_source_and_href() {
        let t = ItemTarget::document("posts/foo.qmd", "posts/foo.html");
        assert_eq!(t.source_path(), Some(std::path::Path::new("posts/foo.qmd")));
        assert_eq!(t.href(), Some("posts/foo.html"));
        assert_eq!(t.output_href(), Some("posts/foo.html"));
        assert_eq!(t.filename().as_deref(), Some("foo.qmd"));
    }

    #[test]
    fn target_href_is_literal_with_segment_filename() {
        let t = ItemTarget::Href("https://example.com/docs/report.pdf?v=2#top".to_string());
        assert_eq!(t.source_path(), None);
        assert_eq!(t.href(), Some("https://example.com/docs/report.pdf?v=2#top"));
        assert_eq!(t.output_href(), None, "only documents have a rendered output");
        assert_eq!(t.filename().as_deref(), Some("report.pdf"));
    }

    #[test]
    fn target_filter_path_is_source_for_documents_and_literal_for_hrefs() {
        assert_eq!(
            ItemTarget::document("posts/foo.qmd", "posts/foo.html").filter_path().as_deref(),
            Some("posts/foo.qmd")
        );
        assert_eq!(
            ItemTarget::Href("https://example.com/x".to_string()).filter_path().as_deref(),
            Some("https://example.com/x")
        );
        assert_eq!(ItemTarget::None.filter_path(), None);
    }

    #[test]
    fn target_none_has_nothing() {
        let t = ItemTarget::None;
        assert_eq!(t.source_path(), None);
        assert_eq!(t.href(), None);
        assert_eq!(t.output_href(), None);
        assert_eq!(t.filename(), None);
    }

    #[test]
    fn hydrated_item_is_a_document_target() {
        let item = hydrate_item(&profile_with(ListingItemInfo::default()));
        assert_eq!(item.origin, ItemOrigin::Document);
        assert_eq!(item.target, ItemTarget::document("posts/foo.qmd", "posts/foo.html"));
    }

    #[test]
    fn rebase_image_from_dir_handles_root_and_dotdot() {
        assert_eq!(rebase_image_from_dir("cover.png", ""), "cover.png");
        assert_eq!(rebase_image_from_dir("cover.png", "posts"), "posts/cover.png");
        assert_eq!(rebase_image_from_dir("../shared/x.png", "a/b"), "a/shared/x.png");
        assert_eq!(rebase_image_from_dir("/site.png", "posts"), "/site.png");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(target_) | test(hydrated_item_is_a_document_target) | test(rebase_image_from_dir_)'`
Expected: compile error — `ItemTarget`, `ItemOrigin`, `rebase_image_from_dir` not found.

- [ ] **Step 3: Add the types to `item.rs`** (above `pub struct ListingItem`), replace the two fields, and update `hydrate_item`:

```rust
use std::path::{Path, PathBuf};

/// Where an item's link points. See plan
/// `2026-08-24-listing-inline-contents.md` §D1.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemTarget {
    /// A project document: the rendered output is the link.
    Document {
        /// Project-relative source path (forward-slash separated,
        /// matching `DocumentProfile::source_path`).
        source_path: PathBuf,
        /// Rendered output href.
        output_href: String,
    },
    /// A literal href the author wrote (`path:` on an inline record
    /// that names no project document — a remote URL, a PDF, or a
    /// `.qmd` that does not resolve, which is Q-12-20's fallback).
    ///
    /// Emitted into the template exactly as written. Note this is
    /// *not* immune to `LinkRewriteTransform`: a dead `.qmd` literal
    /// still looks like an internal reference to it, so it may draw a
    /// second (Q-13-*) diagnostic about the same broken link. That is
    /// the honest report of a link the author asked for and does not
    /// exist — Q-12-20 explains the cause, the rewriter reports the
    /// symptom.
    Href(String),
    /// No link at all (an inline record without `path:`).
    None,
}

impl ItemTarget {
    pub fn document(source_path: impl Into<PathBuf>, output_href: impl Into<String>) -> Self {
        ItemTarget::Document {
            source_path: source_path.into(),
            output_href: output_href.into(),
        }
    }

    /// Project-relative source path — documents only.
    pub fn source_path(&self) -> Option<&Path> {
        match self {
            ItemTarget::Document { source_path, .. } => Some(source_path),
            _ => None,
        }
    }

    /// What a link should point at: the rendered output for a
    /// document, the literal for `Href`, nothing for `None`.
    pub fn href(&self) -> Option<&str> {
        match self {
            ItemTarget::Document { output_href, .. } => Some(output_href),
            ItemTarget::Href(href) => Some(href),
            ItemTarget::None => None,
        }
    }

    /// The value `path` exposes to `include:`/`exclude:` and `sort:`:
    /// the project-relative source path for a document, the literal
    /// href otherwise. Q1's `item.path` is the link either way, so
    /// filters written against Q1 keep working.
    pub fn filter_path(&self) -> Option<String> {
        match self {
            ItemTarget::Document { source_path, .. } => Some(source_path.display().to_string()),
            ItemTarget::Href(href) => Some(href.clone()),
            ItemTarget::None => None,
        }
    }

    /// Rendered output href — documents only. The key the L7
    /// post-render placeholders and the feed's sibling lookup use.
    pub fn output_href(&self) -> Option<&str> {
        match self {
            ItemTarget::Document { output_href, .. } => Some(output_href),
            _ => None,
        }
    }

    /// Display file name: the source file's name for a document, the
    /// last path segment (query and fragment stripped) for a literal
    /// href — Q1 fills `filename` from `basename(path)` either way.
    pub fn filename(&self) -> Option<String> {
        match self {
            ItemTarget::Document { source_path, .. } => source_path
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            ItemTarget::Href(href) => href
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(String::from),
            ItemTarget::None => None,
        }
    }
}

/// How an item came to exist. Drives the generate transform's
/// decisions (L7 placeholder gating, diagnostics wording) so they
/// are explicit rather than inferred from the target's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOrigin {
    /// Matched by a `contents:` glob; hydrated from a `DocumentProfile`.
    Document,
    /// An inline `contents:` record with no document behind it.
    Record,
    /// An inline record whose `path:` named a project document; the
    /// record's fields were laid over the document's item.
    RecordOverDocument,
}
```

In `ListingItem`, replace

```rust
    pub source_path: PathBuf,
    pub output_href: String,
```

(and their doc comments) with

```rust
    /// Where the item links. See [`ItemTarget`].
    pub target: ItemTarget,
    /// How the item came to exist. See [`ItemOrigin`].
    pub origin: ItemOrigin,
```

In `hydrate_item`, replace `source_path: profile.source_path.clone(), output_href: profile.output_href.clone(),` with

```rust
        target: ItemTarget::document(profile.source_path.clone(), profile.output_href.clone()),
        origin: ItemOrigin::Document,
```

Replace `rebase_image` with a dir-based core (keep the old signature as a thin wrapper — `hydrate_item` still calls it):

```rust
fn rebase_image(src: &str, source_path: &Path) -> String {
    let dir = source_path
        .parent()
        .map(|d| {
            d.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(os) => os.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    rebase_image_from_dir(src, &dir)
}

/// Rebase a relative image path onto a project-relative directory
/// (`""` for the project root), normalizing `.`/`..` segments.
/// Absolute URLs, `data:` URIs, and root-absolute paths pass through.
pub(crate) fn rebase_image_from_dir(src: &str, dir: &str) -> String {
    if super::helpers::is_external_src(src) {
        return src.to_string();
    }
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in src.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}
```

Make `join_authors` `pub(crate)`. Add `pub use item::{ItemOrigin, ItemTarget, ListingItem, hydrate_item};` in `mod.rs`.

- [ ] **Step 4: Migrate the consumers** — each is a one-line `match`-free rewrite via the accessors:

`binding.rs:380-395` becomes:

```rust
    let path: Option<String> = match &item.target {
        ItemTarget::Document { source_path, .. } => Some(host_relative_qmd(source_path, host_dir)),
        ItemTarget::Href(href) => Some(href.clone()),
        ItemTarget::None => None,
    };
    if let Some(p) = &path {
        m.insert("path".to_string(), TemplateValue::String(p.clone()));
    }
    if let Some(href) = item.target.href() {
        m.insert("outputHref".to_string(), TemplateValue::String(href.to_string()));
    }
    if let Some(name) = item.target.filename() {
        m.insert("filename".to_string(), TemplateValue::String(name));
    }
```

and the `table_row(…)` call at `:465-468` passes `path.as_deref().unwrap_or("")`. `item_field_display_value` `"filename"` arm (`:592-597`) becomes `"filename" => item.target.filename(),`. Add `use super::item::ItemTarget;` (or `super::ItemTarget`).

`filter.rs:95-96` — `path` falls back to the literal href for a
non-document item, so `include:`/`exclude:` see the same string the
template links to (D1's table):

```rust
        "path" => Curated::Scalar(item.target.filter_path()),
        "output-href" => Curated::Scalar(item.target.href().map(String::from)),
```

`sort.rs:112-118`:

```rust
        "filename" => item.target.filename(),
        "path" => item.target.filter_path(),
        "output-href" => item.target.href().map(String::from),
```

`helpers.rs:129` and `:165`: `&item.output_href` → `item.target.output_href().unwrap_or("")` (gating by origin comes in Task 8; this task keeps behaviour identical for documents).

`feed/binding.rs:307`: `let link = absolute_url(site_url, item.target.href().unwrap_or(""));`.
`feed/stage.rs:318`: `if it.title.trim().is_empty() || it.target.href().is_none() {`
— an item with no link is skipped, which is what the old
`output_href.is_empty()` check meant.

The description arm at `:308-320` needs the D6a rule, not a mechanical
swap — a `Partial`/`Full` placeholder keyed on an empty href would dangle:

```rust
    let description_element = match (feed_options.kind, item.target.output_href()) {
        // No rendered sibling to read: inline what the record itself
        // says, whatever the feed type (plan §D6a).
        (FeedType::Metadata, _) | (_, None) => {
            let desc = item.description.as_deref().unwrap_or("");
            format!("<description><![CDATA[{}]]></description>", desc)
        }
        (FeedType::Partial | FeedType::Full, Some(output_href)) => format!(
            "<description>{{{}:{}}}</description>",
            FEED_PLACEHOLDER_TOKEN, output_href
        ),
    };
```

Add a test beside the existing ones in `feed/binding.rs`:

```rust
    // A record item has no rendered sibling, so a Full feed still
    // inlines the record's own description (plan §D6a).
    #[test]
    fn record_item_gets_an_inline_description_even_in_a_full_feed() {
        let mut item = empty_listing_item();
        item.title = "Card".to_string();
        item.description = Some("hand-written".to_string());
        item.target = ItemTarget::Href("https://example.com/card".to_string());
        item.origin = ItemOrigin::Record;
        let feed_options = ListingFeedOptions {
            kind: FeedType::Full,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.description_element,
            "<description><![CDATA[hand-written]]></description>"
        );
        assert_eq!(fi.link, "https://example.com/card", "remote links pass through");
    }

    // …and a document item in a Full feed still gets the placeholder.
    #[test]
    fn document_item_keeps_the_full_feed_placeholder() {
        let mut item = empty_listing_item();
        item.title = "Doc".to_string();
        item.target = ItemTarget::document("posts/foo.qmd", "posts/foo.html");
        let feed_options = ListingFeedOptions {
            kind: FeedType::Full,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert!(
            fi.description_element.contains(FEED_PLACEHOLDER_TOKEN)
                && fi.description_element.contains("posts/foo.html"),
            "got {}",
            fi.description_element
        );
    }
```

Both use the module's existing `empty_listing_item()` (`feed/binding.rs:571`)
and `default_feed_options()` (`:593`); `Path` is already imported there.

Every test-only literal `ListingItem { … source_path: PathBuf::from(X), output_href: Y.to_string(), … }` becomes `target: ItemTarget::document(X, Y), origin: ItemOrigin::Document,` (import `crate::project::listing::{ItemOrigin, ItemTarget}` in the transforms tests). Tests that *assign* `item.output_href = "…"` (`feed/binding.rs:886-980`) become `item.target = ItemTarget::document("<same stem>.qmd", "…");`. The two `make_item` helpers keep the paths they already use, just moved into
the target: `filter.rs:165` has a fixed `"posts/foo.qmd"`; `sort.rs:170-171`
derives `format!("posts/{title}.qmd")`. Pass each one's existing source path
and href to `ItemTarget::document(…)` unchanged — nothing asserts on them, but
altering them makes the diff lie about what this refactor did.

- [ ] **Step 5: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: clean clippy; all tests pass including the five new ones (no snapshot changes — this is a pure refactor).

- [ ] **Step 6: Commit**

```bash
git add -A crates/quarto-core
git commit -m "Give listing items an explicit link target instead of assuming a document (bd-listing-inline-contents-tyy446ze)"
```

---

## Phase 2 — One record parser

### Task 2: `ListingItemInfo::from_map` with an unknown-key policy

**Files:**
- Modify: `crates/quarto-core/src/document_profile.rs:873-896` (`extract_listing_item`), `:909-930` (`extract_listing_item_extra`)

**Interfaces:**
- Produces: `pub enum UnknownKeyPolicy { Drop, IntoExtra { except: &'static [&'static str] } }`; `pub const LISTING_ITEM_KEYS: &[&str]`; `impl ListingItemInfo { pub fn from_map(li: &ConfigValue, unknown: UnknownKeyPolicy) -> Self }`. Front-matter behaviour unchanged.

- [ ] **Step 1: Write the failing tests** (in `document_profile.rs`'s `tests` module; build values with the crate's `ConfigValue` constructors):

```rust
    fn cv_s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, quarto_source_map::SourceInfo::for_test())
    }
    fn cv_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: quarto_source_map::SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            quarto_source_map::SourceInfo::for_test(),
        )
    }

    #[test]
    fn from_map_drop_policy_ignores_unknown_keys() {
        let li = cv_map(vec![("title", cv_s("T")), ("icon", cv_s("bi-star"))]);
        let info = ListingItemInfo::from_map(&li, UnknownKeyPolicy::Drop);
        assert_eq!(info.title.as_deref(), Some("T"));
        assert!(info.extra.is_empty(), "Drop must not forward `icon`");
    }

    #[test]
    fn from_map_into_extra_routes_unknown_keys_and_skips_excepted() {
        let li = cv_map(vec![
            ("title", cv_s("T")),
            ("icon", cv_s("bi-star")),
            ("link", cv_s("x.html")),
            ("path", cv_s("owned-by-caller.qmd")),
        ]);
        let info = ListingItemInfo::from_map(&li, UnknownKeyPolicy::IntoExtra { except: &["path"] });
        assert_eq!(info.title.as_deref(), Some("T"));
        assert_eq!(info.extra.get("icon").and_then(|v| v.as_plain_text()).as_deref(), Some("bi-star"));
        assert_eq!(info.extra.get("link").and_then(|v| v.as_plain_text()).as_deref(), Some("x.html"));
        assert!(!info.extra.contains_key("title"), "curated keys never land in extra");
        assert!(!info.extra.contains_key("path"), "excepted keys are the caller's");
    }

    #[test]
    fn from_map_explicit_extra_wins_over_bare_key() {
        let li = cv_map(vec![
            ("status", cv_s("bare")),
            ("extra", cv_map(vec![("status", cv_s("explicit"))])),
        ]);
        let info = ListingItemInfo::from_map(&li, UnknownKeyPolicy::IntoExtra { except: &[] });
        assert_eq!(info.extra.get("status").and_then(|v| v.as_plain_text()).as_deref(), Some("explicit"));
        assert_eq!(info.extra.len(), 1, "`extra` itself is not forwarded as a key");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(from_map_)'`
Expected: compile error — `UnknownKeyPolicy` / `from_map` not found.

- [ ] **Step 3: Implement** — add next to `ListingItemInfo`:

```rust
/// What [`ListingItemInfo::from_map`] does with keys it does not
/// recognize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownKeyPolicy {
    /// Drop them. Front-matter `listing-item:` — custom fields must
    /// be declared under `extra:` (bd-0t4e07jk owns widening this).
    Drop,
    /// Route them into `extra`. Inline `contents:` records — the
    /// record *is* the item, so every key is intentional. Keys in
    /// `except` belong to the caller and are neither typed here nor
    /// forwarded.
    IntoExtra { except: &'static [&'static str] },
}

/// Keys [`ListingItemInfo::from_map`] reads into typed fields.
pub const LISTING_ITEM_KEYS: &[&str] = &[
    "title",
    "subtitle",
    "description",
    "image",
    "image-alt",
    "date",
    "date-modified",
    "categories",
    "reading-time-minutes",
    "word-count",
    "extra",
];

impl ListingItemInfo {
    /// Build from any map-shaped `ConfigValue` — the `listing-item:`
    /// front-matter block or one inline `contents:` record. Type
    /// mismatches at known keys leave the field at its default.
    pub fn from_map(li: &ConfigValue, unknown: UnknownKeyPolicy) -> Self {
        let mut extra = extract_listing_item_extra(li);
        if let UnknownKeyPolicy::IntoExtra { except } = unknown
            && let Some(entries) = li.as_map_entries()
        {
            for entry in entries {
                let key = entry.key.as_str();
                if LISTING_ITEM_KEYS.contains(&key) || except.contains(&key) {
                    continue;
                }
                // An explicit `extra:` entry of the same name wins.
                extra
                    .entry(entry.key.clone())
                    .or_insert_with(|| entry.value.clone());
            }
        }
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
            extra,
        }
    }
}
```

(Let-chains are already used in this crate — `listing_generate.rs:269-271`, `config.rs:1573-1575` — so this compiles as written.) `extract_listing_item` becomes:

```rust
fn extract_listing_item(meta: &ConfigValue) -> ListingItemInfo {
    meta.get("listing-item")
        .map(|li| ListingItemInfo::from_map(li, UnknownKeyPolicy::Drop))
        .unwrap_or_default()
}
```

- [ ] **Step 4: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core -E 'binary(quarto-core) & (test(from_map_) | test(listing_item) | test(profile_))'`
Expected: PASS (all existing profile tests unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/quarto-core/src/document_profile.rs
git commit -m "Lift listing-item extraction into ListingItemInfo::from_map with an unknown-key policy (bd-listing-inline-contents-tyy446ze)"
```

### Task 3: `ListingContents::Inline` carries the whole record; Q-12-2 stops firing

**Files:**
- Modify: `crates/quarto-core/src/project/listing/config.rs:142-153`, `:687-699`, tests `:1318-1336`, `:1503-1521`, and the `glob_patterns` test helper at `:1090-1098` (its `Inline(_) => None` arm compiles unchanged — listed so the inventory is complete)
- Modify: `crates/quarto-core/src/project/listing/glob_resolve.rs:198-209` (test)

**Interfaces:**
- Produces: `ListingContents::Inline(ConfigValue)` — the record map with its own span and per-key `key_source`s intact.

- [ ] **Step 1: Rewrite the two existing tests to the new contract** — in `config.rs` replace `contents_inline_record_emits_diagnostic` with:

```rust
    // 7. inline-record contents are captured whole, with no diagnostic
    #[test]
    fn contents_inline_record_is_captured_without_diagnostic() {
        let (listings, diags) = parse(map(vec![(
            "contents",
            arr(vec![map(vec![
                ("title", s("foo")),
                ("path", s("bar.html")),
            ])]),
        )]));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].contents.len(), 1);
        let ListingContents::Inline(record) = &listings[0].contents[0] else {
            panic!("expected Inline, got {:?}", listings[0].contents[0]);
        };
        assert_eq!(record.get("title").and_then(|v| v.as_plain_text()).as_deref(), Some("foo"));
        assert_eq!(record.get("path").and_then(|v| v.as_plain_text()).as_deref(), Some("bar.html"));
        assert!(diags.is_empty(), "Q-12-2 is retired; got {:?}", diags);
    }
```

Delete `q_12_2_underlines_the_whole_inline_contents_record` (there is no diagnostic left to span; Task 4 adds the Q-12-22 span test in its place). In `glob_resolve.rs`, the `inline_records_contribute_nothing` test constructs `ListingContents::Inline(quarto_pandoc_types::ConfigValue::new_map(vec![], SourceInfo::for_test()))`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(contents_inline_record_is_captured_without_diagnostic) | test(inline_records_contribute_nothing)'`
Expected: a **compile error from `glob_resolve.rs`** — `ListingContents::Inline`
still holds a `BTreeMap`, so the rewritten `inline_records_contribute_nothing`
cannot pass it a `ConfigValue`. (Note the config test alone would *not* fail
to compile: `.get("title")` resolves on a `BTreeMap` too. It fails at
`assert!(diags.is_empty())` once the crate builds. Both edits are needed for
this step to mean anything.)

- [ ] **Step 3: Implement** — enum variant:

```rust
    /// Inline metadata record — the whole map, so the record's own
    /// span and each key's `key_source` survive to the generate
    /// transform (`record::parse_record`). The record *is* the item
    /// (plan §D2); no glob resolution is involved.
    Inline(ConfigValue),
```

and the `Map` arm of `parse_contents` becomes simply `ConfigValueKind::Map(_) => Some(ListingContents::Inline(item.clone())),` — delete the `push_diag(… "Q-12-2" …)` call and the `BTreeMap` collect. Update the doc comments on `resolve_content_globs` (`glob_resolve.rs:36-39`) and `flatten_content_globs` (`config.rs:1004-1006`) to say records are handled by the generate transform / Task 6 rather than "Q-12-2".

- [ ] **Step 4: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: PASS. `grep -rn '"Q-12-2"' crates/quarto-core/src` returns nothing.

- [ ] **Step 5: Commit**

```bash
git add crates/quarto-core/src/project/listing
git commit -m "Keep inline contents records whole and stop emitting Q-12-2 (bd-listing-inline-contents-tyy446ze)"
```

### Task 4: `record.rs` — record → item, with Q-12-21 / Q-12-22

**Files:**
- Create: `crates/quarto-core/src/project/listing/record.rs`
- Modify: `crates/quarto-core/src/project/listing/mod.rs` (`pub mod record;`)
- Modify: `crates/quarto-error-catalog/error_catalog.json` (after `"Q-12-19"`)
- Create: `docs/errors/listing/Q-12-21.qmd`, `docs/errors/listing/Q-12-22.qmd`
- Modify: `docs/_quarto.yml:205` (append the two entries after `Q-12-19.qmd`)

**Interfaces:**
- Consumes: `ListingItemInfo::from_map`, `UnknownKeyPolicy` (Task 2); `ItemTarget`, `ItemOrigin`, `join_authors`, `rebase_image_from_dir` (Task 1); `crate::metadata::authors::parse_authors_model(&ConfigValue) -> AuthorsModel` (`.authors[i].name.literal: String`).
- Produces: `pub struct ListingRecord { pub info: ListingItemInfo, pub authors: Vec<String>, pub order: Option<i32>, pub path: Option<(String, SourceInfo)>, pub source: SourceInfo }`; `pub fn parse_record(value: &ConfigValue, diags: &mut Vec<DiagnosticMessage>) -> ListingRecord`; `pub fn record_item(rec: ListingRecord, target: ItemTarget, base_dir: &str) -> ListingItem`; `pub fn overlay_record(item: ListingItem, rec: ListingRecord, base_dir: &str) -> ListingItem`.

- [ ] **Step 1: Write the failing tests.** Create `record.rs` containing *only* the `tests` module below (plus `pub mod record;` in `mod.rs`), and run it. The module items it calls do not exist yet, so the failure is a compile error — that is the point.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }
    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }
    fn parse(value: &ConfigValue) -> (ListingRecord, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let rec = parse_record(value, &mut diags);
        (rec, diags)
    }
    fn codes(diags: &[DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    #[test]
    fn curated_keys_are_typed_and_unknown_keys_land_in_extra() {
        let (rec, diags) = parse(&map(vec![
            ("title", s("Get started")),
            ("description", s("Download and install Positron")),
            ("icon", s("bi-rocket-takeoff")),
            ("link", s("download.qmd")),
        ]));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(rec.info.title.as_deref(), Some("Get started"));
        assert_eq!(rec.info.description.as_deref(), Some("Download and install Positron"));
        assert_eq!(rec.info.extra.get("icon").and_then(|v| v.as_plain_text()).as_deref(), Some("bi-rocket-takeoff"));
        assert_eq!(rec.info.extra.get("link").and_then(|v| v.as_plain_text()).as_deref(), Some("download.qmd"));
        assert!(!rec.info.extra.contains_key("title"));
        assert_eq!(rec.path, None);
    }

    #[test]
    fn author_accepts_string_and_list_and_stays_out_of_extra() {
        let (one, _) = parse(&map(vec![("title", s("T")), ("author", s("Jane Doe"))]));
        assert_eq!(one.authors, vec!["Jane Doe"]);
        let (two, _) = parse(&map(vec![
            ("title", s("T")),
            ("author", arr(vec![s("Jane Doe"), s("John Roe")])),
        ]));
        assert_eq!(two.authors, vec!["Jane Doe", "John Roe"]);
        assert!(!two.info.extra.contains_key("author"));
    }

    #[test]
    fn path_and_order_are_owned_by_the_record() {
        let path_value = ConfigValue::new_string("download.qmd", SourceInfo::for_test());
        let expected_source = path_value.source_info.clone();
        let (rec, diags) = parse(&map(vec![("path", path_value), ("order", s("3"))]));
        assert!(diags.is_empty(), "a `path:` supplies the title fallback; {diags:?}");
        assert_eq!(rec.path, Some(("download.qmd".to_string(), expected_source)));
        assert_eq!(rec.order, Some(3));
        assert!(!rec.info.extra.contains_key("path"));
        assert!(!rec.info.extra.contains_key("order"));
    }

    #[test]
    fn missing_title_without_path_warns_q_12_21() {
        let (_, diags) = parse(&map(vec![("description", s("no title here"))]));
        assert_eq!(codes(&diags), vec!["Q-12-21"]);
    }

    #[test]
    fn near_miss_keys_warn_q_12_22_with_the_intended_key() {
        let (rec, diags) = parse(&map(vec![
            ("title", s("T")),
            ("descripton", s("typo")),
            ("Title", s("case")),
        ]));
        let hits: Vec<&DiagnosticMessage> = diags.iter().filter(|d| d.code.as_deref() == Some("Q-12-22")).collect();
        assert_eq!(hits.len(), 2, "{diags:?}");
        assert!(hits[0].title.contains("`descripton`") && hits[0].title.contains("`description`"));
        assert!(hits[1].title.contains("`Title`") && hits[1].title.contains("`title`"));
        // The key is kept as a custom field regardless.
        assert!(rec.info.extra.contains_key("descripton"));
    }

    #[test]
    fn short_or_distant_keys_are_not_near_misses() {
        let (_, diags) = parse(&map(vec![
            ("title", s("T")),
            ("name", s("2 from `date` — not flagged at length 4")),
            ("link", s("x")),
            ("icon", s("y")),
            ("hide-profiles", arr(vec![s("positron")])),
        ]));
        assert!(codes(&diags).is_empty(), "{diags:?}");
    }

    #[test]
    fn osa_distance_counts_transpositions_once() {
        assert_eq!(osa_distance("titel", "title"), 1);
        assert_eq!(osa_distance("descripton", "description"), 1);
        assert_eq!(osa_distance("name", "date"), 2);
        assert_eq!(osa_distance("", "abc"), 3);
    }

    #[test]
    fn record_item_uses_record_fields_and_falls_back_to_href_stem() {
        let (rec, _) = parse(&map(vec![("path", s("guides/report.pdf")), ("image", s("cover.png"))]));
        let item = record_item(rec, ItemTarget::Href("guides/report.pdf".to_string()), "sub");
        assert_eq!(item.title, "report");
        assert_eq!(item.origin, ItemOrigin::Record);
        assert_eq!(item.image.as_deref(), Some("sub/cover.png"), "image rebases onto the declaring dir");
        assert_eq!(item.target, ItemTarget::Href("guides/report.pdf".to_string()));
    }

    #[test]
    fn overlay_record_fields_win_and_origin_flips() {
        use crate::document_profile::DocumentProfile;
        let profile = DocumentProfile {
            source_path: std::path::PathBuf::from("download.qmd"),
            output_href: "download.html".to_string(),
            format_id: "html".to_string(),
            title: Some("Download stub".to_string()),
            description: Some("from the document".to_string()),
            categories: vec!["doc-cat".to_string()],
            authors: vec!["Doc Author".to_string()],
            ..DocumentProfile::default()
        };
        let base = crate::project::listing::hydrate_item(&profile);
        let (rec, _) = parse(&map(vec![
            ("title", s("Get started")),
            ("path", s("download.qmd")),
            ("categories", arr(vec![s("rec-cat")])),
            ("icon", s("bi-rocket-takeoff")),
        ]));
        let item = overlay_record(base, rec, "");
        assert_eq!(item.title, "Get started", "record title wins");
        assert_eq!(item.description.as_deref(), Some("from the document"), "unset record fields keep the document's");
        assert_eq!(item.categories, vec!["rec-cat"], "categories replace, not merge (Q1 spread)");
        assert_eq!(item.authors, vec!["Doc Author"], "no record author → document authors kept");
        assert_eq!(item.origin, ItemOrigin::RecordOverDocument);
        assert_eq!(item.target, ItemTarget::document("download.qmd", "download.html"));
        assert_eq!(item.extra.get("icon").and_then(|v| v.as_plain_text()).as_deref(), Some("bi-rocket-takeoff"));
    }

    /// Q-12-22 must underline the misspelled *key*, not the record.
    #[test]
    fn q_12_22_underlines_the_offending_key() {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::{InterpretationContext, MergedConfig};
        const FIXTURE_FILE: &str = "index.qmd";
        let yaml = "\
listing:
  contents:
    - title: Inline
      descripton: oops
";
        let parsed = quarto_yaml::parse_file(yaml, FIXTURE_FILE).expect("valid yaml");
        let mut collector = DiagnosticCollector::new();
        let doc_config = yaml_to_config_value(parsed, InterpretationContext::DocumentMetadata, &mut collector);
        let merged = MergedConfig::new(vec![&doc_config]).materialize().expect("materialize");
        let listing_value = merged.get("listing").expect("`listing:` present");
        let mut diags = Vec::new();
        let listings = crate::project::listing::parse_listings(listing_value, &mut diags);
        let ListingContents::Inline(record) = &listings[0].contents[0] else { panic!("expected a record") };
        let (_, diags) = parse(record);
        let ctx = quarto_config::span_assert::context_for(FIXTURE_FILE, yaml);
        let d = diags.iter().find(|d| d.code.as_deref() == Some("Q-12-22")).expect("Q-12-22");
        let span = quarto_config::span_assert::resolve_diagnostic_span(d, &ctx).expect("real span");
        assert_eq!(span.text.trim(), "descripton", "got {:?}", span.text);
    }
}
```

(`use crate::project::listing::ListingContents;` goes in the test module's imports.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'binary(quarto-core) & test(record::tests)'`
Expected: compile error — module items missing.

- [ ] **Step 3: Implement `record.rs`** (above the tests):

```rust
/*
 * project/listing/record.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Inline `contents:` records → listing items
//! (bd-listing-inline-contents-tyy446ze, plan §D2/§D4/§D7).
//!
//! A record *is* the item (Q1 `listItemFromMeta`): curated keys map
//! to typed fields, everything else is a custom field in `extra`.
//! `path:` is captured raw — resolving it needs the project index
//! and the declaring file's directory, which the generate transform
//! has and this module does not.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;

use super::item::{ItemOrigin, ItemTarget, ListingItem, join_authors, rebase_image_from_dir};
use crate::document_profile::{LISTING_ITEM_KEYS, ListingItemInfo, UnknownKeyPolicy};

/// Keys this module owns: typed here, never forwarded to `extra`.
const RECORD_OWN_KEYS: &[&str] = &["author", "authors", "path", "order"];

/// One parsed inline record.
#[derive(Debug, Clone)]
pub struct ListingRecord {
    pub info: ListingItemInfo,
    pub authors: Vec<String>,
    pub order: Option<i32>,
    /// Raw `path:` value with its provenance.
    pub path: Option<(String, SourceInfo)>,
    /// The record's own span — for diagnostics that blame the whole record.
    pub source: SourceInfo,
}

pub fn parse_record(value: &ConfigValue, diags: &mut Vec<DiagnosticMessage>) -> ListingRecord {
    let info = ListingItemInfo::from_map(
        value,
        UnknownKeyPolicy::IntoExtra {
            except: RECORD_OWN_KEYS,
        },
    );
    let authors = crate::metadata::authors::parse_authors_model(value)
        .authors
        .iter()
        .map(|a| a.name.literal.clone())
        .collect();
    let order = value
        .get("order")
        .and_then(|v| v.as_int_lenient())
        .and_then(|i| i32::try_from(i).ok());
    let path = value
        .get("path")
        .and_then(|v| v.as_plain_text().map(|p| (p, v.source_info.clone())));

    diagnose_near_misses(value, diags);
    if info.title.is_none() && path.is_none() {
        diags.push(
            DiagnosticMessageBuilder::warning("Listing record has no `title:`")
                .with_code("Q-12-21")
                .with_location(value.source_info.clone())
                .problem(
                    "The record names no `path:` either, so there is nothing to derive a \
                     title from; the item renders with an empty title.",
                )
                .add_hint("Add `title:` to the record.")
                .build(),
        );
    }

    ListingRecord {
        info,
        authors,
        order,
        path,
        source: value.source_info.clone(),
    }
}

/// Build the item for a record that has no document behind it.
/// `base_dir` is the declaring file's project-relative directory —
/// relative `image:` values rebase onto it (path-resolution contract).
pub fn record_item(rec: ListingRecord, target: ItemTarget, base_dir: &str) -> ListingItem {
    let li = rec.info;
    let title = li
        .title
        .or_else(|| target.filename().map(|f| stem(&f)))
        .unwrap_or_default();
    ListingItem {
        title,
        subtitle: li.subtitle,
        description: li.description,
        author: join_authors(&rec.authors),
        authors: rec.authors,
        date: li.date,
        date_modified: li.date_modified,
        categories: li.categories,
        image: li.image.map(|img| rebase_image_from_dir(&img, base_dir)),
        image_alt: li.image_alt,
        image_lazy_loading: None,
        reading_time_minutes: li.reading_time_minutes,
        word_count: li.word_count,
        order: rec.order,
        target,
        origin: ItemOrigin::Record,
        extra: li.extra,
    }
}

/// Lay a record over a document's hydrated item: every field the
/// record sets wins; `categories` replaces rather than tag-merges
/// (Q1 spreads the record over the document's item).
pub fn overlay_record(mut item: ListingItem, rec: ListingRecord, base_dir: &str) -> ListingItem {
    let li = rec.info;
    if let Some(t) = li.title {
        item.title = t;
    }
    if li.subtitle.is_some() {
        item.subtitle = li.subtitle;
    }
    if li.description.is_some() {
        item.description = li.description;
    }
    if li.date.is_some() {
        item.date = li.date;
    }
    if li.date_modified.is_some() {
        item.date_modified = li.date_modified;
    }
    if let Some(img) = li.image {
        item.image = Some(rebase_image_from_dir(&img, base_dir));
    }
    if li.image_alt.is_some() {
        item.image_alt = li.image_alt;
    }
    if li.reading_time_minutes.is_some() {
        item.reading_time_minutes = li.reading_time_minutes;
    }
    if li.word_count.is_some() {
        item.word_count = li.word_count;
    }
    if !li.categories.is_empty() {
        item.categories = li.categories;
    }
    if !rec.authors.is_empty() {
        item.author = join_authors(&rec.authors);
        item.authors = rec.authors;
    }
    if rec.order.is_some() {
        item.order = rec.order;
    }
    item.extra.extend(li.extra);
    item.origin = ItemOrigin::RecordOverDocument;
    item
}

fn stem(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => filename.to_string(),
    }
}

/// Curated keys an author might misspell.
const NEAR_MISS_TARGETS: &[&str] = &[
    "title",
    "subtitle",
    "description",
    "author",
    "authors",
    "date",
    "date-modified",
    "image",
    "image-alt",
    "categories",
    "path",
    "order",
    "reading-time-minutes",
    "word-count",
    "extra",
];

/// Q-12-22: unknown keys flow silently into `extra` (plan §D2), so a
/// typo'd curated key would otherwise be invisible.
fn diagnose_near_misses(value: &ConfigValue, diags: &mut Vec<DiagnosticMessage>) {
    let Some(entries) = value.as_map_entries() else {
        return;
    };
    for entry in entries {
        let key = entry.key.as_str();
        if NEAR_MISS_TARGETS.contains(&key) || LISTING_ITEM_KEYS.contains(&key) {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        let limit = if lower.chars().count() <= 5 { 1 } else { 2 };
        let Some(target) = NEAR_MISS_TARGETS
            .iter()
            .find(|t| osa_distance(&lower, t) <= limit)
        else {
            continue;
        };
        diags.push(
            DiagnosticMessageBuilder::warning(format!(
                "Listing record key `{key}` looks like a misspelling of `{target}`"
            ))
            .with_code("Q-12-22")
            .with_location(entry.key_source.clone())
            .problem(format!(
                "`{key}` is not a listing field, so it was kept as the custom field \
                 `item.{key}`; the built-in templates will not display it."
            ))
            .add_hint(format!(
                "Rename the key to `{target}`, or keep it if a custom template reads `item.{key}`."
            ))
            .build(),
        );
    }
}

/// Optimal string alignment distance (Levenshtein + adjacent
/// transposition counted once). Small inputs; O(len·len) is fine.
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}
```

Add `pub mod record;` to `mod.rs`.

- [ ] **Step 4: Catalog + pages + sidebar for Q-12-21 and Q-12-22** — insert after the `"Q-12-19"` object in `error_catalog.json`:

```json
  "Q-12-21": {
    "subsystem": "listing",
    "title": "Listing Record Has No Title",
    "message_template": "An inline `contents:` record has neither `title:` nor `path:`; the item renders with an empty title.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-21",
    "since_version": "99.9.9"
  },
  "Q-12-22": {
    "subsystem": "listing",
    "title": "Listing Record Key Looks Misspelled",
    "message_template": "A key in an inline `contents:` record is a near-miss of a listing field; it was kept as a custom field.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-22",
    "since_version": "99.9.9"
  },
```

(Q-12-20 and Q-12-23 are added in Task 5, in code order; insert 21/22 now, leaving room — JSON object order is not semantically significant but keep them ascending for readers.) Create the two pages from the template in `docs/errors/README.md:100-120` with `status: stub`, `subsystem: listing`, `since: "99.9.9"`, `categories: [listing]`. Q-12-21 body: what a record is; why (forgot `title:`, or meant `path:` to a document); fix: add `title:`. Q-12-22 body: records forward unknown keys to custom fields, so typos are invisible; why (`descripton`, `Title`); fix: rename, or ignore if a custom template reads the key; the near-miss rule (≤1 edit for short keys, ≤2 otherwise). Append `- errors/listing/Q-12-21.qmd` and `- errors/listing/Q-12-22.qmd` after line 205 of `docs/_quarto.yml`.

- [ ] **Step 5: Gate**

Run: `cargo xtask lint --quiet && cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: lint clean; PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/quarto-core/src/project/listing crates/quarto-error-catalog/error_catalog.json docs/errors/listing/Q-12-21.qmd docs/errors/listing/Q-12-22.qmd docs/_quarto.yml
git commit -m "Parse inline listing records into items, with near-miss and no-title diagnostics (bd-listing-inline-contents-tyy446ze)"
```

**Phase boundary:** `cargo nextest run --workspace` — report the delta vs 13130/199 (expect +N new tests, 0 failures).

---

## Phase 3 — Second item source

### Task 5: Records in the generate transform (ordering, `path:` resolution, Q-12-20, Q-12-23, `item_visible`)

**Files:**
- Modify: `crates/quarto-core/src/transforms/listing_generate.rs:36-48` (imports), `:111-205` (loop)
- Modify: `crates/quarto-core/src/project/listing/config.rs` (add `pub(crate) fn is_markdown_document_path`)
- Modify: `crates/quarto-core/src/project/listing/helpers.rs` (add `pub(crate) fn is_remote_src`)
- Modify: `docs/errors/listing/Q-12-17.qmd` (it now also fires for a record `path:`)
- Modify: `crates/quarto-error-catalog/error_catalog.json`; Create: `docs/errors/listing/Q-12-20.qmd`, `Q-12-23.qmd`; Modify: `docs/_quarto.yml`

**Interfaces:**
- Consumes: `parse_record`, `record_item`, `overlay_record` (Task 4); `ItemTarget`, `ItemOrigin`, `hydrate_item` (Task 1); `crate::glob::{BaseDirContext, has_metacharacters, join_and_normalize, path_to_forward_slashes}`; `crate::glob::diagnostics::escapes_project(code, key, pattern, source)`; `ProjectIndex::lookup_by_source(&Path)`.
- Produces: `pub(crate) fn is_remote_src(&str) -> bool` in `helpers.rs`.
- Produces: `pub(crate) fn is_markdown_document_path(p: &str) -> bool` in `config.rs`; private `item_visible(&DocumentProfile) -> bool` seam.

- [ ] **Step 1: Write the failing tests** — append to `listing_generate.rs`'s `tests` module (helpers `s`, `b`, `arr`, `map`, `make_profile`, `run_transform` exist there):

```rust
    fn contents_listing(entries: Vec<ConfigValue>) -> ConfigValue {
        map(vec![("listing", map(vec![("id", s("l")), ("contents", arr(entries))]))])
    }
    fn contents_listing_unsorted(entries: Vec<ConfigValue>) -> ConfigValue {
        map(vec![(
            "listing",
            map(vec![("id", s("l")), ("sort", b(false)), ("contents", arr(entries))]),
        )])
    }
    fn codes(diags: &[quarto_error_reporting::DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    #[tokio::test]
    async fn record_without_path_becomes_unlinked_item_with_custom_fields() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Get started")),
                ("icon", s("bi-rocket-takeoff")),
                ("link", s("download.qmd")),
            ])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        let item = &resolved[0].items[0];
        assert_eq!(item.title, "Get started");
        assert_eq!(item.target, ItemTarget::None);
        assert_eq!(item.origin, ItemOrigin::Record);
        assert_eq!(item.extra.get("link").and_then(|v| v.as_plain_text()).as_deref(), Some("download.qmd"));
    }

    #[tokio::test]
    async fn record_path_overlays_the_named_document() {
        let mut doc = make_profile("download.qmd", "download.html", "Download stub");
        doc.description = Some("from the document".to_string());
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Get started")),
                ("path", s("download.qmd")),
            ])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home"), doc],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        let item = &resolved[0].items[0];
        assert_eq!(item.title, "Get started");
        assert_eq!(item.description.as_deref(), Some("from the document"));
        assert_eq!(item.target, ItemTarget::document("download.qmd", "download.html"));
        assert_eq!(item.origin, ItemOrigin::RecordOverDocument);
    }

    #[tokio::test]
    async fn record_path_resolves_against_the_host_directory() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("path", s("../rootpost.qmd"))])]),
            "sub/index.qmd",
            vec![
                make_profile("sub/index.qmd", "sub/index.html", "Sub"),
                make_profile("rootpost.qmd", "rootpost.html", "Root Post"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(resolved[0].items[0].title, "Root Post");
        assert_eq!(resolved[0].items[0].target, ItemTarget::document("rootpost.qmd", "rootpost.html"));
    }

    #[tokio::test]
    async fn record_path_to_unknown_document_warns_q_12_20_and_keeps_href() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("title", s("Typo")), ("path", s("downlaod.qmd"))])]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("guide/downlaod.qmd", "guide/downlaod.html", "Elsewhere"),
            ],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-20"]);
        let hint = format!("{:?}", diags[0]);
        assert!(hint.contains("guide/downlaod.qmd"), "did-you-mean names the same-named document: {hint}");
        assert_eq!(resolved[0].items[0].target, ItemTarget::Href("downlaod.qmd".to_string()));
        assert_eq!(resolved[0].items[0].title, "Typo");
    }

    #[tokio::test]
    async fn record_path_external_url_and_non_document_are_literal_hrefs() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![("title", s("Site")), ("path", s("https://example.com/"))]),
                map(vec![("title", s("Report")), ("path", s("files/report.pdf"))]),
            ]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(resolved[0].items[0].target, ItemTarget::Href("https://example.com/".to_string()));
        assert_eq!(resolved[0].items[1].target, ItemTarget::Href("files/report.pdf".to_string()));
    }

    #[tokio::test]
    async fn record_path_with_leading_slash_anchors_at_the_project_root() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("path", s("/rootpost.qmd"))])]),
            "sub/index.qmd",
            vec![
                make_profile("sub/index.qmd", "sub/index.html", "Sub"),
                make_profile("rootpost.qmd", "rootpost.html", "Root Post"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "a leading `/` is the project root, not a remote URL; {diags:?}");
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::document("rootpost.qmd", "rootpost.html")
        );
    }

    #[tokio::test]
    async fn record_path_escaping_the_project_warns_q_12_17() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("title", s("Out")), ("path", s("../../x.qmd"))])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-17"]);
        assert_eq!(resolved[0].items[0].target, ItemTarget::Href("../../x.qmd".to_string()));
    }

    #[tokio::test]
    async fn records_keep_their_declared_position_under_sort_false() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![("title", s("First record"))]),
                s("posts/*.qmd"),
                map(vec![("title", s("Last record"))]),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(titles(&resolved), vec!["First record", "A", "Last record"]);
    }

    #[tokio::test]
    async fn record_and_glob_naming_the_same_document_yield_two_items() {
        let (resolved, _) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![("title", s("Featured")), ("path", s("posts/a.qmd"))]),
                s("posts/*.qmd"),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert_eq!(titles(&resolved), vec!["Featured", "A"], "Q1 parity: no dedupe");
    }

    #[tokio::test]
    async fn yaml_file_entry_warns_q_12_23_and_not_q_12_19() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![s("items.yml"), s("posts/*.qmd")]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-23"]);
        assert_eq!(titles(&resolved), vec!["A"]);
    }

    #[tokio::test]
    async fn record_near_miss_and_missing_title_surface_from_generate() {
        let (_, diags) = run_transform(
            contents_listing(vec![
                map(vec![("titel", s("x"))]),
                map(vec![("description", s("no title"))]),
            ]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        let mut got = codes(&diags);
        got.sort_unstable();
        assert_eq!(got, vec!["Q-12-21", "Q-12-21", "Q-12-22"]);
    }
```

(`use crate::project::listing::{ItemOrigin, ItemTarget};` in the test imports.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(record_) | test(records_keep) | test(yaml_file_entry)'`
Expected: FAIL — items empty / codes missing.

- [ ] **Step 3: Implement.** In `config.rs` (near `flatten_content_globs`):

```rust
/// Q1's `markdownExtensions` for record `path:` values, plus
/// notebooks (a q2 project input).
pub(crate) fn is_markdown_document_path(p: &str) -> bool {
    let ext = std::path::Path::new(p.split(['?', '#']).next().unwrap_or(""))
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("qmd" | "md" | "rmd" | "ipynb"))
}
```

In `helpers.rs`, beside `is_external_src` (whose leading-`/` clause must **not** change — the hydration rebase and copy-intent depend on it):

```rust
/// True for src values that name somewhere outside this machine:
/// remote URLs, protocol-relative URLs, and `data:` URIs.
///
/// Narrower than [`is_external_src`] on purpose: a leading `/` is
/// *external* for an image `src` but *project-root-relative* for a
/// config-authored path like a listing record's `path:`
/// (`claude-notes/designs/path-resolution-model.md`).
pub(crate) fn is_remote_src(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || src.starts_with("//")
}
```

In `listing_generate.rs` imports:

```rust
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;
use crate::document_profile::DocumentProfile;
use crate::glob::{
    BaseDirContext, GlobOptions, PatternSet, has_metacharacters, join_and_normalize,
    path_to_forward_slashes,
};
use crate::project::index::ProjectIndex;
use crate::project::listing::config::is_markdown_document_path;
use crate::project::listing::helpers::is_remote_src;
use crate::project::listing::record::{overlay_record, parse_record, record_item};
use crate::project::listing::{
    ItemTarget, ListingContents, ListingItem, ResolvedListing, hydrate_item, parse_listings,
};
```

Now edit the transform. This is a **splice, not a paste**: the block below
covers `listing_generate.rs:111` (`for listing in listings {`) through the
`let mut items: Vec<ListingItem> = …;` line, and the `// ⟪UNCHANGED⟫` lines
mark runs of existing code to leave exactly as they are. One line —
`let base_ctx = …` — goes *above* the `for`, outside the loop.

```rust
        // ⟪goes immediately BEFORE `for listing in listings {`⟫
        let base_ctx = BaseDirContext {
            source_context: ctx.source_context,
            project_dir: &ctx.project.dir,
            fallback_dir: &host_dir_str,
        };

        for listing in listings {
            // Q-12-23: a literal YAML-file entry is Q1's third item
            // source (bd-hj1ehfn8), not a glob that matched nothing.
            // Partition it out so it neither resolves nor trips Q-12-19.
            let mut contents: Vec<ListingContents> = Vec::with_capacity(listing.contents.len());
            for entry in &listing.contents {
                match entry {
                    ListingContents::Glob { pattern, source }
                        if !has_metacharacters(pattern)
                            && matches!(
                                std::path::Path::new(pattern).extension().and_then(|e| e.to_str()),
                                Some("yml" | "yaml")
                            ) =>
                    {
                        diags.push(yaml_contents_unsupported(pattern, source));
                    }
                    other => contents.push(other.clone()),
                }
            }

            let resolution = resolve_content_globs(
                &contents,
                ctx.source_context,
                &ctx.project.dir,
                &host_dir_str,
            );
            // ⟪UNCHANGED⟫ the Q-12-17 loop, the `empty_set`/`patterns`
            // compile, `positives`, `positive_sets`, `matched_any`,
            // and the Q-12-18 loop — all exactly as they are today.

            // Declared-position ordering (plan §D3). A glob item keeps
            // today's key — the index of the first *positive pattern*
            // that matched it. A record's key is the number of `Glob`
            // entries declared before it, and the second tuple element
            // (0 for records, 1 for glob items) puts a record ahead of
            // the glob it was written before while leaving glob-vs-glob
            // order exactly as it is today.
            //
            // Do NOT try to recover a glob's `contents` index from its
            // `SourceInfo`: `SourceInfo::for_test()` and
            // `By::programmatic_config()` are *constant* values, so
            // unrelated entries compare equal and every glob collapses
            // to index 0 — which silently reverses
            // `contents_ordered_by_first_matching_pattern_index`.
            let mut ordered: Vec<((usize, u8), ListingItem)> = Vec::new();
            if let Some(index) = ctx.project_index.as_deref() {
                for profile in index.profiles() {
                    // ⟪UNCHANGED⟫ the `candidate_path_str` binding, the
                    // host-page skip and the `patterns.excluded(…)` skip.
                    if !item_visible(profile) {
                        continue;
                    }
                    // ⟪UNCHANGED⟫ the `first_match` / `matched_any` loop
                    // over `positive_sets`.
                    if let Some(pattern_idx) = first_match {
                        ordered.push(((pattern_idx, 1), hydrate_item(profile)));
                    }
                }
            }

            // Second item source: inline records
            // (bd-listing-inline-contents-tyy446ze, plan §D2–D4).
            // `globs_before` is the record's declared position: how
            // many `Glob` entries precede it in `contents:`.
            let mut globs_before = 0usize;
            for entry in &contents {
                let value = match entry {
                    ListingContents::Glob { .. } => {
                        globs_before += 1;
                        continue;
                    }
                    ListingContents::Inline(value) => value,
                };
                let rec = parse_record(value, &mut diags);
                let base_dir = base_ctx.base_dir_for(&rec.source);
                let item = match rec.path.clone() {
                    None => record_item(rec, ItemTarget::None, &base_dir),
                    Some((raw, path_source)) => match resolve_record_path(
                        &raw,
                        &path_source,
                        &base_ctx,
                        ctx.project_index.as_deref(),
                        &mut diags,
                    ) {
                        RecordPath::Document(profile) => {
                            if !item_visible(profile) {
                                continue;
                            }
                            overlay_record(hydrate_item(profile), rec, &base_dir)
                        }
                        RecordPath::Href(href) => record_item(rec, ItemTarget::Href(href), &base_dir),
                    },
                };
                ordered.push(((globs_before, 0), item));
            }
            ordered.sort_by_key(|(key, _)| *key);
            let mut items: Vec<ListingItem> = ordered.into_iter().map(|(_, item)| item).collect();
```

The rest of the loop (Q-12-19, filters, sort, max-items) is unchanged. Add the helpers at module level:

```rust
/// Whether a document may appear as a listing item.
///
/// Listings do not filter drafts today (Q1 does). bd-zeormbsa
/// introduces the shared `is_linkable` predicate on `ProjectIndex`;
/// this is the one seam it replaces — the glob path and the record
/// `path:` path both go through it, so the two can never disagree.
fn item_visible(_profile: &DocumentProfile) -> bool {
    true
}

enum RecordPath<'a> {
    Document(&'a DocumentProfile),
    Href(String),
}

/// Resolve a record's `path:` (Q1 `listItemFromMeta`, plan §D4).
fn resolve_record_path<'a>(
    raw: &str,
    source: &SourceInfo,
    base_ctx: &BaseDirContext<'_>,
    index: Option<&'a ProjectIndex>,
    diags: &mut Vec<DiagnosticMessage>,
) -> RecordPath<'a> {
    // Remote only — a leading `/` is the *project root* here and must
    // fall through to `join_and_normalize` (plan §D4).
    if is_remote_src(raw) {
        return RecordPath::Href(raw.to_string());
    }
    let base_dir = base_ctx.base_dir_for(source);
    let Some(resolved) = join_and_normalize(&base_dir, raw) else {
        diags.push(
            crate::glob::diagnostics::escapes_project("Q-12-17", "Listing record", raw, source)
                .build(),
        );
        return RecordPath::Href(raw.to_string());
    };
    if !is_markdown_document_path(&resolved) {
        return RecordPath::Href(raw.to_string());
    }
    match index.and_then(|i| i.lookup_by_source(std::path::Path::new(&resolved))) {
        Some(profile) => RecordPath::Document(profile),
        None => {
            diags.push(record_path_not_found(raw, &resolved, &base_dir, source, index));
            RecordPath::Href(raw.to_string())
        }
    }
}

fn record_path_not_found(
    raw: &str,
    resolved: &str,
    base_dir: &str,
    source: &SourceInfo,
    index: Option<&ProjectIndex>,
) -> DiagnosticMessage {
    let against = if base_dir.is_empty() {
        "the project root".to_string()
    } else {
        format!("`{base_dir}/`")
    };
    let mut b = DiagnosticMessageBuilder::warning(format!(
        "Listing record `path: {raw}` names no project document"
    ))
    .with_code("Q-12-20")
    .with_location(source.clone())
    .problem(format!(
        "Resolved to `{resolved}` (relative to {against}, where the listing is declared), \
         which is not a document this project renders. The item keeps the link as written, \
         so it may be broken."
    ))
    .add_info(
        "Paths resolve against the directory of the file the listing is written in; \
         a leading `/` anchors at the project root.",
    );
    let want = std::path::Path::new(resolved).file_name().and_then(|f| f.to_str());
    if let Some(candidate) = index.and_then(|i| {
        i.profiles()
            .iter()
            .find(|p| p.source_path.file_name().and_then(|f| f.to_str()) == want)
            .map(|p| path_to_forward_slashes(&p.source_path))
    }) {
        b = b.add_hint(format!("Did you mean `{candidate}`?"));
    }
    b.build()
}

fn yaml_contents_unsupported(pattern: &str, source: &SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "Listing `contents:` entry `{pattern}` is a YAML file, which is not supported yet"
    ))
    .with_code("Q-12-23")
    .with_location(source.clone())
    .problem(
        "Quarto 1 reads a YAML file in `contents:` as a list of listing records. Quarto 2 \
         does not yet (tracked as bd-hj1ehfn8); the entry is skipped.",
    )
    .add_hint("Move the records inline under `contents:` — each `- title: …` map becomes one item.")
    .build()
}
```

- [ ] **Step 4: Catalog + pages + sidebar for Q-12-20 and Q-12-23** — catalog entries (insert Q-12-20 before Q-12-21, Q-12-23 after Q-12-22):

```json
  "Q-12-20": {
    "subsystem": "listing",
    "title": "Listing Record Path Names No Document",
    "message_template": "An inline `contents:` record's `path:` has a document extension but names no document the project renders; the item keeps the link as written.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-20",
    "since_version": "99.9.9"
  },
  "Q-12-23": {
    "subsystem": "listing",
    "title": "YAML-File Listing Contents Not Yet Supported",
    "message_template": "A `contents:` entry names a YAML file. Quarto 2 does not yet read listing records from YAML files; the entry is skipped.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-23",
    "since_version": "99.9.9"
  },
```

Also extend `docs/errors/listing/Q-12-17.qmd`: it now fires for a record's
`path:` as well as a `contents:` glob. One sentence in "What this means" and
one in "How to fix" is enough — the boundary rule itself is unchanged.

Pages: Q-12-20 — what (the `path:` overlay), why (typo, wrong directory — paths resolve against the declaring file, file excluded from `project.render`), fix (correct the path, use a leading `/` to anchor at the root, or drop `path:` if the record is meant to be a plain card), example with the did-you-mean. Q-12-23 — what (Q1's YAML-file source), why (copied from Q1), fix (inline the records), Related: bd-hj1ehfn8 is *not* a user-facing reference — say "planned for a later release". Sidebar: the listing section must read `…Q-12-19, Q-12-20, Q-12-21, Q-12-22, Q-12-23` in ascending order.

- [ ] **Step 5: Gate**

Run: `cargo xtask lint --quiet && cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: PASS. Watch the three pre-existing ordering tests especially —
`contents_ordered_by_first_matching_pattern_index` (`:687`),
`item_matching_multiple_patterns_appears_once_at_first_pattern` (`:711`) and
`q_12_19_silent_when_matches_claimed_by_earlier_pattern` (`:736`): the glob
key is unchanged by design, so all three must stay green **without edits**.
If one of them flips, the ordering key is wrong — do not "fix" the test.

- [ ] **Step 6: Commit**

```bash
git add crates/quarto-core crates/quarto-error-catalog docs/errors/listing docs/_quarto.yml
git commit -m "Build listing items from inline contents records in the generate transform (bd-listing-inline-contents-tyy446ze)"
```

### Task 6: Record `path:` entries become dependency edges

**Files:**
- Modify: `crates/quarto-core/src/project/listing/config.rs:1013-1024` (`flatten_content_globs`), tests `:1801-1809`
- Modify: `crates/quarto-core/src/project/dependency_graph.rs` (tests only)
- Modify: `claude-notes/designs/document-profile-contract.md:67` (row text)

**Interfaces:**
- Consumes: `is_markdown_document_path` and `helpers::is_remote_src` (Task 5), `crate::glob::has_metacharacters`.
- Produces: `flatten_content_globs` yields a `ListingContents::Glob { pattern: <raw path>, source: <path value's SourceInfo> }` for every record with a document `path:`.

- [ ] **Step 1: Write the failing tests** — replace `extract_globs_drops_inline_records` in `config.rs` with:

```rust
    /// A record's document `path:` is a dependency edge (plan §D4):
    /// it is emitted as a literal pattern carrying the value's own
    /// provenance, leading `/` included. Pathless, remote,
    /// non-document and glob-shaped paths contribute nothing.
    #[test]
    fn extract_globs_keeps_record_document_paths_only() {
        let meta = meta_with_listing(map(vec![(
            "contents",
            arr(vec![
                map(vec![("title", s("pathless"))]),
                map(vec![("path", s("download.qmd"))]),
                map(vec![("path", s("/guide/install.qmd"))]),
                map(vec![("path", s("https://example.com/x.qmd"))]),
                map(vec![("path", s("report.pdf"))]),
                map(vec![("path", s("posts/*.qmd"))]),
                s("*.qmd"),
            ]),
        )]));
        assert_eq!(
            glob_patterns(&flatten_content_globs(&meta)),
            vec!["download.qmd", "/guide/install.qmd", "*.qmd"],
            "a leading `/` is a project-root dependency, not a remote URL"
        );
    }
```

and add to `dependency_graph.rs` tests (next to `listing_globs_become_edges_host_relative_default`):

```rust
    /// A record `path:` arrives on the profile as a literal
    /// pattern; it must become an edge and put the host in
    /// `force_render` like any glob.
    #[test]
    fn record_path_literal_becomes_edge_and_forces_render() {
        let profiles = vec![
            listing_host("index.qmd", &["download.qmd"]),
            plain_doc("download.qmd"),
            plain_doc("other.qmd"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);
        let deps = g.edges.get(Path::new("index.qmd")).expect("edge set");
        assert!(deps.contains(Path::new("download.qmd")));
        assert!(!deps.contains(Path::new("other.qmd")));
        assert_eq!(deps.len(), 1);
        assert!(g.force_render.contains(Path::new("index.qmd")));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(extract_globs_keeps_record_document_paths_only) | test(record_path_literal_becomes_edge_and_forces_render)'`
Expected: the config test FAILS, returning `vec!["*.qmd"]`.

The dep-graph test **passes immediately** and cannot fail: `listing_host()`
(`dependency_graph.rs:785-801`) builds `GlobPattern::positive(…)` directly and
never calls `flatten_content_globs`. It is a contract pin — proof that a
literal path behaves like any other resolved pattern once it reaches the
profile — not a regression guard. The config test is this task's only real
coverage; do not water it down.

- [ ] **Step 3: Implement** — `flatten_content_globs`'s tail becomes:

```rust
    listings
        .into_iter()
        .flat_map(|l| l.contents)
        .filter_map(|c| match c {
            glob @ ListingContents::Glob { .. } => Some(glob),
            ListingContents::Inline(value) => record_path_as_glob(&value),
        })
        .collect()
}

/// A record's `path:` to a project document is a dependency edge
/// (plan §D4). Emitted as a literal pattern with the value's own
/// provenance so the base-directory rule is the generate
/// transform's. Glob-shaped paths are skipped rather than compiled.
fn record_path_as_glob(value: &ConfigValue) -> Option<ListingContents> {
    let path_value = value.get("path")?;
    let raw = path_value.as_plain_text()?;
    // `is_remote_src`, not `is_external_src`: a leading `/` is the
    // project root and *is* a dependency (plan §D4). The resolver
    // re-anchors it, exactly as it does for a `/`-anchored glob.
    if super::helpers::is_remote_src(&raw)
        || crate::glob::has_metacharacters(&raw)
        || !is_markdown_document_path(&raw)
    {
        return None;
    }
    Some(ListingContents::Glob {
        pattern: raw,
        source: path_value.source_info.clone(),
    })
}
```

Update the doc comment on `flatten_content_globs`. In the contract doc's `listing_content_globs` row (`document-profile-contract.md:67`),
**first fix the stated type** — the row still says `Vec<String>`, but the field has
been `Vec<crate::glob::GlobPattern>` since v8 (`document_profile.rs:678`) — then
append: "Since bd-listing-inline-contents-tyy446ze the entries also include the
literal `path:` of each inline `contents:` record that names a project document,
so editing that document re-renders the host. Field type and
`DOCUMENT_PROFILE_VERSION` unchanged."

- [ ] **Step 4: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/quarto-core claude-notes/designs/document-profile-contract.md
git commit -m "Treat a listing record's document path as a dependency edge (bd-listing-inline-contents-tyy446ze)"
```

### Task 7: Binding — flattened `extra`, placeholder gating, absent `path`

**Files:**
- Modify: `crates/quarto-core/src/project/listing/binding.rs` — the description-envelope pair at `:411-421`, the image-envelope pair at `:427-434`, and the `extra` block at `:441-447`

**Interfaces:**
- Produces: template map has each `extra` key at top level (curated keys win) and still nested under `extra`; `description-placeholder-begin/end` and `image-placeholder-begin/end` are empty strings unless `item.origin == ItemOrigin::Document`.

- [ ] **Step 1: Write the failing tests** — in `binding.rs`'s `tests` module, which already provides `item(title) -> ListingItem` and `listing() -> Listing` and reads items back through `build_listing_context(…).get("items")` (see `item_binding_extra_passes_through_via_pampa_bridge` at `:1030` for the exact access pattern):

```rust
    /// The first item's template map from a one-item listing context.
    fn first_item_map(i: ListingItem) -> HashMap<String, TemplateValue> {
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!("items not a list")
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!("item not a map")
        };
        m.clone()
    }

    #[test]
    fn extra_keys_are_bound_flat_and_nested_with_curated_winning() {
        let mut i = item("Real title");
        i.extra.insert("link".to_string(), ConfigValue::new_string("x.html", SourceInfo::for_test()));
        i.extra.insert("title".to_string(), ConfigValue::new_string("SHADOWED", SourceInfo::for_test()));
        let m = first_item_map(i);
        assert_eq!(m.get("link"), Some(&TemplateValue::String("x.html".to_string())));
        assert_eq!(m.get("title"), Some(&TemplateValue::String("Real title".to_string())), "curated wins");
        let TemplateValue::Map(extra) = m.get("extra").unwrap() else { panic!() };
        assert_eq!(extra.get("link"), Some(&TemplateValue::String("x.html".to_string())));
    }

    #[test]
    fn record_items_get_no_l7_placeholders_and_no_path() {
        let mut i = item("Card");
        i.origin = ItemOrigin::Record;
        i.target = ItemTarget::None;
        let m = first_item_map(i);
        assert_eq!(m.get("description-placeholder-begin"), Some(&TemplateValue::String(String::new())));
        assert_eq!(m.get("image-placeholder-begin"), Some(&TemplateValue::String(String::new())));
        assert!(!m.contains_key("path"));
        assert!(!m.contains_key("outputHref"));
        assert!(!m.contains_key("filename"));
    }

    #[test]
    fn record_over_document_keeps_path_but_no_placeholders() {
        let mut i = item("Card");
        i.origin = ItemOrigin::RecordOverDocument;
        let m = first_item_map(i);
        assert!(m.contains_key("path"));
        assert_eq!(m.get("description-placeholder-begin"), Some(&TemplateValue::String(String::new())));
    }
```

(`use quarto_pandoc_types::ConfigValue; use quarto_source_map::SourceInfo; use super::super::{ItemOrigin, ItemTarget};` in the test module if not already imported. `TemplateValue` derives `Clone` — used by value throughout this file.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(extra_keys_are_bound_flat) | test(record_items_get_no_l7) | test(record_over_document_keeps_path)'`
Expected: FAIL (`link` not at top level; placeholders non-empty).

- [ ] **Step 3: Implement** — gate every placeholder insertion (`description-placeholder-begin/end` and `image-placeholder-begin/end`) with:

```rust
    // L7 placeholders only for document-origin items (plan §D6): a
    // record's description/image are final strings, and the
    // post-render substitution keys on the document's output href.
    let placeholders = item.origin == ItemOrigin::Document;
```

using `if placeholders { helpers::…(…) } else { String::new() }` for each of the four values. Replace the `extra` block with:

```rust
    // Custom fields: nested (today's `$item.extra.k$` convention) and
    // flat (`$item.k$`, Q1 parity — plan §D2), curated names winning.
    if !item.extra.is_empty() {
        let mut extra = HashMap::new();
        for (k, v) in &item.extra {
            let tv = config_value_to_template_value(v);
            m.entry(k.clone()).or_insert_with(|| tv.clone());
            extra.insert(k.clone(), tv);
        }
        m.insert("extra".to_string(), TemplateValue::Map(extra));
    }
```

(`TemplateValue` must be `Clone`; it is used by value elsewhere in this file — confirm with the compiler.) Note the ordering: this block must run **after** every curated insertion and before `show`/`table-row` so `or_insert_with` sees the curated keys.

- [ ] **Step 4: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/quarto-core/src/project/listing/binding.rs
git commit -m "Bind listing custom fields flat and skip L7 placeholders for record items (bd-listing-inline-contents-tyy446ze)"
```

### Task 8: Built-in templates render unlinked items without an anchor

**Files:**
- Modify: `crates/quarto-core/src/project/listing/templates/item-default.template:3-11`, `:15-21`
- Modify: `crates/quarto-core/src/project/listing/templates/item-grid.template:5-13`, `:17-23`, `:42-44`
- Modify: `crates/quarto-core/src/transforms/listing_render.rs` (tests)

**Interfaces:** none new. Uses the `path` absence from Task 7.

- [ ] **Step 1: Write the failing test** — in `listing_render.rs`'s tests. The module's helpers are `make_item(title: &str, date: Option<&str>) -> ListingItem`, `make_listing(kind: ListingType) -> Listing`, `empty_pandoc() -> Pandoc`, and `run_transform(ast: Pandoc, resolved: Vec<ResolvedListing>) -> (Pandoc, Vec<DiagnosticMessage>)`; existing tests inspect output through `format!("{:?}", ast)` (see `table_fields_subset_renders_single_column_without_diagnostics` at `:672`):

```rust
    #[tokio::test]
    async fn unlinked_record_item_renders_title_without_anchor() {
        let mut item = make_item("Card", None);
        item.origin = ItemOrigin::Record;
        item.target = ItemTarget::None;
        let resolved = vec![ResolvedListing {
            listing: make_listing(ListingType::Default),
            items: vec![item],
        }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(diags.is_empty(), "{diags:?}");
        let rendered = format!("{:?}", ast);
        assert!(rendered.contains("listing-title"), "title heading present: {rendered}");
        assert!(rendered.contains("Card"), "{rendered}");
        assert!(!rendered.contains("Link("), "no Link inline without a target: {rendered}");
    }

    #[tokio::test]
    async fn document_item_still_renders_title_as_link() {
        let resolved = vec![ResolvedListing {
            listing: make_listing(ListingType::Default),
            items: vec![make_item("Doc", None)],
        }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let rendered = format!("{:?}", ast);
        assert!(rendered.contains("Link("), "document items keep their anchor: {rendered}");
    }
```

(`use crate::project::listing::{ItemOrigin, ItemTarget};` in the test imports.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p quarto-core -E 'test(unlinked_record_item_renders_title_without_anchor) | test(document_item_still_renders_title_as_link)'`
Expected: the first FAILS (the template still emits `[Card]()`, a `Link(` with an empty target); the second passes and stays as the regression guard.

- [ ] **Step 3: Edit the templates.** `item-default.template` thumbnail and title/subtitle blocks:

```
$if(image-html)$
::: thumbnail
$if(path)$
[`$image-html$`{=html}]($path$){.no-external}
$else$
`$image-html$`{=html}
$endif$
:::
$else$
::: thumbnail
$if(path)$
[`$image-placeholder-begin$<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>$image-placeholder-end$`{=html}]($path$){.no-external}
$else$
`$image-placeholder-begin$<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>$image-placeholder-end$`{=html}
$endif$
:::
$endif$

::: body

$if(title)$
$if(path)$
### [$title$]($path$){.no-anchor .no-external .listing-title}
$else$
### $title$ {.no-anchor .listing-title}
$endif$
$endif$

$if(subtitle)$
$if(path)$
[$subtitle$]($path$){.no-external .listing-subtitle}
$else$
[$subtitle$]{.listing-subtitle}
$endif$
$endif$
```

`item-grid.template`: same pattern for its thumbnail (`:5-13`), title (`:18`, keep `.card-title`), subtitle (`:22`, keep `.card-subtitle`) and description link (`:43` → `$if(path)$[$description$]($path$){.no-external}$else$$description$$endif$`).

- [ ] **Step 4: Gate**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings && cargo nextest run -p quarto-core`
Expected: PASS. If any `.snap` under `crates/quarto-core` changes, inspect it: document items must render byte-identically (the `$if(path)$` branch is the old text); report any diff explicitly per CLAUDE.md's snapshot rule.

- [ ] **Step 5: Commit**

```bash
git add crates/quarto-core
git commit -m "Render unlinked listing items without an anchor in the built-in templates (bd-listing-inline-contents-tyy446ze)"
```

### Task 9: End-to-end integration tests

**Files:**
- Create: `crates/quarto-core/tests/integration/listing_inline_records.rs`
- Modify: `crates/quarto-core/tests/integration/main.rs` (add `pub mod listing_inline_records;` alphabetically)

**Interfaces:**
- Consumes: the harness shape of `listing_glob_resolution.rs:1-140` (`render_project`, `html_for`, `listing_titles`, `all_diag_codes`, `assert_no_code`) — copy those helpers verbatim into the new file (they are private to their module).

- [ ] **Step 1: Write the tests** (they fail until Tasks 5–8 are in; if you are executing in order they pass immediately — still run them):

```rust
/*
 * tests/integration/listing_inline_records.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for inline `contents:` records
 * (bd-listing-inline-contents-tyy446ze). Mirrors the fixtures in
 * `claude-notes/plans/listing-inline-contents-investigation/`.
 */

// … helpers copied from listing_glob_resolution.rs: canonical, write,
// read, render_project, html_for, listing_titles, all_diag_codes,
// assert_no_code …

const PROJECT: &str = "project:\n  type: website\nwebsite:\n  title: Inline\n";

fn stub(title: &str) -> String {
    format!("---\ntitle: \"{title}\"\n---\nStub page.\n")
}

/// `repro/`: records with `path:` — titles come from the YAML, links
/// from the documents, no Q-12-2.
#[test]
fn records_with_path_render_from_yaml_and_link_to_documents() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("download.qmd"), &stub("Download stub"));
        write(&p.join("features.qmd"), &stub("Features stub"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  type: default\n  sort: false\n  contents:\n    - title: \"Get started\"\n      description: \"Download and install Positron\"\n      path: \"download.qmd\"\n    - title: \"Explore Features\"\n      description: \"Discover key Positron features\"\n      path: \"features.qmd\"\n---\n\nBody before the listing.\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started", "Explore Features"]);
    assert!(host.contains("href=\"download.html\""), "{host}");
    assert!(host.contains("Download and install Positron"), "record description wins: {host}");
    assert_no_code(&outputs, "Q-12-2");
    assert_no_code(&outputs, "Q-12-19");
    assert_no_code(&outputs, "Q-12-20");
}

/// `mixed/` with `sort: false`: declared order, record first.
#[test]
fn mixed_record_and_glob_keep_declared_order() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("download.qmd"), &stub("Download stub"));
        write(&p.join("features.qmd"), &stub("Features stub"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  sort: false\n  contents:\n    - title: \"Get started\"\n      path: \"download.qmd\"\n    - \"features.qmd\"\n---\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started", "Features stub"]);
}

/// `linkonly/` (the Positron shape): no `path:`, custom keys, and a
/// custom doctemplate reading them flat.
///
/// Note `listing_titles` scans for the literal `listing-title">`, which
/// matches an unlinked heading only because `class` is the last attribute
/// the HTML writer emits (`pampa/src/writers/html.rs:505-532` orders
/// `id`, `class`, then key-value attrs). If a heading ever gains a kv
/// attribute this returns `[]` and the assertion below fails opaquely —
/// read the emitted HTML before assuming the feature broke.
#[test]
fn link_only_records_render_unlinked_cards_and_custom_template_reads_flat_keys() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("card.template"),
            "$for(items)$\n```{=html}\n<a class=\"card\" href=\"$items.link$\"><i class=\"$items.icon$\"></i>$items.title$</a>\n```\n$endfor$\n",
        );
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  - id: plain\n    type: default\n    contents:\n      - title: \"Get started\"\n        description: \"Download and install Positron\"\n        icon: \"bi-rocket-takeoff\"\n        link: \"https://positron.posit.co/download.html\"\n  - id: custom\n    type: custom\n    template: card.template\n    contents:\n      - title: \"Migrate from RStudio\"\n        icon: \"bi-arrow-left-right\"\n        link: \"https://positron.posit.co/rstudio-rosetta-stone.html\"\n---\n\n::: {#plain}\n:::\n\n::: {#custom}\n:::\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started"]);
    assert!(
        !host.contains("no-external listing-title"),
        "an unlinked title must not render as the anchor form: {host}"
    );
    assert!(host.contains("href=\"https://positron.posit.co/rstudio-rosetta-stone.html\""), "custom template read `$items.link$`: {host}");
    assert!(host.contains("class=\"bi-arrow-left-right\""), "custom template read `$items.icon$`: {host}");
    assert_no_code(&outputs, "Q-12-2");
    assert_no_code(&outputs, "Q-12-21");
    assert_no_code(&outputs, "Q-12-22");
}

/// A record `path:` that names no document warns Q-12-20 with a
/// did-you-mean and keeps the card.
#[test]
fn record_path_typo_warns_q_12_20_with_did_you_mean() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("guide/download.qmd"), &stub("Download"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents:\n    - title: \"Get started\"\n      path: \"download.qmd\"\n---\n",
        );
    });
    let codes = all_diag_codes(&outputs);
    assert_eq!(codes.iter().filter(|c| *c == "Q-12-20").count(), 1, "{codes:?}");
    // `LinkRewriteTransform` may additionally report the dead `.qmd`
    // link it is handed (a Q-13 code); that is expected, not a
    // double report of the same problem.
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started"]);
    let message = outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .find(|d| d.code.as_deref() == Some("Q-12-20"))
        .map(|d| format!("{d:?}"))
        .unwrap();
    assert!(message.contains("guide/download.qmd"), "did-you-mean: {message}");
}

/// A YAML-file entry warns Q-12-23, not Q-12-19.
#[test]
fn yaml_file_contents_entry_warns_q_12_23() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("items.yml"), "- title: From YAML\n");
        write(&p.join("index.qmd"), "---\ntitle: Home\nlisting:\n  contents:\n    - items.yml\n---\n");
    });
    let codes = all_diag_codes(&outputs);
    assert!(codes.iter().any(|c| c == "Q-12-23"), "{codes:?}");
    assert_no_code(&outputs, "Q-12-19");
}
```

The custom-template contract (`$for(items)$ … $items.title$ … $endfor$`, file next to the host page) is the one `listing_pipeline.rs:1247-1300` already exercises; the point of this test is that `$items.link$` / `$items.icon$` resolve **flat**. Explicit `::: {#id}` slots place each listing (`render_fills_explicit_slot`).

- [ ] **Step 2: Run**

Run: `cargo nextest run -p quarto-core -E 'binary(integration) & test(listing_inline_records::)'`
Expected: PASS (all five).

- [ ] **Step 3: Commit**

```bash
git add crates/quarto-core/tests/integration
git commit -m "End-to-end tests for inline listing records (bd-listing-inline-contents-tyy446ze)"
```

**Phase boundary:** `cargo nextest run --workspace` — report the delta vs 13130/199.

---

## Phase 4 — Retire Q-12-2, docs

### Task 10: Q-12-2 page → deprecated; listings guide page

**Files:**
- Modify: `docs/errors/listing/Q-12-2.qmd`
- Create: `docs/guides/projects/listings.qmd`
- Modify: `docs/_quarto.yml:33` (add `- guides/projects/listings.qmd` after `paths.qmd`)

- [ ] **Step 1: Deprecate the Q-12-2 page.** Front matter `status: deprecated`, and rewrite the now-false `title:`/`description:` (currently "Inline Listing Contents Not Yet Supported" / "…are not yet supported and the entry is skipped") — e.g. `title: "Inline Listing Contents Not Yet Supported (retired)"`, `description: "Older Quarto 2 releases skipped inline contents records; current releases render them."` The `error-docs-*` lints allow a page title to differ from the catalog title, so nothing else catches this. Replace the body's "not yet implemented" prose: "Quarto 2 releases after 0.26 support inline records and no longer emit this code. If you see it, you are on an older release — upgrade, or move each record to a file until you can." Keep the catalog entry untouched (append-only rule).

- [ ] **Step 2: Write `docs/guides/projects/listings.qmd`** (user-facing, usage not internals), sections:

```markdown
---
title: "Listings"
description: "Generate lists of pages or hand-written entries with `listing:`."
---

## Contents

`contents:` names what a listing shows. Each entry is either a glob that
selects project documents, or an inline record — a small map that *is* an
item.

### Globs

(three-line summary; link to the rules on the Q-12-19 error page: patterns
resolve against the file they are written in, `*` is one level, leading `/`
is the project root.)

### Records

```yaml
listing:
  id: cards
  type: grid
  contents:
    - title: "Get started"
      description: "Download and install"
      image: images/rocket.png
    - title: "Release notes"
      path: https://example.com/releases
```

A record's `title`, `subtitle`, `description`, `author`, `date`,
`date-modified`, `image`, `image-alt`, `categories`, `order` and `path`
are the same fields a document supplies. Any other key becomes a custom
field a custom template can read as `$item.key$` (also `$item.extra.key$`).
Relative `image:` and `path:` values resolve against the file the listing
is written in.

### Records that point at a document

`path: guide/install.qmd` merges the record over that document's own
entry: the record's fields win, the document supplies the rest (link,
description, date, …). A `path:` that is an external URL or a
non-document file is used as the link as written.

### Order

Items are sorted by the listing's `sort:`. With `sort: false`, they appear
in the order of the `contents:` entries — records included. (Quarto 1
placed all records after all glob matches.)

### Not yet supported

YAML files in `contents:` (Quarto 1's `contents: items.yml`) are skipped
with a warning ([`Q-12-23`](/docs/errors/listing/Q-12-23.qmd)); move the
records inline.
```

- [ ] **Step 3: Verify the docs build with Q2** (never Q1):

Run: `cargo run --bin q2 -- render docs/ 2>&1 | grep -E "Q-1|Error|Rendered" | head`
Expected: `Rendered N of N files`, no new diagnostics on `guides/projects/listings.qmd` or the error pages; open `docs/_site/guides/projects/listings.html` and confirm the page and sidebar entry.

- [ ] **Step 4: Lint + commit**

```bash
cargo xtask lint --quiet
git add docs
git commit -m "Document inline listing records and deprecate the Q-12-2 page (bd-listing-inline-contents-tyy446ze)"
```

---

## Phase 5 — Verification and wrap-up

### Task 11: Full verification and end-to-end record

- [ ] **Step 1: Workspace tests**

Run: `cargo nextest run --workspace`
Expected: 0 failures; record `passed/skipped` and the delta against 13130/199 in this file's §"End-to-end record".

- [ ] **Step 2: Full verify (WASM leg included)**

Run: `cargo xtask verify > /tmp/verify.log 2>&1; tail -3 /tmp/verify.log`
Expected: `exit=0`, all 14 steps ✓. (Inspect with grep; do not pipe nextest through `tail`.)

- [ ] **Step 3: Real-binary check on the four fixtures** — from the repo root:

```bash
cargo build --bin q2
for f in control repro mixed linkonly; do
  echo "== $f"; (cd claude-notes/plans/listing-inline-contents-investigation/$f && ../../../../target/debug/q2 render 2>&1 | grep -E "Q-12|Rendered"; grep -o 'listing-title[^>]*>[^<]*' _site/index.html)
done
```

Expected: control unchanged (2 items, 0 warnings); repro 2 items titled from YAML, `href="download.html"`/`features.html`, **0 warnings**; mixed 2 items; linkonly 2 items with no `<a` around the titles, 0 warnings. Paste the observed output into §"End-to-end record" and delete the generated `_site`/`.quarto` dirs (the investigation `.gitignore` covers them).

- [ ] **Step 4: Positron smoke (local-only repo, best effort)** — `cd /Users/gordon/src/q2-positron-docs/docs-quarto-2 && <repo>/target/debug/q2 render 2>&1 | grep -c "Q-12-2"` should print `0`; the welcome grid containers should now carry items (card markup still raw EJS until bd-oywyaouf). Record the count; if the repo is absent, say so.

- [ ] **Step 5: Reconcile this plan's checklist** against what landed (confirm each `- [x]`), commit the plan, then follow `superpowers:finishing-a-development-branch`. Before that: `braid comment bd-bqf2 "…both Inline arms (parse_contents / flatten_content_globs) now agree on records — unify with the walker"`; **Do not push without approval.**

## Follow-ups (all filed; all out of this plan's scope)

- bd-0mggxqx5 — `field-required` enforcement for all item kinds (Q1 `validateItem`), parsed at `config.rs:492`, never enforced. Filed 2026-08-24.
- bd-hj1ehfn8 (YAML-file item source) — already filed.
- bd-zeormbsa (drafts) will replace `item_visible`.
- bd-0t4e07jk may flip documents to `UnknownKeyPolicy::IntoExtra` — one-line change after Task 2.

## End-to-end record

*(filled in by Task 11)*

Pre-fix baseline at `596ceb572` is in §"Investigation record".
