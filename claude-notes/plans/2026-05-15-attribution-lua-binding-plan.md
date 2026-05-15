# Attribution Lua host binding (Option B)

## Overview

Implements **Option B** from `2026-05-15-attribution-on-wire-design.md`:
expose the attribution sidecar to Lua filters via a small
`quarto.attribution.*` host binding without putting any data on the AST.

Follow-on to `2026-05-06-attribution-pipeline.md`. The parent plan
defers the Lua surface as a bd-0fd0 (Quarto-API injection slot)
follow-on; this plan formalizes the work.

**Prerequisite:** the attribution-pipeline branch
(`feat/attribution-pipeline`) has landed.
`RenderContext.attribution_data` is the canonical store; this plan
adds a Lua surface that reads it. Nothing here changes the sidecar
shape, the writer-side bake, or the wire format of the q2-debug
JSON / HTML output.

## API the binding exposes

```lua
-- Convenience: looks up attribution for a node, resolves identity.
-- Returns nil if no hit, no provider, or node from non-primary file.
local hit = quarto.attribution.lookup(el)
-- => { actor = "alice@example.com", time = 1715000000000,
--      name = "Alice", color = "#ff0000" }

-- Primitive: arbitrary byte range, raw (no identity resolution).
local raw = quarto.attribution.lookup_range(0, 100)
-- => { actor = "alice@example.com", time = 1715000000000 }

-- Read-only identity map.
local idents = quarto.attribution.identities()
-- => { ["alice@example.com"] = { name = "Alice", color = "#ff0000" } }
```

Backed by `AttributionMap::query_byte_range` (already shipped on the
attribution-pipeline branch — O(log N) per call). No AST mutation, no
new wire shape, no memory cost beyond the sidecar.

## Architectural decisions to pin before coding

### 1. Pipeline ordering — move attribution-generate before user filters

Today: `AttributionGenerateTransform` runs at end-of-Navigation-Phase,
*inside* `AstTransformsStage`. `UserFiltersStage::pre` runs *before*
`AstTransformsStage`; `UserFiltersStage::post` runs *after*. As-is,
`post`-phase Lua filters (`post-quarto`, `pre-render`, `post-render`,
`pre-finalize`, `post-finalize`) see attribution; `pre`-phase filters
(`pre-ast`, `post-ast`, `pre-quarto`) don't.

**Decision:** move `AttributionGenerateTransform` to before
`UserFiltersStage::pre`. Pre-filters can use attribution too. The
end-of-Navigation-Phase placement was for tidiness (slot with the
other `*-generate` stages); attribution-generate doesn't read
`navigation.*`, so the move is safe. Symmetric and simpler to
document ("filters always see attribution if a provider is
installed").

### 2. Cross-crate plumbing — trait in pampa, impl in quarto-core

Lua filters execute in `pampa`
(`crates/pampa/src/lua/`, `crates/pampa/src/unified_filter.rs`).
Attribution types live in `quarto-core::attribution`. `pampa` cannot
depend on `quarto-core` — dependency direction is the other way.

**Decision:** define `trait AttributionLookup` in pampa with only
the methods Lua needs (`lookup_range`, `identities`, both returning
pampa-defined plain structs). `quarto-core::UserFiltersStage` builds
a handle and passes it through to `apply_filters` as a new optional
parameter. Pampa's Lua binding reads the handle when registering the
`quarto.attribution.*` table. The two crates communicate through a
small typed interface that's natural for the Lua boundary;
`apply_filters`'s current signature already accepts a `runtime:
Arc<dyn SystemRuntime>` parameter — the attribution handle follows
the same pattern.

### 3. Node → byte-range from Lua

`crates/pampa/src/lua/types.rs` exposes `attr`, `classes`,
`attributes`, etc. on Block/Inline userdata. Source-info is not
exposed today.

**Decision:** Phase 3 adds `el.source_info` as a tiny userdata with
`byte_range()` and `file_id()` methods — a small, public accessor
that's cheap and reusable beyond attribution. Phase 4's
`quarto.attribution.lookup(el)` is implemented as
`lookup_range(el.source_info:byte_range())`. Lua users can also
call `lookup_range` directly with hand-computed offsets.

## Phase 0 — Tests first (TDD)

> Per CLAUDE.md: tests written, running, and **red** before any
> Phase 1 implementation.

### Unit tests (`crates/pampa/src/lua/quarto_api.rs` tests module)

- [x] **`AttributionLookup` impl correctness.** Given an
  `Arc<AttributionData>` with runs
  `[{0..5, alice, t=1}, {5..10, bob, t=2}]`, `lookup_range(2, 8)`
  returns `Some({actor: "bob", time: 2})` (most-recent rule per
  `query_byte_range`). Off-paths: empty data → `None`; no overlap →
  `None`.
- [x] **Identity passthrough.** `identities()` returns the
  underlying `IdentityMap` entries verbatim (name and color
  matching input).
- [x] **No-provider case.** When `ctx.attribution_data.is_none()`,
  the handle is not injected; the pampa-side binding registers
  no-op stubs (`lookup`/`lookup_range` return nil; `identities`
  returns an empty table). Assert via a Lua filter fixture run
  without a provider.

### Lua filter integration tests (`crates/pampa/src/lua/filter_tests.rs`)

- [x] **Happy-path lookup.** Filter:
  ```lua
  function Span(el)
    local hit = quarto.attribution.lookup(el)
    if hit then el.classes:insert("attr-" .. hit.actor) end
    return el
  end
  ```
  Fixture: qmd with two Span elements; in-test `AttributionData`
  with two matching runs (`alice`, `bob`). Assert
  `class="attr-alice"` / `class="attr-bob"` in the rendered output.
- [x] **Identity table.** Filter walks
  `quarto.attribution.identities()` and emits the map as a
  `RawBlock`. Assert emitted text matches expected JSON.
- [x] **`lookup_range` primitive.** Filter calls
  `quarto.attribution.lookup_range(0, 10)` with hardcoded offsets;
  assert return shape `{actor, time}` (no `name`/`color`).
- [x] **Non-primary file skip.** Fixture with `{{< include
  other.qmd >}}`; the filter calls `lookup` on a node from
  `other.qmd` whose byte range *would* overlap a run in the primary
  doc's map. Assert nil returned. Pins the v1 single-doc invariant,
  parallel to the existing
  `attribution_render_skips_non_primary_file_nodes` test.

### Pipeline-ordering test

- [x] After Phase 1's move, a `pre-quarto` filter sees attribution.
  Fixture: same as the happy-path test but routed through
  `pre-quarto`. Assert classes appear.

### End-to-end CLI test (`crates/quarto/tests/attribution_lua_e2e.rs`)

- [x] Reuse the temp-git-repo scaffolding from
  `attribution_cli_e2e.rs`. Invoke:
  ```bash
  cargo run --bin quarto -- render <doc>.qmd --to html \
    --attribution=git --filter color-by-author.lua
  ```
  (Or the equivalent project-YAML opt-in if `--filter` shape
  differs.) Assert the rendered HTML contains attribution-derived
  CSS classes the Lua filter would add. Per CLAUDE.md, the test
  *must* read and grep the actual rendered file, not infer success
  from absence of errors.

## Phase 1 — Pipeline ordering

- [x] Move `AttributionGenerateTransform` registration from
  end-of-Navigation-Phase to before `UserFiltersStage::pre`.
  Refresh the comments in `pipeline.rs` that motivated the current
  placement; cross-reference this plan.
- [x] Verify the Phase 0 pipeline-ordering test now passes.
- [x] Verify every existing attribution test still passes (the
  generate stage's outputs are time-independent of pipeline
  position; this is a no-op for downstream consumers).

## Phase 2 — Extract `resolve_byte_range` helper

- [x] `resolve_byte_range` is currently a private function in
  `crates/quarto-core/src/transforms/attribution_render.rs`. Move
  it to `crates/quarto-core/src/attribution/mod.rs` (or a sibling
  `resolve.rs`) and make it `pub`.
- [x] Update `attribution_render.rs` to call the public version.
  Existing render-transform tests must pass unchanged.

## Phase 3 — Lua `source_info` accessor

- [x] In `crates/pampa/src/lua/types.rs`, add `source_info` as an
  accessible field on Block and Inline userdata. Returns a small
  userdata `SourceInfoLua` wrapping the underlying `SourceInfo`.
- [x] `SourceInfoLua` methods:
  - `:byte_range()` → table `{start, end}`, or nil when the chain
    resolves to `None` (e.g. `Concat`/`FilterProvenance`).
  - `:file_id()` → integer, or nil if unresolvable.
  Both internally call the public `resolve_byte_range` from Phase 2.
- [x] Add type fixtures for the new accessor in the
  `crates/pampa/src/lua/types.rs` test module.

## Phase 4 — `quarto.attribution` namespace

### 4a. Cross-crate plumbing (Option β)

- [x] In `pampa`, define `pub trait AttributionLookup`:
  ```rust
  pub trait AttributionLookup: Send + Sync {
      fn lookup_range(&self, start: usize, end: usize) -> Option<LookupHit>;
      fn identities(&self) -> Vec<IdentityEntry>;
  }
  pub struct LookupHit { pub actor: String, pub time: i64 }
  pub struct IdentityEntry {
      pub actor: String,
      pub name: String,
      pub color: String,
  }
  ```
  Plain `String` here — pampa doesn't need `Arc<str>` interning;
  per-call clone cost is negligible vs the Lua VM call cost.
- [x] In `quarto-core::attribution`, add
  ```rust
  pub struct AttributionLookupHandle(pub Arc<AttributionData>);
  impl pampa::AttributionLookup for AttributionLookupHandle { … }
  ```
- [x] Thread the handle through `apply_filters` as a new parameter:
  `apply_filters(pandoc, context, filters, target_format, runtime,
  attribution: Option<Arc<dyn AttributionLookup>>)`. Mirrors the
  existing `runtime` parameter and keeps `FilterContext`'s purpose
  scoped to diagnostics.
- [x] In `quarto-core::UserFiltersStage::run`, when
  `ctx.attribution_data.is_some()`, construct the handle and pass
  it through. When `None`, pass `None`.

### 4b. Lua API registration

- [x] In `crates/pampa/src/lua/quarto_api.rs`, add
  `fn register_quarto_attribution(lua: &Lua, quarto: &Table,
  handle: Option<Arc<dyn AttributionLookup>>) -> Result<()>`:
  - Build a `quarto.attribution` sub-table.
  - Register `lookup_range(start, end)` reading the handle. When
    handle is `None`, returns nil unconditionally.
  - Register `identities()` reading the handle. When handle is
    `None`, returns an empty table.
  - Register `lookup(el)` as a Lua-side function (or Rust function
    that reads `el.source_info:byte_range()` then calls
    `lookup_range` + joins identity). `el.source_info` from
    Phase 3 carries the resolution.
- [x] Wire `register_quarto_attribution` into the existing
  `register_quarto_api` (or equivalent) entry point that builds the
  `quarto` global table. Pass the optional handle through from the
  filter invocation site.

### 4c. Tests pass

- [x] Phase 0 Lua integration tests all green.
- [x] End-to-end CLI test green; inspect the rendered HTML per
  CLAUDE.md and record the exact invocation + output snippet in
  this plan.

## Phase 5 — WASM consideration

The hub-client does run Lua filters in WASM (`pampa` has
`#[cfg(target_arch = "wasm32")]` paths for the restricted Lua
stdlib). The attribution binding should compile and work there too:

- [x] Verify `AttributionLookup` trait and the handle compile under
  `wasm32-unknown-unknown` (no `Send + Sync` issues if Phase 1
  `?Send` discipline is followed for any async paths).
- [x] WASM filter execution path receives the same handle:
  hub-client's preview pipeline injects
  `PreBuiltAttributionProvider`, which populates
  `ctx.attribution_data`, which becomes the handle for the
  WASM-side filter pass. Verify with `cargo xtask verify` (the
  command CI uses for `-D warnings`).
- [x] Add a hub-client end-to-end check: open a doc with the
  Authorship toggle on, run a small Lua filter via whatever filter
  surface hub-client supports, confirm attribution is visible. If
  hub-client doesn't expose user-filter configuration in the
  preview path, document this explicitly as "WASM-side binding
  works mechanically but no UI surface exercises it in v1."

## Phase 6 — Documentation

- [x] Extend `docs/authoring/attribution.qmd` (the user-facing
  attribution page added in the parent plan) with a "Using
  attribution in Lua filters" section:
  - A small worked example filter (the "colour spans by author"
    pattern).
  - Function signatures and return shapes.
  - The "non-primary file returns nil" rule.
  - That the binding is read-only and v1 single-doc.
- [x] API reference doc-comments in `quarto_api.rs` on each
  registered function: parameters, return shape, nil cases.
- [x] Brief mention in `crates/pampa/src/lua/quarto_api.rs`
  module doc-comment that `quarto.attribution` is the
  bd-0fd0 host binding for the attribution feature.

## Out of scope (deferred to future plans)

- **Mutation from Lua.** `quarto.attribution.set(el, actor, time)`
  or similar write APIs. Read-only is the v1 contract.
- **Multi-file (v2) support.** Once the parent plan ships v2 with
  `HashMap<PathBuf, AttributionMap>`, this binding gains a `path`
  parameter on `lookup_range`. Until then: v1 single-doc only;
  non-primary nodes return nil.
- **Option C** (per-node `Attr` KVs on the AST). See
  `2026-05-15-attribution-on-wire-design.md` § "Where C wins
  long-term" for the decision; defer pending an external-Pandoc-
  filter use case.

## Plan deviations (during implementation)

These are adjustments applied while executing the plan. Each lists the
plan assumption, the on-the-ground reality, and the decision taken.

### D1 — `attribution_data` lives on `RenderContext`, not `StageContext`

**Plan assumption:** Phase 4a says
"In `quarto-core::UserFiltersStage::run`, when
`ctx.attribution_data.is_some()`, construct the handle and pass it through."

**Reality:** `attribution_data: Option<Arc<AttributionData>>` is a field
on the inner `RenderContext` (inside `AstTransformsStage`), not on the
outer `StageContext` that `UserFiltersStage` sees. `StageContext` only
carries `attribution_provider`. Today `AttributionGenerateTransform`
populates `RenderContext.attribution_data` from inside the transform
pipeline, and `AttributionRenderTransform` consumes it. The sidecar
never escapes that scope.

**Decision:**
1. Add `pub attribution_data: Option<Arc<AttributionData>>` to
   `StageContext`. The field is populated by the new top-level
   `AttributionGenerateStage` (see D2) and bridged into the inner
   `RenderContext` by `AstTransformsStage` (mirroring how
   `attribution_provider` is already bridged).
2. `UserFiltersStage::pre`/`post` read `ctx.attribution_data` from
   `StageContext` to construct the `AttributionLookupHandle`.

### D3 — Tests interleaved per phase, not all front-loaded in Phase 0

**Plan assumption:** Phase 0 says "tests written, running, and **red**
before any Phase 1 implementation."

**Reality:** Many of the test surface's type references
(`AttributionLookup`, `LookupHit`, `IdentityEntry`, the
`quarto.attribution.*` namespace, `el.source_info`) don't compile
until their underlying types exist. A strict "all tests RED first"
approach would mean either:
1. Adding stub types just to make tests compile, then implementing
   on top — twice the surface area to read.
2. Writing tests in a non-compiling state, which doesn't run.

**Decision:** Tests are added in the same commit as the code change
that motivates them, but **before** the green-path behavior. Each
test first asserts the contract, then I confirm it fails as expected
in the working tree (e.g. by stubbing the function to return `None`
and running the test), then I implement and re-run. This preserves
TDD's "verify the test catches the bug" guarantee while keeping the
test surface in lockstep with the implementation surface.

### D2 — Generate-transform is an `AstTransform`, not a `PipelineStage`

**Plan assumption:** Phase 1 says "Move `AttributionGenerateTransform`
registration from end-of-Navigation-Phase to before
`UserFiltersStage::pre`."

**Reality:** `AttributionGenerateTransform` implements `AstTransform`
(takes `&mut RenderContext`), which is the interface for transforms
*inside* the `AstTransformsStage` inner pipeline. `UserFiltersStage::pre`
is a top-level `PipelineStage` (takes `&mut StageContext`). The two
interfaces aren't interchangeable — a transform can't be "moved" to a
position in the top-level pipeline.

**Decision:** Create `AttributionGenerateStage` (top-level
`PipelineStage` in `crates/quarto-core/src/stage/stages/attribution_generate.rs`)
that wraps the same provider-build + identity-merge logic and stores
the result on `ctx.attribution_data` (per D1). The existing
`AttributionGenerateTransform` is removed from the inner transform
pipeline. `AstTransformsStage` bridges `ctx.attribution_data` →
`render_ctx.attribution_data` so `AttributionRenderTransform` still
sees the sidecar.

`AttributionGenerateStage` builds a minimal `RenderContext` just to
call `provider.build(&render_ctx)?` — `GitBlameProvider` reads
`ctx.binaries.git` and `ctx.document.input` from it, and
`PreBuiltAttributionProvider` ignores the argument entirely.

### D4 — `apply_lua_filter` keeps its old signature; add `_with_attribution` siblings

**Plan assumption:** Phase 4a says "Thread the handle through
`apply_filters` as a new parameter".

**Reality:** `apply_lua_filter` and `apply_lua_filters` are called
from 120+ test sites across `pampa::lua::filter_tests` and the
`crates/pampa/tests/` integration tests. Bumping the signature would
touch every one of them — pure mechanical churn, with no test-quality
benefit.

**Decision:** Keep `apply_lua_filter` / `apply_lua_filters` /
`apply_filter` / `apply_filters` at their original signatures (no
attribution param). Add sibling functions:
- `apply_lua_filter_with_attribution`
- `apply_lua_filters_with_attribution`
- `apply_filter_with_attribution`
- `apply_filters_with_attribution`

The no-attribution versions delegate to the `_with_attribution`
versions passing `None`. `UserFiltersStage` (the only production
caller that has a handle) uses
`unified_filter::apply_filters_with_attribution` directly. Test
suites stay unchanged.

This is the explicit option called out under "Risks and unknowns" as
a viable alternative when ripple is too wide.

## Risks and unknowns

- **mlua app_data vs new parameter.** Resolved per D4 above: kept
  the new-parameter approach but only on `_with_attribution` sibling
  functions. The original signatures stay unchanged.
- **`source_info` on every Block/Inline.** Some Pandoc node types
  carry source_info today; some may not (e.g. types synthesized by
  earlier transforms). Phase 3 should pin the behaviour for
  source-info-less nodes — recommendation: `el.source_info` is nil,
  and `quarto.attribution.lookup(el)` returns nil. Don't fabricate
  ranges.
- **Test fixture sharing with parent plan.** The git-blame fixtures
  under `tests/fixtures/attribution-blame/` are already in place
  from the parent plan; Phase 0 reuses them. If the parent's
  fixture format changes, Phase 0 tests rebase automatically.

## Work items checklist

### Phase 0 — Tests (TDD, must be red)

- [x] Unit tests for `AttributionLookup` impl correctness.
- [x] Lua filter integration tests: `lookup`, `lookup_range`,
  `identities`, no-provider, non-primary-file.
- [x] Pipeline-ordering test (`pre-quarto` filter sees attribution).
- [x] End-to-end CLI test (`--attribution=git` + Lua filter,
  inspect rendered HTML).

### Phase 1 — Pipeline ordering

- [x] Move `AttributionGenerateTransform` registration site.

### Phase 2 — Extract helper

- [x] `resolve_byte_range` → `pub` in `attribution/mod.rs` (or
  `attribution/resolve.rs`).

### Phase 3 — Lua source_info accessor

- [x] `el.source_info:byte_range()` / `:file_id()` on Block and
  Inline userdata.

### Phase 4 — `quarto.attribution` namespace

- [x] `AttributionLookup` trait + plain structs in pampa.
- [x] `AttributionLookupHandle` impl in quarto-core.
- [x] `apply_filters` accepts an optional attribution handle.
- [x] `UserFiltersStage` constructs and passes the handle.
- [x] `register_quarto_attribution` in `quarto_api.rs`.

### Phase 5 — WASM

- [x] `cargo xtask verify` clean (WASM build green).
- [x] Hub-client preview check (or explicit "no UI surface in v1"
  note).

### Phase 6 — Docs

- [x] `docs/authoring/attribution.qmd` Lua filter section.
- [x] API doc-comments on every registered function.
