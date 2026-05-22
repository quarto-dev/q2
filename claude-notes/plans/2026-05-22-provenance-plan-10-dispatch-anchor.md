# Provenance Plan 10 — Dispatch anchor + Lua source registration in SourceContext

**Date:** 2026-05-22
**Branch:** feature/provenance
**Status:** Research plan (pre-implementation; API surface not yet pinned)
**Milestone:** none directly — improves source-pointing diagnostics
  and attribution for Lua-driven content; does not gate M3.

## Epic context

Part of the **provenance epic** (Plans 3–10). Lua filter files and
Lua-shortcode handler files contribute source-side bytes to
`Generated` nodes (a filter constructed an `Str("HELLO")` somewhere
in `upper.lua`; a `{{< kbd >}}` handler ran code at `kbd.lua:14`).
Today, that source identity lives in `By.data` as a stringly-typed
`{filter_path, line}` payload constructed via `debug.getinfo()`.
It belongs in the `from` anchor list, attached via a new
`AnchorRole::Dispatch` role.

Same asymmetry contract as `ValueSource` (settled by Plan 9):
**Dispatch is diagnostic-only**, never walked by the writer's
`preimage_in`. The point is attribution and source-pointing
diagnostics — "this rendered text came from line 14 of `kbd.lua`" —
not round-trip.

## Goal

Migrate Lua-driven `Generated` shapes from string-keyed
`by.data: {filter_path, line}` to typed source_info pointers in the
anchor list:

- **Filter constructions**: `Generated { by: filter(), from:
  [Dispatch -> lua_si] }` (was `Generated { by: filter(path, line),
  from: [] }`).
- **Lua-handler shortcode resolutions**: `Generated { by:
  shortcode(name), from: [Invocation -> token_si, Dispatch -> lua_si] }`
  (was `Generated { by: shortcode{name, lua_path, lua_line}, from:
  [Invocation -> token_si] }`).

To make those source_info pointers meaningful, **register Lua filter
files and Lua-shortcode-handler files in `SourceContext`** so they
get `FileId`s and their content is available for byte-range
resolution.

When this plan lands, source-pointing diagnostics from Lua land
("at line 14 of upper.lua, column 5–10") use the same SourceContext
machinery as qmd / YAML diagnostics. Attribution tooling can chase
the `Dispatch` anchor back to the Lua function that produced a node.

## Scope

### In scope

#### Phase 1 — `AnchorRole::Dispatch`

- Add `Dispatch` variant to `AnchorRole` enum in
  `crates/quarto-source-map/src/source_info.rs:91-118` alongside the
  existing `Invocation`, `ValueSource`, `Other`.
- Doc-comment explicitly references the Plan-9-established policy:
  `preimage_in` walks `Invocation` only; `Dispatch` is
  diagnostic-only and never consulted by the writer.
- Add `Anchor::dispatch(source_info: Arc<SourceInfo>) -> Self`
  constructor parallel to `Anchor::invocation` / `Anchor::value_source`.

#### Phase 2 — SourceContext extension for Lua files

- Extend `SourceContext::add_file` (currently
  `crates/quarto-source-map/src/context.rs:59`) to support Lua files.
  Two possible extensions:
  - (A) Add a `FileKind { Qmd, Yaml, Lua, … }` discriminator on
    `FileInformation`. `add_file` stays signature-compatible;
    callers passing Lua files use a new `add_lua_file` helper or
    pass `FileKind::Lua` explicitly.
  - (B) Reuse `add_file` as-is (Lua files are just files;
    path/content are sufficient).
  - Recommendation: (B) for v1; (A) only if a downstream consumer
    needs to distinguish kind (e.g. line-numbering rules differ for
    Lua vs. qmd, which they don't today).
- Confirm `FileInformation::compute_line_breaks` handles Lua source
  correctly (it should — it just indexes `\n` positions).

#### Phase 3 — Lua engine bridge: pass FileId through callbacks

- `apply_lua_filters`
  (`crates/pampa/src/lua/filter.rs:158-200` and surrounding) reads
  the filter path from `FilterSpec::Lua(path)` and the filter file
  bytes from disk. **Register the file in `SourceContext` at that
  point**, capturing the returned `FileId`.
- Thread the `FileId` into the Lua closure context so callbacks that
  introspect `debug.getinfo()` can resolve `(source: path, line:
  line_num)` into `SourceInfo::Original { file_id, start, end }`
  where `start..end` covers the line's bytes (via
  `FileInformation`'s line-break index).
- Update `get_caller_source_info`
  (`crates/pampa/src/lua/diagnostics.rs:255`) — currently constructs
  `Generated { by: By::filter(path, line), from: SmallVec::new() }`.
  New shape: `Generated { by: By::filter(), from:
  [Dispatch(Arc::new(Original{file_id, start, end}))] }`.

#### Phase 4 — `By::filter` signature shrinks

- Change `By::filter(path: impl Into<String>, line: usize)`
  (currently at `crates/quarto-source-map/src/source_info.rs:458`)
  to `By::filter()`. The path/line move to the Dispatch anchor's
  source_info; `by.data` becomes `null`.
- All call sites in `crates/pampa/src/lua/types.rs:1830`,
  `crates/pampa/src/lua/diagnostics.rs:203,262,847`,
  `crates/pampa/src/readers/json.rs:305,2764` migrate. Most are
  diagnostic-side; the json reader has a legacy-back-compat path
  reading `"FilterProvenance"` tag.
- **No backward-compat carve-out for `By::filter`.** Same reasoning
  as Plan 9's `By::appendix` change:
  1. `By::filter` is workspace-internal Rust — no FFI, no extension
     SDK, no TS mirror.
  2. Plan 5 has shipped `By::filter(path, line)` to the JSON wire
     format. **Wire migration required** (see §Phase 6 below) —
     readers temporarily accept both shapes; writers emit the new
     shape after Plan 10 lands; legacy readers removed in a
     subsequent cleanup.
- `By::as_filter()` accessor (currently returns
  `Option<(&str, usize)>` from `by.data`) gets removed or
  repurposed. Callers needing path/line read the Dispatch anchor's
  source_info and resolve via `SourceContext`.

#### Phase 5 — Lua-handler shortcode resolutions

- The shortcode resolver
  (`crates/quarto-core/src/transforms/shortcode_resolve.rs:380-460`)
  dispatches to Lua handlers via `dispatch_to_lua_engine`. When the
  handler is Lua-backed, attach a `Dispatch` anchor pointing at the
  handler function's source line.
- Built-in (Rust) handlers like `MetaShortcodeHandler` stay with
  `from: [Invocation]` only — no Dispatch.
- The Lua engine needs to know which file each handler is registered
  in (already known via the registration call site). Stash that
  alongside the handler binding.

#### Phase 6 — Wire format migration

- Plan 5 emits `Generated { by: filter, by.data: {filter_path, line} }`
  to JSON wire code 4. After Plan 10:
  - Writers emit `Generated { by: filter, by.data: null }` plus a
    `Dispatch` anchor in the `from` list.
  - Readers accept both shapes during a transition window:
    - Old shape (path/line in `by.data`): synthesize the Dispatch
      anchor at read time from the data payload. Requires looking
      up the path in SourceContext or registering on the fly.
    - New shape: pass through.
- The transition window is one release cycle; document the cleanup
  follow-up.
- Equivalent migration on the Lua-shortcode-handler shape
  (currently `by.data: {name, lua_path, lua_line}` → `by.data:
  {name}` + Dispatch anchor).

#### Phase 7 — Cache-key surface

- Lua filter file content becomes Pass1 cache input. Either:
  - (A) Extend `Pass1KeyInputs`
    (`crates/quarto-core/src/project/cache_key.rs:108`) with a
    `lua_filter_files: &[(PathBuf, Vec<u8>)]` field, hash filter
    bytes there; or
  - (B) Reference SourceContext-registered Lua files by `FileId` +
    content (cleaner but requires SourceContext to be cache-key
    aware).
- **Coordinate with Plan 7a's `filter_sources_hash`** (planned
  parallel implementation in `crates/quarto-core/src/cache_key.rs`):
  - Plan 7a hashes filter file bytes for idempotence verdicts.
  - Plan 10 hashes filter file bytes for cache invalidation.
  - These are the same hash conceptually; merge into one
    computation. Recommendation: Plan 10's Phase 7 lands the
    `lua_filter_files` field; Plan 7a's idempotence cache reuses
    the same field rather than re-hashing.

### Out of scope

- **Lua hot-reload / file-watcher integration** — a Lua file editing
  experience that re-runs the filter on save. Demand-driven
  invalidation via cache-key hashing is sufficient for v1.
- **Lua-LSP cross-references** (jump-to-definition into filter code
  from a rendered diagnostic) — UX work that consumes Plan 10's
  output but isn't part of it. Likely a future hub-client plan.
- **Non-Lua extension-contributed handlers** (future WASM-shortcode,
  native-Rust-shortcode). The `Dispatch` role is Lua-flavored — the
  source_info pointer assumes a file with byte ranges. WASM /
  native handlers may want a different anchor role (e.g.
  `Other("wasm-handler")` carrying a handler URI). Defer until those
  handler kinds exist.
- **Citeproc / JSON-filter source pointers**. Citeproc is a built-in
  Rust filter (no Lua); JSON filters are external processes (no
  source we can register). `FilterSpec::Citeproc` / `FilterSpec::Json`
  variants stay with `Generated { by: filter(), from: [] }` —
  diagnostic source pointing isn't meaningful for them.
- **Lua-engine-side restructuring** (e.g. moving the mlua bridge to
  a separate crate). Plan 10 changes the contract at the bridge
  boundary; it does not refactor the bridge.
- **bd-2mxo / `AttrSourceInfo` fixes** — separate concerns.

## Design decisions (settled)

- **`AnchorRole::Dispatch` is diagnostic-only.** Follows Plan 9's
  `AnchorRole::Other` policy: `preimage_in` walks `Invocation` only.
  Dispatch is consumed by attribution / diagnostic UI, not by the
  writer's Verbatim path.

- **`By::filter` becomes nullary.** Path/line move to Dispatch.
  `By.data` for filter-kind is `null`. Wire format migrates (Phase 6
  above).

- **Lua-handler shortcode keeps `name` in `by.data`.** The shortcode
  name is part of the *identity* (which shortcode resolution
  produced this node), not the *dispatch source* (which file
  resolved it). The two are distinguishable: name is a parameter of
  the `By` shape (`shortcode("meta")` vs `shortcode("kbd")`); dispatch
  source is an anchor pointing at the handler's location.

- **Source range of a Dispatch anchor: line-covering `Original`.**
  `debug.getinfo()` gives line numbers, not byte ranges. Once Lua
  file content is in SourceContext, we compute the byte range of the
  named line via `FileInformation`'s line-break index. The Dispatch
  anchor's source_info is `Original { file_id: lua_file, start:
  line_start, end: line_end }`. Sub-line precision (specific
  function or expression) is out of scope for v1 — `debug.getinfo()`
  doesn't provide it without parsing the Lua source.

- **Filter files are registered eagerly at `apply_lua_filters`
  entry.** Not lazily on first `debug.getinfo()` call — eager
  registration ensures the FileId is stable across multiple
  callbacks and accessible without thread-safety gymnastics in the
  Lua-closure context.

- **Lua-shortcode handler files are registered at handler
  registration time** (when `_extension.yml` loads). Same eager
  pattern as filter files. The handler registry maps handler
  name → `(FileId, line_in_file)`.

- **No backward-compat carve-out for the wire format.** Plan 5's
  emitted shape (`by.data: {filter_path, line}`) has shipped, but
  Plan 7's incremental writer has not — so no qmd files persist
  with this shape on disk. The wire format appears only in caches
  / IPC transcripts that are forward-readable for one release cycle
  (Phase 6's dual-reader window).

- **Plan posture: research plan.** This document settles the API
  shape (the Dispatch role, the `By::filter` migration, the
  SourceContext extension); it does not yet commit to the
  implementation order. A subsequent review pass converts it to a
  development plan with checklisted phases.

## API surface to settle (research-plan deliverables)

By the time this plan converts to a development plan, the following
must be pinned:

1. **`AnchorRole::Dispatch` doc-comment text** — exact wording of
   "diagnostic-only, never consulted by `preimage_in`" policy.

2. **`SourceContext` Lua-file kind discrimination** — option (A)
   with `FileKind` enum vs. option (B) reuse `add_file` as-is.
   Recommend (B); revisit if downstream needs (A).

3. **Lua engine bridge: how the `FileId` is threaded into the
   closure context.** mlua's app-data slot (`Lua::set_app_data`) is
   the obvious answer. Confirm during implementation.

4. **`Pass1KeyInputs` field shape** — option (A) `lua_filter_files`
   field vs. option (B) SourceContext-referenced. Recommend (A) for
   v1; Plan 7a coordinates by reading the same field.

5. **Wire-format migration window** — which release cycle the dual
   reader stays active. Stated in Plan 6's commit message;
   propagated to wire-format documentation.

6. **`By::as_filter()` deprecation** — remove vs. repurpose to
   read from the Dispatch anchor. Recommend: remove; callers
   needing path/line read the Dispatch source_info directly.

## Open questions for implementation

- **Pre-registration vs. on-demand registration of Lua files.**
  Eager (Phase 3) means every render pays the SourceContext cost
  even if `debug.getinfo()` never fires. On-demand registration is
  cheaper but introduces order-dependence in the closure context.
  Recommend eager; benchmark to confirm cost is negligible.

- **`debug.getinfo` performance.** Calling
  `debug.getinfo` on every constructed node may dominate filter
  runtime. Verify against a filter-heavy fixture during
  implementation; if it's expensive, batch source-info attachment to
  the post-walk helper (`enrich_or_create` in Plan 6's design).

- **Coordination with Plan 7a's `filter_sources_hash`.** Plan 7a
  proposes hashing filter files for idempotence verdicts; Plan 10
  hashes them for cache invalidation. Recommend: settle on one hash
  computation owned by Plan 10's Phase 7; Plan 7a reuses it. Confirm
  during the Plan 7a → Plan 10 sequencing discussion.

- **Lua-shortcode-handler file registration timing.** Extension
  loading (`_extension.yml` parsing) happens before filter pipeline
  setup. Need to ensure SourceContext is available at extension
  load — likely via the existing `StageContext`-style threading.
  Confirm.

- **Reader behavior for unknown Lua paths during legacy-shape
  decode.** When the legacy `by.data: {filter_path, line}` shape
  arrives at a reader and the named path isn't yet registered in
  SourceContext, two options:
  - Register on the fly (read the file bytes, synthesize a FileId).
  - Emit a `Dispatch` anchor pointing at a file-less SourceInfo
    (e.g. `Original { file_id: SENTINEL_UNKNOWN, … }`).
  Recommend the first; the reader has the path string and can
  populate SourceContext.

- **Migration of existing Plan 4 tests.** The unit tests in
  `crates/quarto-source-map/src/source_info.rs:715-770` exercise
  `By::filter("foo.lua", 42)` extensively. They migrate to
  `By::filter()` + a Dispatch anchor; the path/line assertions move
  to the anchor's `source_info`. Mechanical but ~10 test changes.

- **Plan 6's Lua post-walk shape (`enrich_or_create`).** Plan 6
  Phase 6's post-walk helper (per the diff in Plan 6 §"The post-walk
  helper") promotes Lua-attached source_info to the canonical
  `Generated { by: filter, ... }` form. After Plan 10 the canonical
  form is `Generated { by: filter(), from: [Dispatch] }`. The
  helper updates accordingly. Confirm Plan 6 lands before Plan 10
  implementation (or that Plan 6 is amended to anticipate the
  shape change).

## References

- `crates/quarto-source-map/src/source_info.rs:91-118` —
  `AnchorRole` enum (Phase 1 extends).
- `crates/quarto-source-map/src/source_info.rs:458-466` —
  `By::filter` constructor (Phase 4 signature change).
- `crates/quarto-source-map/src/source_info.rs:582-594` —
  `By::as_filter` accessor (Phase 4 removes / repurposes).
- `crates/quarto-source-map/src/context.rs:59-130` —
  `SourceContext::add_file*` family (Phase 2 extends).
- `crates/quarto-source-map/src/file_info.rs:12-58` —
  `FileInformation`; line-break index used in Phase 3 for byte-range
  resolution.
- `crates/pampa/src/lua/filter.rs:158-200,270` —
  `apply_lua_filters` entry; Phase 3's eager-registration site.
- `crates/pampa/src/lua/types.rs:1820-1840` — `debug.getinfo()`
  consumer (Phase 3 migrates to FileId-backed shape).
- `crates/pampa/src/lua/diagnostics.rs:195-265,847` — Generated
  construction sites; Phase 3 + 4 migrate.
- `crates/pampa/src/readers/json.rs:305,2764` — wire-format
  decoder; Phase 6's dual-reader window.
- `crates/quarto-core/src/project/cache_key.rs:108-141` —
  `Pass1KeyInputs`; Phase 7 extends.
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:380-460` —
  Lua shortcode dispatch; Phase 5's stamping site.
- Plan 6 §"Dispatch follow-up" — Plan 10's scope-pickup point.
- Plan 9 §"Settled `AnchorRole::Other` policy" — Plan 10 inherits the
  policy for Dispatch.
- Plan 5 (wire format) — Phase 6's migration is on top of Plan 5's
  code-4 emission.
- Plan 7a — coordinates on filter-source hashing (Phase 7).
- bd-36fr9 (closes).

## Test plan

### Phase 1 (`AnchorRole::Dispatch`)

- Constructor unit tests parallel to `Anchor::invocation` /
  `Anchor::value_source`.
- Serde round-trip test for a `Generated` carrying a `Dispatch`
  anchor.
- `preimage_in` asymmetry test: `Generated { by: filter(), from:
  [Dispatch(lua_si)] }` → `preimage_in` returns None (Lua bytes are
  not body bytes; the writer must not copy them into the parent
  file).
- `anchors_with_role(&AnchorRole::Dispatch).count()` returns 1 on
  the above shape.

### Phase 2 (SourceContext Lua-file extension)

- `add_file` with a `.lua` path produces a FileId; content is
  retrievable.
- `FileInformation::map_offset` resolves byte offsets to (row, col)
  for Lua source.

### Phase 3 (Lua bridge FileId threading)

- A filter that constructs a node (via `pandoc.Str(...)`) produces
  a `Generated { by: filter(), from: [Dispatch] }` shape; the
  Dispatch anchor's source_info chain-resolves to the filter
  file's FileId and the constructed line's byte range.
- `get_caller_source_info` returns the new shape; legacy callers
  failing to find a `(path, line)` in `by.data` get a
  doc-commented migration message.

### Phase 4 (`By::filter` signature shrinkage)

- All migrated unit tests pass with the nullary constructor.
- `By::filter().is_atomic_kind()` still returns true (atomicity
  unchanged).

### Phase 5 (Lua-handler shortcode)

- A Lua-handler shortcode resolution produces `Generated { by:
  shortcode(name), from: [Invocation, Dispatch] }`. Built-in
  shortcode resolutions (meta, var) stay `from: [Invocation]` only.

### Phase 6 (wire format migration)

- Writer: emits the new shape.
- Reader: accepts both old and new shapes; legacy decode produces
  the same in-memory shape as a fresh write would.
- Snapshot test asserting byte-for-byte stability of a few sample
  Lua-filter-emitting fixtures after migration.

### Phase 7 (cache-key surface)

- Cache key invalidates when a Lua filter file's content changes.
- Cache key stable when Lua filter file content is unchanged.

### End-to-end

- Lua filter raising a `quarto.warn(...)` from line 14 of `foo.lua`
  produces a diagnostic whose source range
  chain-resolves (via `SourceInfo::resolve_byte_range`) to
  `(foo_lua_file_id, line_14_start, line_14_end)`.
- A document with a Lua-handler shortcode (`{{< kbd Alt-X >}}`):
  - Resolved inline carries Dispatch anchor pointing at the
    handler's Lua source.
  - Edit-back round-trip preserves the `{{< kbd Alt-X >}}` token
    in the qmd source (Plan 7 Verbatim via the Invocation anchor;
    Dispatch is not consulted).

## Dependencies

### Hard dependencies

- **Plan 4** — `AnchorRole` enum.
- **Plan 6** — `Generated`-stamping post-walk helper
  (`enrich_or_create`) is the natural point to migrate to the new
  shape. Plan 6 must land before Plan 10 implementation, OR Plan 6
  is amended to anticipate the Dispatch shape during
  implementation. Recommend the former.
- **Plan 5** — Plan 10's wire-format migration is on top of Plan
  5's code-4 emission.

### Soft dependencies

- **Plan 9** — establishes the `AnchorRole::Other` policy that
  Dispatch inherits. Doesn't strictly block Plan 10 implementation
  (the policy is doc-only), but Plan 9 lands the policy in writing
  first.
- **Plan 7a** — coordinates on filter file hashing (Phase 7).
  Recommend Plan 10's Phase 7 lands the cache-input shape; Plan 7a's
  idempotence cache reuses it.

### Does not block

- **Plan 7 implementation** can ship without Plan 10. Plan 7's
  writer consults `Invocation` only; Dispatch lands in the
  diagnostic UX cycle.

### Blocks

- Future Lua-LSP / hub-client diagnostic-clicks-to-source UX work.
- Future extension-author-facing handler-trace tooling.

## Risk areas

- **Lua engine bridge complexity.** Touches mlua interop, app-data
  context threading, debug.getinfo behavior across Lua versions
  (5.1 vs. 5.4 — verify what we use). The mlua side has historically
  been a source of subtle bugs; budget extra time for edge cases.

- **`debug.getinfo` performance.** Calling on every constructed node
  could dominate filter runtime. Mitigation: batch via Plan 6's
  post-walk helper if necessary; benchmark.

- **Wire-format dual-reader correctness.** Bugs in the legacy-shape
  decoder could silently corrupt the in-memory shape. Mitigation:
  snapshot tests in Phase 6 + an explicit `compute_blocks_hash_fresh`
  comparison between legacy-decoded and freshly-emitted ASTs.

- **SourceContext lifetime / sharing.** Lua files registered eagerly
  at `apply_lua_filters` entry need to be available for the
  duration of the pipeline. The existing SourceContext sharing
  pattern (likely `Arc<Mutex<…>>` or `&mut` through the pipeline)
  must accommodate Lua-file additions mid-pipeline. Verify.

- **Coordination friction with Plan 7a.** Both plans touch
  `cache_key.rs` and want to hash filter files. If Plans 7a and 10
  land in arbitrary order, the second one merging may have to
  reconcile field naming / shape. Mitigation: settle the
  `lua_filter_files: &[(PathBuf, Vec<u8>)]` field naming in this
  research plan; Plan 7a's research plan refers to the same shape.

- **Migration tests that touch `By::filter("foo.lua", 42)`.** ~10
  unit tests in `source_info.rs` migrate mechanically; if any are
  missed during the signature change, the workspace fails to
  compile. Mitigation: the compiler is the safety net here — `cargo
  build --workspace` will name every offending site.

## Estimated scope

| Phase | Lines (rough) |
|---|---|
| 1: `AnchorRole::Dispatch` + Anchor constructor + tests | ~80 |
| 2: SourceContext Lua-file support (probably minimal) | ~40 |
| 3: Lua bridge FileId threading + byte-range computation | ~200 |
| 4: `By::filter` signature shrinkage + call-site migration | ~120 |
| 5: Lua-handler shortcode Dispatch attachment | ~80 |
| 6: Wire-format dual-reader + tests | ~150 |
| 7: Cache-key extension + Plan 7a coordination | ~80 |
| Tests across phases | ~350 |
| **Total** | **~1100** |

Two focused sessions likely; high-complexity due to mlua interop
and the wire-format migration. The Lua engine bridge work in
Phase 3 is the riskiest piece — if `debug.getinfo` ergonomics or
performance surprise, the design changes.

## Notes

This plan is the "Lua-source pointing" wing of the provenance epic.
Plan 9 covers metadata-derived attribution; Plan 10 covers
Lua-derived attribution. Both rely on the `AnchorRole::Other`
policy Plan 9 commits to writing.

After Plan 10, the `Generated.by.data` payload shrinks across all
known kinds:
- `filter`: `{filter_path, line}` → `null` (Plan 10).
- `shortcode`: `{name, lua_path, lua_line}` for Lua handlers →
  `{name}` (Plan 10). Built-in handlers unchanged.
- `appendix`: `null` → serialized `AppendixSection` enum (Plan 9).
- `sectionize`, `title-block`, `footnotes`, `appendix-container`,
  `tree-sitter-postprocess`, `user-edit`, `include`: `null`
  (unchanged).

The trajectory is "By.data shrinks; the anchor list grows." That's
the right direction — typed source_info pointers in `from` are
strictly more powerful than untyped strings in `by.data`, and they
follow the established `Invocation` / `ValueSource` / `Dispatch`
role discipline.

### Naming convention

Uses the `provenance-plan-N-<slug>.md` naming (no `q2-preview-`
prefix) established by Plan 9. The provenance epic has outgrown the
original q2-preview framing.
