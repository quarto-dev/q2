# DocumentProfile: comment summary for downstream tooling (GH #445)

**Strand:** bd-0rsk07il
**GH issue:** https://github.com/quarto-dev/q2/issues/445
**Status:** reviewed 2026-08-25 (open questions resolved — see
§"Resolved decisions"); awaiting explicit go-ahead to execute.

## Overview

Editorial comments are `[>> comment text ]` marks in qmd source, parsed
into `Inline::EditComment` nodes (`crates/quarto-pandoc-types/src/inline.rs:325`:
`attr: Attr`, `content: Inlines`, `source_info`, `attr_source`). In the
q2-preview AST JSON they surface as `Span`s with class
`quarto-edit-comment`, which
`ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`
extracts and renders as per-block bubbles. The comment display mode
toggle (expand / show / hide) lives in
`hub-client/src/components/ReplayDrawer.tsx` (`CommentsModeToggle`).

GH #445 asks: teach Quarto 2's Pass-1 processing (the
`DocumentProfile`) to summarize the comments present in a document, so
UI that wants "are there comments? how many?" doesn't have to process
the whole document — and so *other* documents' comment states are
knowable without rendering them (Pass-1 profiles exist for every
project file).

**First consumer:** hub-client's comment-mode toggle gets a badge/pill
with the count of outstanding comments on the active page.

## Facts established during investigation

- **Representation at the checkpoint (execution finding, 2026-08-25):**
  the qmd reader's postprocess step rewrites every `Inline::EditComment`
  into an `Inline::Span` whose classes start with `quarto-edit-comment`
  (id and kvs preserved, content trimmed —
  `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`,
  `.with_edit_comment`). The qmd writer maps that span form back to
  `[>> …]` decorated syntax only when it has no id/kvs and exactly the
  one class (`write_span`, `crates/pampa/src/writers/qmd.rs:1954`);
  otherwise it writes `[…]{.quarto-edit-comment …}` — which re-parses
  to the same span shape. So the **class-based span is the canonical
  AST form**; the extractor keys on the class (with a defensive
  `Inline::EditComment` arm since the type still exists pre-postprocess).
- The profile checkpoint (`DocumentProfileStage`, between
  `MetadataMergeStage` and `PreEngineSugaringStage`) sees the AST
  **after include expansion** and **before any AST mutation** —
  `EditComment` nodes are parse products, so they are all present at
  the checkpoint. Comments inside included files count toward the
  including document (consistent with how `outline` works).
- Comments attached to code blocks are stored by the hub UI as `[>> ]`
  paragraphs inside a wrapper Div (`quarto-edit-comment-container`) —
  still ordinary `EditComment` inlines, so a plain AST walk finds them.
- Comments carry **no author/date today**: `CommentBlock.tsx`'s
  `addComment` writes a bare span (empty attr). Author dots in the UI
  come from the automerge **attribution overlay**, not from source.
  `EditComment.attr` (id/classes/kvs) exists and round-trips through
  the qmd writer (`write_editcomment`), so author/date *could* be
  persisted later without a parser change.
- "Outstanding" = present in source. The ✓ resolve button deletes the
  comment from the source; there is no resolved-but-kept state.
- The WASM render path (`render_page_in_project*` in
  `crates/wasm-quarto-hub-client/src/lib.rs`) runs Pass-1 over every
  project file and returns a `RenderResponse` JSON to hub-client. The
  response has no profile-derived fields today.
- `LinkResolutionStage`
  (`crates/quarto-core/src/stage/stages/link_resolution.rs`) already
  implements the manual block/inline walk pattern (it matches
  `Inline::EditComment` explicitly); the comment scan follows the same
  pattern.

## Design decisions (proposed — please review)

### D1. Profile field: entries, not just a count

```rust
/// One editorial comment found in the document body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileComment {
    /// Plain-text projection of the comment's content
    /// (`pampa::writers::plaintext::inlines_to_string`).
    pub text: String,
    /// Source span of the mark, for jump-to-comment / gutter markers.
    pub source: quarto_source_map::SourceInfo,
    /// In-band author, from the mark's `author=` attribute
    /// (`[>> text ]{author="…"}`). `None` when unstamped — all
    /// hub-authored comments today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// In-band timestamp, from the mark's `date=` attribute. Kept as
    /// the raw string (same policy as `DocumentProfile::date`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Remaining attr kvs passthrough (anything other than
    /// `author`/`date`), so future conventions need no shape change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<(String, String)>,
}

/// On DocumentProfile:
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub comments: Vec<ProfileComment>,
```

Count is `comments.len()` — no separate count field to keep in sync.
Carrying entries (text + span) rather than a bare count is what makes
the follow-on use cases (jump-to-comment, tooltips, MCP listing,
search) possible without another version bump. Comments are short;
profile size impact is negligible.

Rejected alternative: `comment_count: u32` only. Cheaper, but every
listed use case beyond the badge would immediately need a second bump.

### D2. Version bump 12 → 13

Strictly, a `#[serde(default)]` field is additive (cf. `order`, added
without a bump). But profiles are **cached** (Phase-8 incremental
rebuilds): a cached v12 profile would deserialize cleanly and report
"no comments" for a document that has them — a silent semantic misread,
exactly what the version-bump rule exists for. Bump, and add the v13
entry to the version-history doc comment and the contract doc.

### D3. Extraction in pure `DocumentProfile::extract`

A `count_comments(blocks)`-style walk (module-private, mirroring
`extract_outline`) over the body: recurse through block/inline
containers collecting `Inline::EditComment`. No stage side-channel
needed — comments are in the AST the extractor already receives.
Walk must cover comments nested inside other inlines (Emph, Span,
etc.) and inside container blocks (Div, BlockQuote, lists, tables,
figure captions) — reuse the traversal shape of
`LinkResolutionStage`.

Deliberately **not** included: comments in YAML front matter (not a
thing), comments in non-body metadata inlines. Document order =
source order (walk order).

### D4. Surfacing to hub-client: active-page summary on `RenderResponse`

Add to `RenderResponse` (wasm lib.rs):

```rust
#[serde(skip_serializing_if = "Option::is_none")]
comments: Option<JsonCommentsSummary>,  // { count, entries: [{ text, line?, ... }] }
```

populated on both the project-active-page branch (from the Pass-1
profile of the active page) and the single-doc branch (the profile is
produced mid-pipeline; Phase 2 resolves the cleanest retention point —
likely stashing the extracted profile, or just its comment vec, on
`StageContext` the way other cross-stage artifacts travel).
Hub-client threads it from the render result through `ReactPreview` up
to `Editor` via a callback (same pattern as `onDiagnosticsChange`),
into `ReplayDrawer`.

**Deferred (follow-up strand, filed at execution time):** a
project-wide `path → comment summary` map in the response, for
file-sidebar per-file badges. Pass-1 already computes everything;
this is pure surface area, but it doubles the response-plumbing scope
and the first consumer doesn't need it.

### D5. Authorship: interpret in-band attrs now; stamp them as follow-up

The issue mentions authors and last-comment-date. Hub-authored marks
carry neither today (authorship comes from the attribution overlay),
but **authorship should have an in-band representation in the qmd
itself** — a comment's author shouldn't be recoverable only through
automerge history. So:

- **In scope here (read side):** the profile *interprets* `author=`
  and `date=` attributes on comment marks as first-class
  `ProfileComment` fields (D1). The convention
  `[>> text ]{author="…" date="…"}` is thereby defined and honored by
  Pass-1 from day one, and "last comment date" becomes derivable the
  moment marks are stamped.
- **Follow-up strand (write side):** teach the hub add-comment path
  (`CommentBlock.tsx` `addComment`) to stamp `author=`/`date=` from
  the session identity when writing the span, plus how the bubbles
  display in-band authors vs. attribution-derived ones (in-band should
  win when present). Filed at execution time, linked to this strand —
  it's a hub-client product/UX change with its own review surface
  (source noise, identity naming), and the badge doesn't depend on it.
- The attribution-overlay join stays available for unstamped legacy
  comments; the profile's `source` span is the join key.

## Other use cases this unlocks (for discussion)

- **File-sidebar badges**: per-file outstanding-comment counts across
  the project, from the Pass-1 index — no rendering of inactive files.
  (The deferred half of D4.)
- **CLI report**: `q2 comments` (or `q2 inspect`) listing outstanding
  comments across a project with file:line — review-round tooling.
- **Publish/render gate**: warn (or `--fail-on-comments`) when
  rendering/publishing a document that still has outstanding comments,
  analogous in spirit to `draft`.
- **MCP surface**: a quarto-hub-mcp tool listing outstanding comments
  so agents can run "address every open comment" triage loops.
- **Hub search**: comment text as a searchable facet
  (`hub-client/src/services/search/` already consumes profile data
  for titles).
- **Editor affordances**: gutter/scrollbar comment markers in Monaco
  from the profile's source spans, without an extra parse.
- **Notifications/presence**: "N new comments since you last looked" —
  client-side diff of successive profile summaries.

None of these are in scope here; each would be its own strand once the
profile field exists.

## Phases and work items

### Phase 1 — Rust core (TDD) — **done 2026-08-25**

- [x] Tests first: 5 unit tests in `document_profile.rs` (empty +
      JSON omission; paragraph comment w/ span-slices-source check;
      all container kinds in source order; author/date/kv attrs both
      syntaxes; JSON round-trip) + 1 pipeline integration test
      (`profile_sees_comments_from_included_file` in
      `document_profile_pipeline.rs`). Verified failing first
      (E0609 on the missing field).
- [x] `ProfileComment` + `comments` field + `CommentCollector` walk
      (mirrors `LinkResolutionStage`'s traversal; comment spans are
      leaves). Execution finding recorded in §Facts: at the checkpoint
      comments are `Span`s with class `quarto-edit-comment` (reader
      postprocess rewrites `EditComment`), so the walk keys on the
      class with a defensive `EditComment` arm.
- [x] `DOCUMENT_PROFILE_VERSION` 12 → 13 (+ history comment); contract
      doc updated (header version tag, guarantees row, change log).
      Cache invalidation is automatic — the version is in the
      cache-key hash domain (`project/cache_key.rs`).
- [x] `cargo nextest run --workspace`: 13395 passed. Clippy clean on
      quarto-core.

### Phase 2 — WASM / response surface — **done 2026-08-25**

- [x] Retention point: `UnwrapProfileStage` now **moves** the profile
      onto `StageContext.document_profile` instead of discarding it
      (zero-copy; runs after `LinkResolutionStage`, so the stash is
      complete). `run_pipeline` bridges it to
      `RenderContext.document_profile`; both Pass-2 renderers copy it
      onto `WasmPassTwoOutput.document_profile` (mirroring
      `theme_fingerprint`). Verified the q2-preview pipeline keeps
      both checkpoint stages (`Q2_PREVIEW_STAGE_EXCLUDED` excludes
      only math-js / render-html-body / apply-template).
- [x] `RenderResponse.comments: Option<Vec<JsonComment>>` populated in
      both single-doc and project-active branches (all five
      construction sites); `None` ≡ zero for consumers.
- [x] `JsonComment` transport type + `ProfileComment::to_json` live in
      quarto-core (natively testable; 1-based Monaco positions, end
      fallback mirroring `diagnostic_to_json`, `file` field for
      include-mapped comments). TDD: 4 new tests verified failing
      first (`render_qmd_to_html_bridges_document_profile_to_ctx`,
      `active_page_profile_comments_on_{html,preview}_output`,
      `profile_comment_to_json_positions_and_fields`).
- [x] TS types: `RenderComment` + `RenderResponse.comments` in
      `ts-packages/preview-renderer/src/types/diagnostic.ts` — the
      single definition preview-runtime and hub-client both import.
- [x] Workspace green (13399); `npm run build:wasm` compiles the
      wasm-side changes cleanly; clippy clean.

### Phase 3 — hub-client badge (first consumer) — **done 2026-08-25**

- [x] Threading: `ReactPreview` reports `RenderResponse.comments` via
      a new `onCommentsChange` prop after each successful
      preview-pipeline render (wire-absent → `[]`; parse-only formats
      and failures preserve last-good, matching the AST/fingerprint
      semantics) → `PreviewRouter` passthrough → `Editor` keeps
      `outstandingCommentCount` state → `ReplayDrawer.commentsCount`.
- [x] Badge: one `comments-toggle-badge` pill on the
      `CommentsModeToggle` group (both drawer states), hidden at 0,
      with singular/plural aria-label. TDD:
      `ReplayDrawer.commentsBadge.test.tsx` (4 tests) verified
      failing first.
- [x] `npm run build:all` (strict tsc -b) ✓; `npm run test` 1005 ✓;
      `npm run test:ci` / `test:wasm` 133 ✓ against the rebuilt WASM.
- [ ] `hub-client/changelog.md` two-commit dance (second commit needs
      the first's hash).

### Phase 4 — verification & wrap-up

- [ ] Full `cargo xtask verify` (WASM leg affected — quarto-core
      change).
- [ ] End-to-end: real browser session against local hub
      (`npm run local-prod` or dev server), a fixture doc with
      comments; observe the badge count, add a comment, watch it
      increment; record invocation + observation per CLAUDE.md's
      end-to-end policy.
- [ ] File follow-up strands, linked `discovered-from:bd-0rsk07il`:
      in-band author/date stamping in the hub add-comment path (D5
      write side); project-wide summary map for sidebar badges (D4
      deferral); any of the use-case list the user wants tracked.
- [ ] Close bd-0rsk07il; comment on GH #445.

## Resolved decisions (plan review, 2026-08-25)

1. **D1 scope — entries + spans, confirmed.** Size: cache bloat is
   expected to be dominated by non-text input, not comment text.
   Privacy: the profile adds no information not already present in the
   document itself, so no *new* exposure; existing safeguards apply.
   Verified in-tree: the native profile cache lives at
   `<project>/.quarto/cache/`
   (`crates/quarto-core/src/project/profile_cache.rs`, via
   `NativeRuntime::with_cache_dir`), and `q2 create`'s git scaffolding
   ensures `/.quarto/` is in `.gitignore`
   (`crates/quarto/src/commands/create/project.rs:151-153`). Projects
   assembled by hand without that ignore entry could commit cached
   profiles — but the source document carrying the same text is
   committed regardless, so this changes nothing.
2. **D4 — active-page-only for the first cut, confirmed.** The
   project-wide `path → summary` map stays a follow-up strand.
3. **Badge placement — single location** (one badge for the toggle
   group, not one per button). Final design to be iterated on a
   working version; implementation uses best judgment for the first
   screenshot.
4. **D5 convention — confirmed:** `author=` / `date=` attribute
   names; `date` is ISO 8601 UTC; the hub stamps the **display name**
   as the identity string for now (expected to be tweaked once seen in
   action). Write-side stamping is a **follow-up strand**, not Phase 3.
