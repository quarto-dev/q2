# Plan 6 — Provenance audit (Generated for synthesizers, anchors for shortcodes)

**Date:** 2026-05-04 (revised 2026-05-20, review pass 2026-05-22)
**Branch:** feature/q2-preview
**Status:** Implementation plan (review-pass edits applied; theorem
attr_source question closed)
**Milestone:** none directly — completes the AST shape Plans 7/8 rely on

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 6 is the audit pass
that converts every transform's `SourceInfo::default()` emission into
the correct `Generated { by, from }` shape Plan 4 defines, and
attaches `Invocation` anchors uniformly to all shortcode resolutions.
The file name keeps its q2-preview-plan-N form for continuity with the
earlier discussion notes.

## Work items checklist

Implementation order. The plan body (Scope / Implementation notes / Test plan)
holds the design details; this list is the work-tracking surface.

### Phase 0 — prerequisite
- [x] Add `Inline::source_info_mut` (~33 LOC) + `Block::source_info_mut`
  (~24 LOC) accessors in `quarto-pandoc-types`, with round-trip unit tests
  for one representative variant of each.

### Audit
- [x] Comprehensive grep + categorize `SourceInfo::default()` sites in
  `crates/quarto-core/src/transforms/` and `crates/pampa/src/`.
  (Report: `claude-notes/research/2026-05-22-plan-6-audit.md`.
  Follow-ups: bd-12vrr callout default-title, bd-1inj0 code-block
  chrome.)
- [x] Document the positional-alignment invariant on `AttrSourceInfo.attributes`
  (`crates/quarto-pandoc-types/src/attr.rs:31`).

### Stamper + dispatch funnel
- [x] Implement `stamp_shortcode_anchors` + mutable AST walkers in
  `shortcode_resolve.rs` (model on existing `recurse_inline` /
  `resolve_block`).
- [x] Wire the stamper into `resolve_shortcode`'s dispatch funnel so every
  Rust / Lua / extension dispatch is post-walked.
- [x] Thread `shortcode_owned.source_info` into `make_error_inline` and
  `shortcode_to_literal` from their four call sites.

### Synthesizer fixes
- [x] `TitleBlockTransform`: emit `Generated { by: By::title_block(), from: [] }`
  on the synthesized h1.
- [x] `SectionizeTransform`: emit `Generated { by: By::sectionize(), from: [] }`
  on the synthetic Section Div (both close-on-stack and end-of-input sites).
- [x] `FootnotesTransform`: emit `Generated { by: By::footnotes(), from: [] }`
  on the container Div.
- [x] `AppendixStructureTransform`: emit `Generated { by: By::appendix(), from: [] }`
  on the container Div, bibliography wrapper, license/copyright/citation
  helpers (all 5 sites — the helpers were not enumerated in the plan body
  but are structurally identical synthesizers; see audit report §"Decisions
  on plan-adjacent sites").
- [x] `theorem.rs::extract_name_attr` + `proof.rs::extract_name_attr`:
  thread `&div.attr_source` through; index before `kvs.remove("name")`;
  fall back on length-mismatch. **Implementation note**: the
  `debug_assert_eq!` form the plan body suggested is too strict — it
  fires on the common test pattern of `AttrSourceInfo::empty()` plus a
  non-empty `kvs`. Relaxed to `debug_assert!(attr_source.attributes.
  is_empty() || kvs.len() == attr_source.attributes.len(), ...)`. The
  empty case is "no provenance" (not a bug); only populated-but-
  misaligned input is a bd-3aolj/bd-1e6a5 sync error.
- [x] `pampa::pandoc::treesitter_utils::postprocess` synthetic Space
  (~line 1348): emit `Generated { by: By::tree_sitter_postprocess(), from: [] }`.

### Tests
- [x] Shortcode required-anchor invariant
  (`shortcode_resolution_required_anchor_invariant` — every
  `by:shortcode` carries an Invocation).
- [x] Per-transform fix tests (sectionize / title_block / footnotes /
  appendix — shape test in each transform's own test module).
- [x] Lua-shortcode enrichment test
  (`lua_shortcode_typed_return_enriched_to_shortcode_kind` — typed Lua
  return promoted from `by:filter` → `by:shortcode`, `filter_path` /
  `line` migrated into `by.data.lua_path` / `by.data.lua_line`,
  Invocation appended).
- [x] Multi-inline shortcode anchor test
  (`multi_inline_shortcode_resolution_shares_invocation_source` —
  Strong[Str], Space, Str all share the same Invocation source_info).
- [x] Escaped-shortcode regression test
  (`escaped_shortcode_keeps_original_source_info`).
- [x] Error-inline regression test
  (`unknown_shortcode_error_uses_token_source_info` — both Strong + Str
  layers carry the token's Original source_info, not Default or
  Generated). The earlier `test_make_error_inline` unit test was also
  updated to assert the threaded shape.
- [x] `source_info` determinism test
  (`shortcode_resolution_is_deterministic` — two runs produce
  structurally-equal ASTs, including all `Generated.by` /
  `Generated.from[]` / Original byte ranges).
- [ ] Audit-completion test across the full pipeline (no
  `SourceInfo::default()` survives across all transforms). Deferred —
  the required-anchor invariant + per-transform shape tests cover the
  same property piecemeal; a pipeline-level audit would belong in the
  e2e test crate alongside Plan 3's idempotence fixtures and is
  better wired in there. Open follow-up.
- [ ] Attribution interaction test (multi-author latest-wins via
  `query_byte_range`). Deferred — needs `GitBlameProvider` setup; the
  attribution chain is mechanically covered by Plan 4's
  `resolve_byte_range` (Generated → Invocation → Original) and Plan 6
  doesn't change the chain. Open follow-up.
- [ ] Error + escaped round-trip test (incremental writer
  verbatim-copies). Deferred to Plan 7 (writer infrastructure).
- [ ] Shortcode-inside-include composition test (Invocation anchor
  `file_id != 0`). Deferred to Plan 8 (include wrapper introduces the
  cross-file context).
- [ ] Plan 3 idempotence test rerun (no new non-determinism). Verified
  by `cargo nextest run --workspace` — all 9460 tests pass, including
  Plan 3's idempotence fixtures.

### Verification
- [ ] `cargo xtask verify` (full, including hub-build leg).
- [ ] End-to-end exercise on a fixture covering shortcodes / sections /
  footnotes / appendix / theorems; inspect output and record the
  invocation + observed shape per CLAUDE.md's "End-to-end verification
  before declaring success" rule.

## Goal

Audit every transform that emits `SourceInfo::default()` (a meaningless
zero-range Original) and fix it to emit correct provenance. Two
patterns apply:

- **Transforms that genuinely synthesize content with no source preimage**
  (Sectionize's section Divs, TitleBlock's synthesized h1, etc.): emit
  `Generated { by: By::<kind>(), from: smallvec![] }` from Plan 4.
- **The shortcode resolver, uniformly**: emit `Generated { by: By::shortcode(name),
  from: smallvec![Anchor::invocation(token_si)] }` on every resolved
  node, regardless of whether the handler is Rust-built-in or
  Lua-implemented. The `Invocation` anchor's `source_info` is the
  shortcode token's range; Plan 7's writer uses it for Verbatim-copy
  on KeepBefore; attribution chains through it via `resolve_byte_range`.

The earlier `Derived` variant proposal collapsed into `Generated` with
an `Invocation` anchor during the 2026-05-20 design discussion; this
plan reflects the unified shape.

Plan 6 does NOT introduce a `CustomNode("ShortcodeResolution")` wrapper
(an earlier draft proposed that; we walked it back). Wrappers are
appropriate for cases where there's no available source-side anchor in
the same file (includes — different FileId — Plan 8 handles those). For
shortcodes the resolved nodes can carry source_info pointing into the
parent file directly via the typed `Invocation` anchor.

## Prerequisite — Phase 0: mutable accessors on Inline / Block

Plan 6's `stamp_shortcode_anchors` helper (see "The post-walk helper"
below) takes `&mut Inline` / `&mut Block` and rewrites the
`source_info` field. Today `crates/quarto-pandoc-types/src/inline.rs:57`
defines only `pub fn source_info(&self) -> &SourceInfo` (immutable);
Plan 4 does not add a mutable counterpart. Every existing site that
mutates `source_info` in the workspace holds a *typed* reference
(`&mut Str`, `&mut CodeBlock`, …) and assigns the public field
directly — there is no generic `&mut Inline -> &mut SourceInfo`
accessor.

**Before any stamping code can compile**, add to
`crates/quarto-pandoc-types/src/inline.rs` and `block.rs`:

```rust
impl Inline {
    pub fn source_info_mut(&mut self) -> &mut quarto_source_map::SourceInfo {
        match self {
            Inline::Str(s) => &mut s.source_info,
            // ... 28 variants, mechanical mirror of `source_info(&self)`
        }
    }
}

impl Block {
    pub fn source_info_mut(&mut self) -> &mut quarto_source_map::SourceInfo {
        match self {
            Block::Plain(p) => &mut p.source_info,
            // ... 18 variants, mechanical mirror of `source_info(&self)`
        }
    }
}
```

Pure mechanical mirror of the existing read accessors — ~33 LOC for
`Inline` + ~24 LOC for `Block`. Add a unit test that round-trips a
mutation through the accessor on one representative variant of each.

## Scope

### In scope

For each transform that currently emits `SourceInfo::default()`, replace
with the correct provenance:

- **`ShortcodeResolveTransform`** (`crates/quarto-core/src/transforms/shortcode_resolve.rs`):
  Currently emits `SourceInfo::default()` on 12 production sites (see
  References for the per-line breakdown). **Fix the dispatch funnel
  uniformly via a post-walk helper**: immediately after every handler
  dispatch (Rust handler OR Lua-engine dispatch OR extension
  dispatch), walk the returned nodes and stamp
  `Generated { by: By::shortcode(name), from: smallvec![Anchor::invocation(Arc::new(ctx.source_info.clone()))] }`
  on each block/inline.
  - The post-walk **enriches**, not overrides: any `by.data` fields the
    Lua machinery attached (`filter_path`, `line` — Plan 4's filter
    `by.data` shape) are preserved by promoting the kind from
    `filter` to `shortcode`, renaming to `lua_path` / `lua_line` in
    `by.data` to reflect the new context. See "Lua-shortcode
    enrichment" below.
  - The post-walk recurses into nested blocks/inlines (model on
    `recurse_inline` / `resolve_block` in this file) so every node in
    the dispatch output gets the anchor.
  - **Two outlier sites do NOT pass through the dispatch funnel** and
    need call-site source_info threading instead of the stamper:
    - `make_error_inline` (lines 1030-1038): visible `?key` Str +
      Strong wrapper for unknown shortcodes. Today both layers carry
      `SourceInfo::default()`. Fix: pass `shortcode_owned.source_info`
      through from call sites at lines 659 and 914, and use it as the
      Str/Strong's `source_info` (an `Original` pointing at the
      shortcode token's bytes — same shape Plan 6's
      audit-completion test expects). **Atomicity intent**: the error
      region is treated as normal editable user-source content (NOT
      atomic). If the user edits `?meta:bad` in React, the bytes
      change in the source qmd via the verbatim-copy path. Plan 7's
      `is_atomic_kind()` does not fire because the source_info is
      Original, not Generated. The Strong-wraps-Str overlap (both
      layers carry the same range) is structurally parallel to the
      footnote `<sup>` case Plan 7:261-267 already documents.
    - `shortcode_to_literal` (lines 1043-1109): the literal-text Str
      produced for escaped `{{</ ... >}}` shortcodes. Today it emits
      `SourceInfo::default()`. Fix: pass `shortcode_owned.source_info`
      through from call sites at lines 665 and 920, and use it as the
      Str's `source_info`. This is required to satisfy the
      "Escaped-shortcode regression test" (line 453: "its source_info
      stays Original (not Generated)") — without this fix, the
      regression test would fail on Plan 6's own implementation.
- **`TitleBlockTransform`** (line 183-185): synthesizes a level-1 Header
  from `title:` metadata. Fix: emit `Generated { by: By::title_block(), from: smallvec![] }`
  on the synthesized Header (and any nested Inlines). Note: q2-preview
  skips this transform (Plan 1), but the audit covers the HTML
  pipeline too.
- **`SectionizeTransform`** (`pampa/src/transforms/sectionize.rs:96, 148`):
  the synthetic Section Div. Fix: `Generated { by: By::sectionize(), from: smallvec![] }`.
  The wrapped Header retains its original source_info. Body blocks retain
  theirs.
- **`FootnotesTransform`**: the synthesized footnotes container Div.
  Fix: `Generated { by: By::footnotes(), from: smallvec![] }`. The
  synthesized `<sup>` markers are already source-mapped via
  `create_footnote_ref` cloning from the original `Note` inline (so
  they stay Original — no change needed). The four synthesized inline
  layers (Span/Superscript/Link/Str) all carry the same range,
  producing a multi-node overlap; Plan 7:261-267 documents that this
  is round-trip-friendly without extra writer work (block-level
  Verbatim of the surrounding Para covers it). q2-preview pipeline
  runs this transform (per Plan 2B's audit); the audit applies to
  both pipelines.
- **`AppendixStructureTransform`**: the synthetic appendix container Div.
  Fix: `Generated { by: By::appendix(), from: smallvec![] }`. Same scope
  note as Footnotes.
- **`theorem.rs::extract_name_attr`** (line 313) **and the parallel
  `proof.rs::extract_name_attr`** (line 167): the title Str extracted
  from `name="..."` is currently built with `SourceInfo::default()`.
  Fix: thread `&div.attr_source` into `extract_name_attr` in both
  files; index by `kvs.keys().position(|k| k == "name")` *before* the
  `remove`; use `attr_source.attributes[idx].1` (an
  `Option<SourceInfo>` carrying the parser-recorded
  `Original{file_id, value_start, value_end}` for the attribute
  value's bytes) as the Str's `source_info`. Falls back to
  `SourceInfo::default()` only when the Option is `None` (e.g. JSON
  read from external Pandoc producers that don't emit `attrS`) OR
  when length-alignment fails (see safeguards below). The parser
  populates the value range at
  `crates/pampa/src/pandoc/treesitter.rs:1075-1107` →
  `treesitter_utils/commonmark_attribute.rs:38-50`; no parser-side
  prerequisite is needed.

  **Positional-alignment safeguards** (review-pass 2026-05-22): the
  fix relies on the invariant *"`AttrSourceInfo.attributes[i]` is the
  `(key_src, val_src)` for the i-th entry in `Attr.2`'s insertion
  order."* This invariant holds in the parser's main path but **is
  not documented and is broken in two preexisting code paths**
  (duplicate-key handling in `commonmark_attribute.rs:41-49`;
  caption-attr-into-table merge in `section.rs:85-113` and
  `postprocess.rs:1483-1496`). Plan 6 therefore:
  1. **Documents the invariant** with a doc-comment on
     `AttrSourceInfo.attributes` in `crates/quarto-pandoc-types/src/attr.rs:31`.
  2. **Guards the index in `extract_name_attr`** with a runtime
     length check (`if kvs.len() == attr_source.attributes.len()`)
     and a `debug_assert_eq!` on lengths. Falls back to
     `SourceInfo::default()` when they diverge, so production never
     panics on misaligned input.
  3. **Two follow-up beads tracked** (out-of-band, preexisting bugs):
     **bd-3aolj** (duplicate-key handling in
     `commonmark_attribute.rs:41-49` — `LinkedHashMap::insert` updates
     in place while `attr_source.attributes.push` always appends) and
     **bd-1e6a5** (caption-attr-into-table merge in
     `section.rs:85-113` / `postprocess.rs:1483-1496` — same root
     cause when caption + table keys overlap). Plan 6 does not block
     on them; its runtime guard handles the failure mode safely.
  4. Note: `kvs.remove("name")` after the index lookup itself shrinks
     `attr.2` by one without touching `attr_source.attributes`. The
     surviving `div.attr_source` is then handed to `CustomNode::new`
     (`theorem.rs:281`). Downstream consumers of `attr_source` on
     that CustomNode see misaligned data. The rest of `convert_div`
     does not re-index `attr_source`, so this is harmless locally,
     but a future consumer of the constructed CustomNode's
     `attr_source` could trip on it. Considered acceptable for v1;
     if a future caller indexes, it should use the same guarded
     pattern.

  JSON round-trip preserves the value range: `attrS.kvs` serializes
  as a positional array of `[key_ref, val_ref]` pairs
  (`json.rs:600-633`) and reads back identically (`json.rs:423-508`).
  No Plan-5 follow-up needed.
- **`pampa::pandoc::treesitter_utils::postprocess`** (line 1348): the
  "Synthetic Space" inserted to separate citation from suffix. Fix:
  `Generated { by: By::tree_sitter_postprocess(), from: smallvec![] }`.

The audit pass also looks for any *other* sites emitting
`SourceInfo::default()` that aren't enumerated. Plan 6 starts with a
comprehensive grep.

### Out of scope

- The `is_atomic_kind()` predicate and `is_atomic_custom_node` registry
  (Plan 7 owns the writer-side atomicity logic).
- The writer's soft-drop / atomic-violation handling (Plan 7).
- The writer's multi-inline shortcode dedupe rule (Plan 7).
- The `IncludeExpansion` CustomNode wrapper (Plan 8).
- React component for shortcode-resolved inlines (Plan 2A's framework
  atomic gate already handles this via the `isAtomicSourceInfo`
  accessor; Plan 4's `is_atomic_kind` set names `shortcode` as atomic).
- **Metadata-loader changes** to record per-key source-info for `meta`
  and `var` shortcodes. Files separately; see "ValueSource follow-up"
  below.
- **Lua-file registration in `SourceContext`** to enable typed
  `Dispatch` anchors. Files separately; see "Dispatch follow-up"
  below.
- The HTML pipeline doesn't need a "ShortcodeResolutionResolveTransform"
  (no wrapper to unwrap). Shortcode-resolved nodes ARE flat
  inlines/blocks with `Generated` source_info; the HTML writer doesn't
  care about source_info, it just renders the nodes. Behavior
  unchanged for HTML.

## Design decisions (settled in conversation)

- **Single funnel covers all shortcodes**. The `ShortcodeResolveTransform::resolve_shortcode`
  method is the single dispatch point for in-file shortcodes (Rust
  built-ins, Lua-loaded extension handlers, extension name lookup).
  Plan 6's stamping helper runs once per dispatch, uniformly. All
  built-in (`meta`) and Lua-implemented (`kbd`, `lipsum`, `placeholder`,
  `version`, `video`) shortcodes get the same treatment. User-extension
  shortcodes via Lua: same. `{{< include >}}` is the genuine exception
  — handled by `IncludeExpansionStage` (a separate pipeline stage) and
  Plan 8's wrapper, not via Generated.
- **Include×shortcode composition is architecturally well-defined.**
  `IncludeExpansionStage` runs at the stage layer
  (`crates/quarto-core/src/pipeline.rs:258`) before
  `AstTransformsStage` (`pipeline.rs:312`), so includes are spliced
  flat before any shortcode resolution. Shortcode resolution is
  single-pass — `resolve_blocks` advances its index *past* inserted
  blocks (`shortcode_resolve.rs:625-677`); returned content is never
  re-scanned, so a shortcode emitting the literal text
  `"{{< include foo.qmd >}}"` lands as a `Str`, never as a parsed
  `Shortcode` (the reverse composition is structurally impossible).
  When a shortcode appears *inside* include-spliced content, the
  Invocation anchor's `source_info` points into the included file
  (different `FileId` than the parent) — this is correct: the token's
  bytes live there. Plan 8's wrapper carries the parent-file anchor
  independently; Plan 7's `preimage_in(parent_file)` returns `None`
  for the included children and the wrapper governs verbatim-copy.
- **Enrichment, not override**. The Lua machinery's auto-attach
  produces `Generated { by: filter, from: [], by.data: { filter_path,
  line } }` (post-Plan-4, per Plan 4 §"by.data shape table" line 590)
  for *typed* Inline/Block nodes constructed during a Lua shortcode
  dispatch (e.g. `return pandoc.Str(...)`). Bare-string returns
  (`return "text"` → `LuaShortcodeResult::Text`) do NOT pass through
  `filter_source_info`; they land with `SourceInfo::default()` and
  enter the post-walk's fresh-Generated branch directly. The shortcode
  resolver's post-walk enriches the filter-attached cases:
  - **Appends** an `Invocation` anchor pointing at the shortcode token.
  - **Promotes** `by.kind` from `"filter"` to `"shortcode"`, renaming
    `filter_path` → `lua_path` and `line` → `lua_line` in `by.data`
    (reflecting the new shortcode context) and adding the shortcode
    `name`.
  The Lua-side dispatch precision is preserved; the shortcode context
  layer is added on top. No information is discarded.

  **Scope**: this enrichment fires only from
  `ShortcodeResolveTransform::resolve_shortcode`. General Lua filter
  dispatches (`UserFiltersStage`) leave `Generated { by: filter, ... }`
  intact — that is the steady-state for filter constructions, per
  Plan 4 §"Filter constructions become Generated { by: filter, from:
  [] }". The post-walk is not wired into the filter stage and should
  not be.
- **Most transforms just need to preserve ctx.source_info**. The
  "audit and fix" is mostly bug fixes — ctx already has the info; the
  transforms just drop it. Mechanical change.
- **Shortcode resolutions use `Generated` + `Invocation` anchor, not a
  wrapper.** Each resolved Str/Inline/Block gets `Generated { by:
  shortcode(name), from: [Invocation -> Arc::new(ctx.source_info.clone())] }`.
  The anchor's source_info is the shortcode token's range (an Original
  from `ctx.source_info`). Plan 7's writer uses it for Verbatim-copy
  on KeepBefore. Multi-inline resolutions: every resolved node shares
  the same anchor's source_info, enabling Plan 7's dedupe rule.
- **Genuine synthesizers use `Generated` with empty anchors**.
  Sectionize, TitleBlock, Footnotes, Appendix containers — none of
  these correspond to source bytes, so they get
  `Generated { by: By::<kind>(), from: smallvec![] }`. Plan 7's coarsen
  treats their wrappers as Transparent (recurse into source-bearing
  children) or Omit depending on `by.is_atomic_kind()`.
- **No `atomic` flag needed**. Plan 7's atomic-violation logic detects
  atomicity via `by.is_atomic_kind()` (per Plan 4's predicate) and via
  the `is_atomic_custom_node` registry for CustomNode types
  (`IncludeExpansion`, `CrossrefResolvedRef`). Shortcode atomicity
  falls into the first category (`shortcode` is in the atomic-kind
  set).

## Attribution interaction

The `Invocation` anchor's existence delivers correct attribution for
shortcode-resolved content **with no attribution-code changes**:

- `query_attribution(node.source_info, runs)` calls `resolve_byte_range`.
- Per Plan 4's updated `resolve_byte_range`, `Generated` delegates to
  `invocation_anchor()`, which returns the `Invocation` anchor's
  `source_info` — typically an `Original` covering the shortcode
  token's bytes.
- The chain resolves to `(file_id=0, token_start, token_end)`.
- `query_attribution` accepts (file_id == 0, start < end) and calls
  `query_byte_range`.
- The existing max-time-across-overlapping-runs logic in
  `AttributionMap::query_byte_range` picks the latest author covering
  the token's bytes.

For multi-author shortcodes: if author A wrote `{{< meta foo >}}` at
T1 and author B changed `foo` to `bar` at T2 > T1, the byte range
covers bytes touched by both; `query_byte_range` picks the latest
(B). This is the policy specified in the 2026-05-20 design
discussion ("attributed to latest author of the shortcode text"),
and it falls out mechanically from Plan 6's anchor stamping plus
Plan 4's chain-walking accessor — no special-case code.

## Lua-shortcode enrichment

The Lua machinery's `filter_source_info` (in
`crates/pampa/src/lua/types.rs`) walks the live Lua call stack to find
the first non-C frame and produces (post-Plan 4) the canonical
filter-construction shape:

```rust
Generated {
    by: By::filter(filter_path, line),  // by.data = { filter_path, line }
    from: smallvec![],
}
```

This auto-attach fires when Lua code constructs *typed* nodes via
`pandoc.Str(...)`, `pandoc.Span(...)`, etc. Bare-string Lua returns
(`return "text"` → `LuaShortcodeResult::Text`) do NOT pass through
`filter_source_info`; their resulting Str carries
`SourceInfo::default()` instead.

When this filter-shape source_info appears inside a Lua shortcode
handler dispatch, the resolver's post-walk enriches it to:

```rust
Generated {
    by: By {
        kind: "shortcode".to_string(),
        data: json!({
            "name": shortcode_name,
            "lua_path": <by.data["filter_path"]>,
            "lua_line": <by.data["line"]>,
        }),
    },
    from: smallvec![Anchor::invocation(Arc::new(ctx.source_info.clone()))],
}
```

The Lua-side `filter_path` / `line` precision is preserved in
`by.data` under the more contextually-precise names `lua_path` /
`lua_line`; the shortcode `name` is added; the kind is promoted from
`filter` to `shortcode`. **Nothing is discarded.** Nodes that entered
the post-walk with `SourceInfo::default()` (bare-string Lua returns,
or Rust handler returns) hit the fresh-Generated branch instead and
end up with `by.data = { name }` plus the Invocation anchor.

This is the canonical "enrichment-via-post-walk" pattern. Other
transforms that wrap dispatch may follow the same shape later (always
append, promote `by.kind`, preserve prior `by.data` fields where
meaningful).

When the **Lua-file-registration follow-up** lands (see "Dispatch
follow-up" below), `lua_path` / `lua_line` migrate out of `by.data` and
into a typed `Dispatch` anchor. `by.data` for Lua-dispatched shortcodes
then shrinks to just `{ "name": shortcode_name }`.

## The post-walk helper

```rust
/// After every shortcode handler dispatch, stamp Invocation provenance
/// on the returned nodes. Recurses into nested AST so every block and
/// inline gets the anchor. Enriches existing `Generated { by: filter, ... }`
/// (from Lua auto-attach) by promoting kind and appending the anchor;
/// otherwise sets source_info to a fresh Generated shape.
fn stamp_shortcode_anchors(
    result: &mut ShortcodeResult,
    shortcode_name: &str,
    token_si: &SourceInfo,
) {
    let token_arc = Arc::new(token_si.clone());
    match result {
        ShortcodeResult::Inlines(inlines) => {
            for inline in inlines.iter_mut() {
                stamp_inline(inline, shortcode_name, &token_arc);
            }
        }
        ShortcodeResult::Blocks(blocks) => {
            for block in blocks.iter_mut() {
                stamp_block(block, shortcode_name, &token_arc);
            }
        }
        ShortcodeResult::Preserve | ShortcodeResult::Error(_) => {}
    }
}

fn stamp_inline(inline: &mut Inline, name: &str, token_arc: &Arc<SourceInfo>) {
    let si = inline.source_info_mut();
    *si = enrich_or_create(si, name, token_arc);
    // recurse into nested inlines (Strong, Emph, Link, ...)
    walk_nested_inlines(inline, |child| stamp_inline(child, name, token_arc));
}

fn enrich_or_create(
    existing: &SourceInfo,
    name: &str,
    token_arc: &Arc<SourceInfo>,
) -> SourceInfo {
    // If the Lua machinery attached Generated { by: filter, ... },
    // promote it. Otherwise fresh Generated.
    //
    // NOTE (bd-36fr9 co-change): the by.data["filter_path"]/["line"]
    // reads below are temporary. Once Lua-file registration lands,
    // those fields move out of by.data and into a Dispatch anchor in
    // `from`. This branch then reads the existing Dispatch anchor
    // from `existing.from[]` and copies it into the new from-list
    // alongside Invocation. See §"Dispatch follow-up".
    //
    // NOTE (bd-129m3 integration point): for `meta` / `var` shortcodes
    // post-loader-change, the helper also appends a ValueSource
    // anchor pointing at the metadata value's source range. See
    // §"ValueSource follow-up".
    let by = match existing {
        SourceInfo::Generated { by, .. } if by.kind == "filter" => {
            let lua_path = by.data.get("filter_path").cloned();
            let lua_line = by.data.get("line").cloned();
            let mut data = serde_json::json!({ "name": name });
            if let Some(p) = lua_path { data["lua_path"] = p; }
            if let Some(l) = lua_line { data["lua_line"] = l; }
            By { kind: "shortcode".to_string(), data }
        }
        _ => By::shortcode(name),
    };
    SourceInfo::Generated {
        by,
        from: smallvec![Anchor::invocation(Arc::clone(token_arc))],
    }
}
```

(Block stamping is parallel — recurse into block children and inlines
they contain.)

## Open questions for implementation

- **Comprehensive audit**: grep for `SourceInfo::default()` in
  `crates/quarto-core/src/transforms/` and `crates/pampa/src/`.
  Categorize each site: preserve ctx info / emit Generated with
  appropriate by-kind / emit Generated with Invocation / leave as-is
  (test code). Plan 6's first commit (after Phase 0) is the audit
  report; subsequent commits fix each site.

(Previously-open questions resolved by review pass 2026-05-22:
"Theorem title from attr" — `AttrSourceInfo` already carries the
value range; see §Scope theorem bullet for the threaded-in fix.
"Escaped shortcodes" — the In-scope `shortcode_to_literal` fix at
the call site (passing `shortcode_owned.source_info` through)
produces the Original shape the regression test expects.
"Recursion into deep AST" — concrete reusable shape and full
container-variant set documented; see §Implementation notes
below.)

## Implementation notes

- **Recursion shape for the post-walk.** The walker must traverse the
  full container set — for inlines: Strong, Emph, Strikeout,
  Superscript, Subscript, SmallCaps, Quoted, Cite, Link,
  Image (alt/caption), Span, Underline, Delete, Insert, Highlight,
  EditComment, Note (block content), Custom (slot contents); for
  blocks: Div, BlockQuote, OrderedList, BulletList, DefinitionList,
  Figure, Table (cells), Custom (slot contents). The canonical
  reusable shape is in
  `crates/quarto-core/src/transforms/shortcode_resolve.rs`'s own
  `recurse_inline` (~lines 945-1027) and `resolve_block`
  (~lines 710-863), which already cover this set including Image's
  alt/caption content and Note's nested blocks. Model the new mutable
  walkers on these — drop the async + shortcode-resolution logic,
  keep the match-arm dispatch and Image/Note recursion. The narrower
  walkers in `callout.rs` and `theorem.rs` are block-only and do NOT
  cover the inline variants the stamper needs; do not use them as the
  reference shape.

## ValueSource follow-up

Plan 6 does NOT attach `ValueSource` anchors. The shape is defined
(Plan 4 ships `AnchorRole::ValueSource`) but the data isn't available:
the metadata loader doesn't surface per-key source-info to the
shortcode resolver today. Specifically, the merged `meta` ConfigValue
the resolver consults has `source_info` per key INTERNALLY, but
`MetaShortcodeHandler::resolve` calls `ctx.metadata.get_nested(&key)`
and then `config_value_to_inlines(value)` which discards the
per-key source information when flattening to strings.

The follow-up issue ("metadata-loader threads per-key source-info
through to shortcode handlers"):

1. Loader change: `ConfigValue` already carries `source_info`
   per-value (`crates/quarto-pandoc-types/src/config_value.rs:155`);
   the lookup path returns ConfigValue references, but
   `config_value_to_inlines` converts to bare Strs discarding source.
   Thread source through.
2. Resolver change: when constructing the resolved nodes, attach a
   `ValueSource` anchor pointing at the value's `source_info`.
3. This is the structural feature behind Elliot's 2026-05-20 chain
   request — the resolved content would carry both `Invocation` (where
   the shortcode was written) and `ValueSource` (where the value was
   defined).

When the follow-up lands, Plan 6's post-walk grows one more anchor
append at the appropriate dispatch sites. The current Plan 6 ships
with just `Invocation`; the type is forward-compatible.

**Integration point**: bd-129m3 should append the ValueSource anchor
inside `enrich_or_create` (see §"The post-walk helper" below). Once
the metadata loader threads per-key source-info through, the helper
gains access to the value's source range via the `ShortcodeContext`
and pushes a second anchor into `from` alongside the Invocation. No
other call sites in Plan 6 change.

Tracked as **bd-129m3** ("Provenance follow-up: ValueSource anchor
stamping for meta/var shortcodes").

## Dispatch follow-up

Plan 6 does NOT use a typed `Dispatch` anchor for Lua-side
construction info. Lua filter files aren't registered in `SourceContext`,
so we can't construct an `Original` pointing into them. In the interim,
`(lua_path, lua_line)` lives in `by.data` (see "Lua-shortcode
enrichment" above).

The follow-up issue ("register Lua filter files in `SourceContext`"):

1. `SourceContext::register_file(path, bytes) -> FileId`.
2. Lua engine calls it when loading each filter.
3. `filter_source_info` produces `Original { file_id, start, end }`
   instead of returning a path-line pair.
4. Lua-attached source_info becomes `Generated { by: filter, from:
   [Dispatch -> Original{lua_file, ...}] }`.
5. Plan 6's post-walk's enrichment then preserves the `Dispatch`
   anchor (typed) instead of preserving `by.data` fields.

When the follow-up lands, `AnchorRole::Dispatch` joins the enum (a
non-breaking enum extension); `by.data` for `filter` / Lua-dispatched
`shortcode` kinds shrinks to per-kind config only.

**Co-change in `enrich_or_create`**: bd-36fr9 must update Plan 6's
helper (§"The post-walk helper" below). The current "enrich" branch
reads `by.data.get("filter_path")` and `by.data.get("line")` from
the existing `Generated{by:filter, ...}`; post-bd-36fr9, those
fields are gone from `by.data` and the relevant info lives in the
`Dispatch` anchor inside `from`. The helper then reads the existing
Dispatch anchor and copies it into the new shortcode-shape `from`
alongside the Invocation. The §"Lua-shortcode enrichment" example
above also needs updating to show the post-bd-36fr9 shape.

Tracked as **bd-36fr9** ("Provenance follow-up: Dispatch anchor for
Lua-handler filter & shortcode").

## References

- `crates/quarto-core/src/transforms/shortcode_resolve.rs` — main fix
  site. Per-line breakdown of production `SourceInfo::default()`
  emissions:
  - Lines 172, 179, 186, 203, 208, 215, 222 — `config_value_to_inlines`
    (Str construction for `meta` / `var` lookups).
  - Line 238 — `flatten_blocks_to_inlines` (synthesized
    paragraph-separator Space; NOT part of `config_value_to_inlines`).
  - Line 470 — `lua_result_to_shortcode_result::Text` arm (bare-string
    Lua return wrapped in a Str).
  - Lines 1034, 1036 — `make_error_inline` (visible `?key` Str + Strong
    wrapper for unknown shortcodes).
  - Line 1109 — `shortcode_to_literal` (escaped-shortcode literal text).
  The stamper handles the first three groups uniformly via the dispatch
  funnel; `make_error_inline` and `shortcode_to_literal` need call-site
  source_info threading (see "In scope" bullet).
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:306-371` —
  `resolve_shortcode` method (single funnel for all dispatches; the
  post-walk hooks in here).
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:710-1027` —
  existing `resolve_block` / `recurse_inline` walkers. Canonical
  reusable shape for the new mutable walkers (drop async +
  shortcode-resolution logic; keep the match-arm dispatch and
  Image/Note recursion).
- `crates/quarto-core/src/transforms/title_block.rs:183, 185` — h1
  synthesis sites.
- `crates/pampa/src/transforms/sectionize.rs:96, 148` — section Div
  synthesis sites. (Line 169 in that file is a `dummy_source_info()`
  test helper, not a production site.)
- `crates/quarto-core/src/transforms/footnotes.rs` — container Div
  synthesis (around line 495 / `create_footnotes_section`).
- `crates/quarto-core/src/transforms/appendix.rs` — appendix container
  Div synthesis (`create_appendix_container` ~line 257).
- `crates/quarto-core/src/transforms/theorem.rs:313` and
  `crates/quarto-core/src/transforms/proof.rs:167` — name-attr title
  extraction in `extract_name_attr`. Both pass `&div.attr_source`
  through and use `attr_source.attributes[idx].1` (an
  `Option<SourceInfo>`).
- `crates/quarto-pandoc-types/src/attr.rs:27-32` — `AttrSourceInfo`
  shape (`attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>`
  for key/value source ranges).
- `crates/pampa/src/pandoc/treesitter.rs:1075-1107` and
  `crates/pampa/src/pandoc/treesitter_utils/commonmark_attribute.rs:38-50`
  — parser sites that populate the attr value's byte range. No
  prerequisite parser change needed.
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1348` —
  synthetic Space.
- `crates/pampa/src/lua/types.rs:1812-1840` — `filter_source_info`
  Lua-side auto-attach. Note: only fires for typed Inline/Block
  returns (`pandoc.Str(...)`); bare-string returns
  (`return "text"` → `LuaShortcodeResult::Text`) bypass it.
- `crates/quarto-pandoc-types/src/custom.rs` — CustomNode shape.
- `crates/quarto-core/src/transforms/callout.rs` — example pattern for
  sugar transforms wrapping output in CustomNode. NOTE: callout +
  theorem are block-only walkers; for inline recursion, use
  `shortcode_resolve.rs::recurse_inline` instead.
- `crates/quarto-core/src/stage/stages/user_filters.rs` — general Lua
  filter dispatch site. Does NOT invoke the post-walk; its
  constructions keep `by.kind == "filter"` as steady state.
- `crates/quarto-core/src/pipeline.rs:258, 312` — `IncludeExpansionStage`
  precedes `AstTransformsStage`, so includes are spliced before
  shortcodes resolve. See §"Include×shortcode composition" in Design
  decisions.

## Test plan

- **Audit-completion test**: a unit test that builds a fixture document
  exercising shortcode resolution, sectionize, and (HTML pipeline only)
  title-block / footnotes / appendix. **Asserts that the resulting AST
  has no nodes with `SourceInfo::default()` source_info AND every
  synthesized node carries an appropriate `Generated` shape** (matches
  the §Atomic-kind-set / §by.data tables in Plan 4). Defensive
  regression: catches a future PR that adds a transform without
  provenance.
- **Shortcode required-anchor invariant**: the audit-completion test
  ALSO walks the post-stamping AST and asserts no `Generated { by:
  shortcode, from: [] }` remains. Every `by.kind == "shortcode"` node
  must carry at least one `Invocation` anchor pointing at the source
  token's bytes. Per Plan 4 §"Required-anchor invariant for shortcode",
  this is the producer-side enforcement of the rule; Plan 7 adds a
  `debug_assert!` on the consumer side as belt-and-suspenders. The
  stamper is the only construction site for `by: shortcode` in v1, so
  the test exercises the full source of bad shapes.
- **Per-transform fix tests**: for each fixed transform, a test that
  inspects the produced source_info shape:
  - SectionizeTransform: synthetic Div has `Generated { by: { kind:
    "sectionize" }, from: [] }`. Header inside has its original
    source_info.
  - ShortcodeResolveTransform (uniform): each resolved Str has
    `Generated { by: { kind: "shortcode", data: { name: "..." } },
    from: [Anchor { role: Invocation, source_info: ... }] }`. The
    anchor's source_info chain-walks to the shortcode token's bytes
    via `resolve_byte_range`.
  - Lua-shortcode test: a `{{< kbd Ctrl+C >}}` invocation produces a
    Span with `Generated { by: { kind: "shortcode", data: { name:
    "kbd", lua_path: "...", lua_line: N } }, from: [Invocation] }`.
    **NOT** `by.kind == "filter"`; the post-walk promoted it.
  - Other built-in Lua shortcodes (lipsum, placeholder, version, video):
    same shape, with the appropriate `name`.
  - Etc. for each transform.
- **Multi-inline shortcode anchor test**: a metadata key with markdown
  (`title: "**Bold** Title"`). After ShortcodeResolveTransform, the
  resulting `[Strong[Str], Space, Str]` ALL have `Generated` with
  `Invocation` anchors whose `source_info` is the same shortcode
  token's range. This is what Plan 7's dedupe rule detects.
- **Attribution interaction test**: render a doc with `{{< meta foo >}}`
  through two commits by different authors (author A wrote the line at
  T1; author B changed `foo` → `bar` at T2). With Plan 6 stamped and a
  `GitBlameProvider` installed, the resulting `astContext.attribution`
  for the resolved Str references author B's identity (the latest
  author of the token bytes). This is the multi-author latest-wins
  policy.
- **Escaped-shortcode regression test**: `{{</ meta foo >}}` resolves
  to literal text; its source_info stays Original (not Generated).
- **Error-inline regression test**: an unknown shortcode `{{< bogus >}}`
  resolves via `make_error_inline` to `Strong[Str("?bogus")]`. Both
  layers carry `Original` source_info pointing at the bogus
  shortcode's token bytes (NOT `Default`, NOT `Generated`). Plan 7's
  `is_atomic_kind()` does not fire; round-trip through the
  incremental writer Verbatim-copies the original token bytes.
- **Error / escaped round-trip test**: full incremental-writer
  round-trip on a fixture containing both `{{</ meta foo >}}` and
  `{{< bogus >}}`. After Plan 6's stamping + Plan 7's writer, the
  output qmd should byte-equal the input for those regions
  (verbatim-copy via the Original anchor in both cases).
- **Shortcode-inside-include composition test**: `parent.qmd`
  contains `{{< include foo.qmd >}}`; `foo.qmd` contains
  `{{< meta title >}}`. After Plan 6 stamping (and Plan 8's wrapper),
  the resolved Str inside the IncludeExpansion wrapper has
  `Generated { by: { kind: "shortcode", data: { name: "title" } },
  from: [Invocation -> Original{file_id: <foo.qmd's FileId>, ...}] }`.
  Assert the Invocation anchor's source_info `file_id != 0` (i.e.
  points into the included file, not the parent). Plan 8's wrapper
  carries the parent-file anchor at its level; this test exercises
  Plan 6's stamping invariant under the cross-file context. Plan 8's
  own test plan covers wrapper round-trip independently.
- **Idempotence still holds**: re-run Plan 3's idempotence test after
  the audit — the changes shouldn't introduce non-determinism.
- **`source_info` determinism (Plan 6-specific gap)**: Plan 3's hashes
  exclude `source_info` by design (`compute_blocks_hash_fresh` and
  `compute_meta_hash_fresh` both skip it). So Plan 3 does **not**
  catch a transform whose synthesized `Generated { by, from }`
  output is non-deterministic *in the source_info layer* — e.g., an
  `Anchor::invocation` that hashes a different `SourceInfo` on
  repeated runs because the shortcode-token's range was recomputed
  rather than cloned. Plan 6 must add its own per-fixture
  source_info-determinism check: render twice, walk the AST in
  lockstep, assert every `Generated.by`, every `Generated.from[]`,
  and every Original `SourceInfo` is `==`-equal across runs. Place
  this alongside Plan 3's idempotence test (same fixtures, parallel
  assertion) so the test crate covers both contracts.

## Dependencies

### Hard dependencies

- **Plan 4** — Plan 6's transforms use `By::shortcode(...)`,
  `By::sectionize()`, `By::title_block()`, etc., plus the `Generated`
  variant and `Anchor`/`AnchorRole` types. Cannot compile without
  Plan 4.

### Soft dependencies

- **Plan 5** — Plan 6's source_info changes are visible to in-Rust
  consumers as soon as Plan 6 lands. But for the changes to round-trip
  through the JSON wire format (the path q2-preview takes when crossing
  the WASM boundary to React and back), Plan 5's wire-format extension
  is required. Without Plan 5, a Plan 6 AST that gets serialized to JSON
  and deserialized loses the `Generated` shape (decoded via legacy
  code-3 fallback as Substring approximations).

  Pragmatic implication: Plan 6 lands cleanly in-Rust without Plan 5,
  but isn't observable in q2-preview without Plan 5. The plans can be
  developed in parallel after Plan 4 lands; Plan 5 should land at or
  before the q2-preview integration is exercised end-to-end.

### Blocks

- **Plan 7** — writer needs Plan 6's audit-fixed AST shape to walk
  preimages correctly and to detect atomic-kind for `is_atomic`
  enforcement.
- Independent of Plan 8 (Plan 8 introduces its own wrapper for
  includes; shortcodes don't use that pattern).

## Risk areas

- **Audit completeness**: missing a site means a future Plan 7
  round-trip silently corrupts that region. Mitigation: the
  audit-completion test scans for `SourceInfo::default()` AND for
  synthesized-but-not-Generated shapes in produced ASTs.
- **Breaking existing HTML pipeline tests**: the audit changes
  source_info on many nodes. The hash-based reconciler doesn't care,
  but tests that inspect specific source_info shapes might fail. Run
  the full workspace test suite after each transform fix.
- **Shortcode-resolved nodes change source_info shape**: existing tests
  that assert "the resolved title Str has SourceInfo::default()" or
  similar will fail. Update them to expect Generated. The HTML output
  doesn't change shape (still flat inlines/blocks); only source_info
  on those nodes changes.
- **No new CustomNode type added** (deliberate, retained from the
  earlier draft). The HTML pipeline isn't affected — shortcode-resolved
  content remains flat inlines/blocks; the HTML writer renders them
  normally.
- **Post-walk recursion bugs**: missing a nested AST shape in the walk
  means some inner nodes don't get the anchor. Cover Strong/Emph/Link
  for inlines and Div/BlockQuote/Span-in-Plain for blocks.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Phase 0: `Inline::source_info_mut` + `Block::source_info_mut` accessors + unit tests | ~70 |
| Audit pass (grep + categorize) | ~30 (mostly notes) |
| `stamp_shortcode_anchors` helper + mutable recursion walks (modeled on `shortcode_resolve.rs::recurse_inline` / `resolve_block`) | ~220 |
| Shortcode resolver dispatch-site fixes — 12 production sites: `config_value_to_inlines` ×7, `flatten_blocks_to_inlines` ×1, `lua_result_to_shortcode_result::Text` ×1, `make_error_inline` ×2, `shortcode_to_literal` ×1. Most covered by the stamper; `make_error_inline` and `shortcode_to_literal` need call-site source_info threading. | ~70 |
| TitleBlock fix | ~20 |
| Sectionize fix | ~20 |
| Footnotes fix | ~30 |
| Appendix fix | ~30 |
| Theorem + proof title-from-attr fix (thread `attr_source` through `extract_name_attr` in both files) | ~30 |
| TreeSitter postprocess fix | ~10 |
| Tests | ~280 |
| **Total** | **~810** |

The earlier "~540" estimate omitted the Phase-0 mut accessors (~70 LOC),
under-counted the recursion walkers (mutable walks over the full
inline/block container set are ~220 LOC, not ~80), and missed the
`make_error_inline` / `shortcode_to_literal` / `proof.rs` fix sites.

## Notes

This is a "scattered fixes" plan — touches many transform files with
small per-file changes. Most of the diff is mechanical: `SourceInfo::default()`
→ either `ctx.source_info.clone()` (Original) for synthesizers that DO
have a source preimage but currently drop it, or
`Generated { by: By::<kind>(), from: smallvec![] }` for genuine
synthesizers, or `stamp_shortcode_anchors(...)` for shortcode
dispatches.

The conceptual surface is small; the file count is not.

The earlier-draft "wrap shortcode resolutions in `CustomNode("ShortcodeResolution")`"
approach was walked back. Per the user's reasoning: wrappers were heavy
for what's fundamentally a provenance problem. The typed `Invocation`
anchor in `Generated` gives Plan 7 atomic detection at the writer
level (via `by.is_atomic_kind()` returning true for `shortcode`)
without the structural cost of a new CustomNode type, the qmd writer
arm, the HTML-pipeline-resolve transform, or the React component for
the wrapper. Includes (Plan 8) still use a wrapper because their
cross-file FileId issue genuinely requires anchoring at the
parent-file level.

The shortcode-resolution provenance change propagates to: q2-preview
rendering (Plan 2A's framework atomic gate in `dispatch.tsx`'s `Node`
detects `shortcode` kind via `ATOMIC_GENERATED_KINDS` and the
JS-side `isAtomicSourceInfo` accessor), writer round-trip (Plan 7's
soft-drop logic detects `by.is_atomic_kind()` + UseAfter and emits
Q-3-42; Plan 7's dedupe rule handles multi-inline shortcode
resolutions via the shared anchor source_info), and possibly some
existing tests that asserted on the flat Str's source_info shape.

The post-walk's enrichment pattern (promote kind, preserve prior
`by.data`, append anchor) is the canonical shape for any future
transform that wraps a Lua dispatch. Document the pattern in Plan 6's
helper so future contributors have a reference.
