# Plan 6 — Provenance audit (Generated for synthesizers, anchors for shortcodes)

**Date:** 2026-05-04 (revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — completes the AST shape Plans 7/8 rely on

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 6 is the audit pass
that converts every transform's `SourceInfo::default()` emission into
the correct `Generated { by, anchors }` shape Plan 4 defines, and
attaches `Invocation` anchors uniformly to all shortcode resolutions.
The file name keeps its q2-preview-plan-N form for continuity with the
earlier discussion notes.

## Goal

Audit every transform that emits `SourceInfo::default()` (a meaningless
zero-range Original) and fix it to emit correct provenance. Two
patterns apply:

- **Transforms that genuinely synthesize content with no source preimage**
  (Sectionize's section Divs, TitleBlock's synthesized h1, etc.): emit
  `Generated { by: By::<kind>(), anchors: vec![] }` from Plan 4.
- **The shortcode resolver, uniformly**: emit `Generated { by: By::shortcode(name),
  anchors: vec![Anchor::invocation(token_si)] }` on every resolved
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

## Scope

### In scope

For each transform that currently emits `SourceInfo::default()`, replace
with the correct provenance:

- **`ShortcodeResolveTransform`** (`crates/quarto-core/src/transforms/shortcode_resolve.rs`):
  Currently emits `SourceInfo::default()` on every resolved
  Str/Inline (~12 sites). **Fix uniformly via a post-walk helper**:
  immediately after every handler dispatch (Rust handler OR Lua-engine
  dispatch OR extension dispatch), walk the returned nodes and stamp
  `Generated { by: By::shortcode(name), anchors: vec![Anchor::invocation(Arc::new(ctx.source_info.clone()))] }`
  on each block/inline.
  - The post-walk **enriches**, not overrides: any `by.data` fields the
    Lua machinery attached (`lua_path`, `lua_line`) are preserved by
    promoting the kind from `filter` to `shortcode` while keeping the
    data fields. See "Lua-shortcode enrichment" below.
  - The post-walk recurses into nested blocks/inlines so every node in
    the dispatch output gets the anchor.
- **`TitleBlockTransform`** (line 183-185): synthesizes a level-1 Header
  from `title:` metadata. Fix: emit `Generated { by: By::title_block(), anchors: vec![] }`
  on the synthesized Header (and any nested Inlines). Note: q2-preview
  skips this transform (Plan 1), but the audit covers the HTML
  pipeline too.
- **`SectionizeTransform`** (`pampa/src/transforms/sectionize.rs:96, 148`):
  the synthetic Section Div. Fix: `Generated { by: By::sectionize(), anchors: vec![] }`.
  The wrapped Header retains its original source_info. Body blocks retain
  theirs.
- **`FootnotesTransform`**: the synthesized footnotes container Div.
  Fix: `Generated { by: By::footnotes(), anchors: vec![] }`. The
  synthesized `<sup>` markers are already source-mapped via
  `create_footnote_ref` cloning from the original `Note` inline (so
  they stay Original — no change needed). q2-preview pipeline runs
  this transform (per Plan 2B's audit); the audit applies to both
  pipelines.
- **`AppendixStructureTransform`**: the synthetic appendix container Div.
  Fix: `Generated { by: By::appendix(), anchors: vec![] }`. Same scope
  note as Footnotes.
- **`theorem.rs::extract_name_attr`** (line 313): the title Str
  extracted from `name="..."` attribute is built with
  `SourceInfo::default()`. Fix: use the attr value's source_info
  (currently lost — inspection needed for whether `attr_source` carries
  this info). At minimum, `Generated { by: By::raw("theorem-title-attr", json!({})), anchors: vec![] }`
  if we can't recover it, but better to preserve the actual source
  position from the attr-source.
- **`pampa::pandoc::treesitter_utils::postprocess`** (line 1348): the
  "Synthetic Space" inserted to separate citation from suffix. Fix:
  `Generated { by: By::tree_sitter_postprocess(), anchors: vec![] }`.

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
- **Enrichment, not override**. The Lua machinery's auto-attach
  produces `Generated { by: filter, anchors: [], by.data: { lua_path,
  lua_line } }` (post-Plan-4) for nodes constructed during a Lua
  shortcode dispatch. The shortcode resolver's post-walk enriches:
  - **Appends** an `Invocation` anchor pointing at the shortcode token.
  - **Promotes** `by.kind` from `"filter"` to `"shortcode"` while
    preserving the Lua-side fields in `by.data` (`lua_path`, `lua_line`)
    AND adding the shortcode `name`.
  The Lua-side dispatch precision is preserved; the shortcode context
  layer is added on top. No information is discarded.
- **Most transforms just need to preserve ctx.source_info**. The
  "audit and fix" is mostly bug fixes — ctx already has the info; the
  transforms just drop it. Mechanical change.
- **Shortcode resolutions use `Generated` + `Invocation` anchor, not a
  wrapper.** Each resolved Str/Inline/Block gets `Generated { by:
  shortcode(name), anchors: [Invocation -> Arc::new(ctx.source_info.clone())] }`.
  The anchor's source_info is the shortcode token's range (an Original
  from `ctx.source_info`). Plan 7's writer uses it for Verbatim-copy
  on KeepBefore. Multi-inline resolutions: every resolved node shares
  the same anchor's source_info, enabling Plan 7's dedupe rule.
- **Genuine synthesizers use `Generated` with empty anchors**.
  Sectionize, TitleBlock, Footnotes, Appendix containers — none of
  these correspond to source bytes, so they get
  `Generated { by: By::<kind>(), anchors: vec![] }`. Plan 7's coarsen
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
the first non-C frame and produces (post-Plan 4):

```rust
Generated {
    by: By::filter(lua_path.to_string(), line_num),
    anchors: vec![],
}
```

When this happens inside a Lua shortcode handler dispatch, the resolver's
post-walk sees this shape and enriches it to:

```rust
Generated {
    by: By {
        kind: "shortcode".to_string(),
        data: json!({
            "name": shortcode_name,
            "lua_path": <preserved_from_by.data>,
            "lua_line": <preserved_from_by.data>,
        }),
    },
    anchors: vec![Anchor::invocation(Arc::new(ctx.source_info.clone()))],
}
```

The Lua-side `lua_path` / `lua_line` precision is preserved in `by.data`;
the shortcode `name` is added; the kind is promoted. **Nothing is
discarded.**

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
        anchors: vec![Anchor::invocation(Arc::clone(token_arc))],
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
  (test code). Plan 6's first commit is the audit report; subsequent
  commits fix each site.
- **Theorem title from attr**: when `extract_name_attr` extracts the
  title from `name="Pythagoras"`, it gets a String with no source_info.
  Inspecting `attr_source` may or may not give the byte range of the
  attr value. Worth investigating; if achievable, use
  `Original{attr_value_range}`; otherwise
  `Generated { by: By::raw("theorem-title-attr", ...), anchors: vec![] }`.
- **Escaped shortcodes**: today `Shortcode::is_escaped` is a flag, and
  escaped shortcodes preserve as literal text (no resolution). Don't
  apply the post-walk to escaped shortcodes — they're not resolved;
  they stay as literal text with their original source_info.
- **Recursion into deep AST**: the post-walk must traverse Span, Link,
  Image, Strong, Emph, etc. for inlines; Div, BlockQuote, etc. for
  blocks. The walk is similar to existing transform machinery; reuse
  patterns from CalloutTransform or theorem.rs if possible.

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
4. Lua-attached source_info becomes `Generated { by: filter, anchors:
   [Dispatch -> Original{lua_file, ...}] }`.
5. Plan 6's post-walk's enrichment then preserves the `Dispatch`
   anchor (typed) instead of preserving `by.data` fields.

When the follow-up lands, `AnchorRole::Dispatch` joins the enum (a
non-breaking enum extension); `by.data` for `filter` / Lua-dispatched
`shortcode` kinds shrinks to per-kind config only.

## References

- `crates/quarto-core/src/transforms/shortcode_resolve.rs` — main fix
  site. Lines 172, 179, 186, 203, 208, 215, 222, 238, etc. emit
  `SourceInfo::default()`.
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:306-322` —
  `resolve_shortcode` method (single funnel for all dispatches; the
  post-walk hooks in here).
- `crates/quarto-core/src/transforms/title_block.rs:183, 185` — h1
  synthesis sites.
- `crates/pampa/src/transforms/sectionize.rs:96, 148, 169` — section
  Div synthesis sites.
- `crates/quarto-core/src/transforms/footnotes.rs` — investigate
  container site.
- `crates/quarto-core/src/transforms/appendix.rs` — investigate wrapper
  site.
- `crates/quarto-core/src/transforms/theorem.rs:281, 313` — name-attr
  title extraction.
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1348` —
  synthetic Space.
- `crates/pampa/src/lua/types.rs:1812-1840` — `filter_source_info`
  Lua-side auto-attach.
- `crates/quarto-pandoc-types/src/custom.rs` — CustomNode shape.
- `crates/quarto-core/src/transforms/callout.rs` — example pattern for
  sugar transforms wrapping output in CustomNode.

## Test plan

- **Audit-completion test**: a unit test that builds a fixture document
  exercising shortcode resolution, sectionize, and (HTML pipeline only)
  title-block / footnotes / appendix. **Asserts that the resulting AST
  has no nodes with `SourceInfo::default()` source_info AND every
  synthesized node carries an appropriate `Generated` shape** (matches
  the §Atomic-kind-set / §by.data tables in Plan 4). Defensive
  regression: catches a future PR that adds a transform without
  provenance.
- **Per-transform fix tests**: for each fixed transform, a test that
  inspects the produced source_info shape:
  - SectionizeTransform: synthetic Div has `Generated { by: { kind:
    "sectionize" }, anchors: [] }`. Header inside has its original
    source_info.
  - ShortcodeResolveTransform (uniform): each resolved Str has
    `Generated { by: { kind: "shortcode", data: { name: "..." } },
    anchors: [Anchor { role: Invocation, source_info: ... }] }`. The
    anchor's source_info chain-walks to the shortcode token's bytes
    via `resolve_byte_range`.
  - Lua-shortcode test: a `{{< kbd Ctrl+C >}}` invocation produces a
    Span with `Generated { by: { kind: "shortcode", data: { name:
    "kbd", lua_path: "...", lua_line: N } }, anchors: [Invocation] }`.
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
- **Idempotence still holds**: re-run Plan 3's idempotence test after
  the audit — the changes shouldn't introduce non-determinism.
- **`source_info` determinism (Plan 6-specific gap)**: Plan 3's hashes
  exclude `source_info` by design (`compute_blocks_hash_fresh` and
  `compute_meta_hash_fresh` both skip it). So Plan 3 does **not**
  catch a transform whose synthesized `Generated { by, anchors }`
  output is non-deterministic *in the source_info layer* — e.g., an
  `Anchor::invocation` that hashes a different `SourceInfo` on
  repeated runs because the shortcode-token's range was recomputed
  rather than cloned. Plan 6 must add its own per-fixture
  source_info-determinism check: render twice, walk the AST in
  lockstep, assert every `Generated.by`, every `Generated.anchors[]`,
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
| Audit pass (grep + categorize) | ~30 (mostly notes) |
| `stamp_shortcode_anchors` helper + recursion walks | ~80 |
| Shortcode resolver fix (~12 sites, all use the helper) | ~50 |
| TitleBlock fix | ~20 |
| Sectionize fix | ~20 |
| Footnotes fix | ~30 |
| Appendix fix | ~30 |
| Theorem title-from-attr fix | ~20 |
| TreeSitter postprocess fix | ~10 |
| Tests | ~250 |
| **Total** | **~540** |

Smaller than earlier drafts because the unified `Generated` type
collapses all shortcode handlers into a single funnel + helper. One
focused session likely.

## Notes

This is a "scattered fixes" plan — touches many transform files with
small per-file changes. Most of the diff is mechanical: `SourceInfo::default()`
→ either `ctx.source_info.clone()` (Original) for synthesizers that DO
have a source preimage but currently drop it, or
`Generated { by: By::<kind>(), anchors: vec![] }` for genuine
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
