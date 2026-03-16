# User Filters in the Render Pipeline

**Created**: 2026-03-16
**Status**: In Progress (Phases 1-6 complete)
**Parent Epic**: k-407 (Extensible filters for quarto-markdown-pandoc)
**Related Issues**: k-409 (Lua filter support), k-thpl (Port Lua filter infrastructure)

## Overview

Wire the `filters` YAML metadata key into the `quarto-core` render pipeline so that
user-specified filters (Lua, JSON, citeproc) are applied during `q2 render`.

Currently, pampa has a fully working filter engine (`apply_lua_filters`, `apply_filters`,
`FilterSpec`), but `quarto-core` never calls it. The `filters` field exists in data
structures but is never consumed.

### Design Decisions (settled)

- **YAML vocabulary**: Reuse quarto 1's vocabulary — string, `{type, path}` object,
  `{type, path, at}` object with entry point
- **Entry points**: All 8 TS Quarto entry points are accepted. Filters are sorted by
  the canonical TS Quarto order, then split into two groups for our two pipeline
  insertion points (before and after `AstTransformsStage`).
  The `quarto` sentinel splits the filter list (default for before-sentinel is
  `pre-quarto`, default for after-sentinel is `post-render` — matching TS Quarto).
- **Implementation**: Option A — new pipeline stages flanking `AstTransformsStage`,
  NOT wrapping filters as `AstTransform` implementations
- **Filter path resolution**: Relative to the document file. Extensions are out of scope.
- **Metadata merging**: Standard `MergedConfig` merge applies. Document `filters:` replaces
  project `filters:`.
- **Filter types**: All three — Lua (`.lua`), JSON (external executable), citeproc (`"citeproc"`)
  — via pampa's existing `FilterSpec`
- **WASM**: Out of scope. Will be a separate effort.

### Entry Point Model

TS Quarto defines 8 entry points executed in this order within its Lua filter pipeline:

```
1. pre-ast
2. post-ast
3. pre-quarto       ← default for filters before sentinel (or no sentinel)
4. post-quarto
5. pre-render
6. post-render      ← default for filters after sentinel
7. pre-finalize
8. post-finalize
```

In our pipeline we have only two insertion points (Pre and Post around
`AstTransformsStage`). We map all 8 entry points to these two positions:

| Entry point    | q2 position | Rationale |
|---------------|-------------|-----------|
| `pre-ast`      | Pre         | Before transforms |
| `post-ast`     | Pre         | Before transforms |
| `pre-quarto`   | Pre         | Before transforms (sentinel default) |
| `post-quarto`  | Post        | After transforms |
| `pre-render`   | Post        | After transforms |
| `post-render`  | Post        | After transforms (sentinel default) |
| `pre-finalize` | Post        | After transforms |
| `post-finalize`| Post        | After transforms |

Within each group (Pre or Post), filters are sorted by entry point order (the 1-8
list above), preserving relative order for filters at the same entry point.

Unknown `at` values fall back to `pre-quarto` with a warning (matching TS Quarto
behavior).

### Pipeline After This Work

```
ParseDocument → EngineExecution → MetadataMerge → CompileThemeCss →
  [UserPreFilters] → AstTransforms → [UserPostFilters] →
  RenderHtmlBody → ApplyTemplate
```

The two new stages are no-ops when `filters:` is absent or empty.

---

## Work Items

### Phase 1: Expose pampa's Filter API

Currently `unified_filter.rs` and `json_filter.rs` are private modules of pampa's
**binary** (`main.rs`), not exported from the library. `quarto-core` cannot use them.

- [x] **1.1** Move `unified_filter` from binary-only to library export:
  - Add `pub mod unified_filter;` to `crates/pampa/src/lib.rs`
  - Remove `mod unified_filter;` from `crates/pampa/src/main.rs` (it will use
    the library's version via `pampa::unified_filter`)
  - Update `main.rs` references from `unified_filter::` to `pampa::unified_filter::`
    (or add `use pampa::unified_filter;`)

- [x] **1.2** Move `json_filter` from binary-only to library export:
  - Add `#[cfg(feature = "json-filter")] pub mod json_filter;` to `crates/pampa/src/lib.rs`
  - Remove `mod json_filter;` from `main.rs`, update references
  - This is needed because `unified_filter.rs` references `crate::json_filter` under
    `#[cfg(feature = "json-filter")]`

- [x] **1.3** Enable required features on the pampa dependency for quarto-core:
  - The workspace defines `pampa` with `default-features = false`
  - pampa's defaults include `lua-filter`, `json-filter`, `template-fs`, `terminal-support`
    but none are enabled for library consumers
  - Update `crates/quarto-core/Cargo.toml`:
    ```toml
    pampa = { workspace = true, features = ["lua-filter", "json-filter"] }
    ```
  - This pulls in `mlua` (for Lua) and enables subprocess spawning (for JSON filters)

- [x] **1.4** Verify pampa library + binary compile: `cargo build -p pampa`
  - pampa: 3381 tests passed, quarto-core: 645 tests passed

### Phase 2: Filter Resolution from Metadata

- [x] **2.1** Create `crates/quarto-core/src/filter_resolve.rs` — module for reading
  and resolving the `filters` metadata key

  This module provides a single public function:

  ```rust
  pub struct ResolvedFilters {
      pub pre: Vec<FilterSpec>,   // filters mapped to Pre position
      pub post: Vec<FilterSpec>,  // filters mapped to Post position
  }

  pub fn resolve_filters(
      meta: &ConfigValue,
      document_dir: &Path,
  ) -> Result<ResolvedFilters>
  ```

  Implementation:
  - Read `meta["filters"]` — if absent, return empty pre/post
  - Find the `"quarto"` sentinel index (if any). If absent, sentinel index = ∞.
  - Iterate the array (skipping the sentinel). For each element:
    - **String**: `"citeproc"` or a path (use `FilterSpec::parse`)
    - **Map with `type` + `path`**: explicit filter spec (`.lua` → Lua, else JSON)
    - **Map with `type` + `path` + `at`**: entry point filter with explicit `at`
  - Assign each filter an entry point:
    - If it has an explicit `at` field, use that. Unknown `at` values → warn and
      use `"pre-quarto"` (matching TS Quarto behavior).
    - If no `at` field: before sentinel → `"pre-quarto"`, after sentinel → `"post-render"`
      (matching TS Quarto's `kQuartoPre` / `kQuartoPost` defaults).
  - Sort filters by entry point order (pre-ast < post-ast < pre-quarto < post-quarto
    < pre-render < post-render < pre-finalize < post-finalize), preserving relative
    order for filters at the same entry point.
  - Split into Pre/Post groups using the mapping table in "Entry Point Model" above.
  - Resolve relative paths against `document_dir`

- [x] **2.2** Write tests for `resolve_filters`:
  - Empty/missing `filters` key → empty result
  - String filters: `["a.lua", "b.py", "citeproc"]` → all Pre (default `pre-quarto`)
  - Object filters: `[{type: "lua", path: "a.lua"}]` → Pre
  - `quarto` sentinel splitting: `["pre.lua", "quarto", "post.lua"]`
    → `pre.lua` in Pre (`pre-quarto`), `post.lua` in Post (`post-render`)
  - All 8 `at` values: verify correct Pre/Post mapping
  - `at` overrides sentinel: `["quarto", {path: "x.lua", at: "pre-quarto"}]`
    → `x.lua` in Pre despite appearing after sentinel
  - Sorting by entry point order: `[{path: "b.lua", at: "post-render"},
    {path: "a.lua", at: "pre-quarto"}]` → Pre has `a.lua`, Post has `b.lua`
  - Same entry point preserves relative order
  - Path resolution: relative paths joined with document_dir
  - Unknown `at` value warns and defaults to `pre-quarto`
  - No `quarto` sentinel → all filters are Pre (`pre-quarto`)

### Phase 3: UserFiltersStage

- [x] **3.1** Create `crates/quarto-core/src/stage/stages/user_filters.rs` — the
  `UserFiltersStage` pipeline stage

  ```rust
  pub struct UserFiltersStage {
      position: FilterPosition,  // Pre or Post
  }

  enum FilterPosition {
      Pre,   // runs before AstTransformsStage
      Post,  // runs after AstTransformsStage
  }
  ```

  The stage:
  1. Calls `resolve_filters(doc.ast.meta, document_dir)` to get `ResolvedFilters`
  2. Selects `pre` or `post` based on its `position`
  3. If the selected list is empty, passes through unchanged (no-op)
  4. Calls `pampa::unified_filter::apply_filters(pandoc, context, &specs, format)`
  5. Replaces `doc.ast` and `doc.ast_context` with the filtered results
  6. Collects diagnostics into `ctx.diagnostics`

  Key design points:
  - Both Pre and Post stages call `resolve_filters` independently. This is fine because
    it's cheap (just reads metadata) and keeps the stages stateless.
  - `resolve_filters` handles all entry point sorting internally. The stage just picks
    `.pre` or `.post` from the result.
  - The stage needs access to `doc.ast_context` (pampa's `ASTContext`), which is already
    in `DocumentAst`.
  - The target format string comes from `ctx.format.identifier`.
  - `apply_filters` returns `FilterError`; convert to `PipelineError` via `.map_err()`.

- [x] **3.2** Register the stage in `stage/stages/mod.rs`

- [x] **3.3** Write tests for `UserFiltersStage`:
  - No filters → passthrough (AST unchanged)
  - Pre stage with Lua filter → filter applied
  - Post stage with Lua filter → filter applied
  - Pre + Post with sentinel → correct splitting
  - Filter error → `PipelineError` propagated
  - Diagnostics from filters are collected

### Phase 4: Pipeline Integration

- [x] **4.1** Update `build_html_pipeline_stages()` in `pipeline.rs`:

  ```rust
  pub fn build_html_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
      vec![
          Box::new(ParseDocumentStage::new()),
          Box::new(EngineExecutionStage::new()),
          Box::new(MetadataMergeStage::new()),
          Box::new(CompileThemeCssStage::new()),
          Box::new(UserFiltersStage::pre()),    // NEW
          Box::new(AstTransformsStage::new()),
          Box::new(UserFiltersStage::post()),   // NEW
          Box::new(RenderHtmlBodyStage::new()),
          Box::new(ApplyTemplateStage::new()),
      ]
  }
  ```

- [x] **4.2** Update `build_html_pipeline_stages()` docstring with the new stages

- [x] **4.3** Do NOT update `build_wasm_html_pipeline()` — WASM filter support is
  a separate effort

### Phase 5: Smoke Test

- [x] **5.1** Create a smoke test in `crates/quarto/tests/smoke-all/` that exercises
  user filters via `q2 render`:

  ```
  smoke-all/filters/
  ├── _quarto.yml           # (empty or minimal)
  ├── crazytalk.lua          # alternates case
  ├── pre-filter.qmd         # filters: [crazytalk.lua]
  └── post-filter.qmd        # filters: [quarto, crazytalk.lua]
  ```

  Each `.qmd` file uses `_quarto.tests` metadata to verify the HTML output contains
  the expected transformed text.

- [x] **5.2** Verify the uppercase filter produces `HELLO WORLD` in the rendered HTML
  (used uppercase.lua instead of crazytalk; simpler and equally effective)

- [x] **5.3** Verify post-filter runs after built-in transforms (post-filter.qmd uses
  `quarto` sentinel to place uppercase.lua after transforms)

### Phase 6: Workspace Verification

- [x] **6.1** `cargo build --workspace`
- [x] **6.2** `cargo nextest run --workspace` — 6793 tests passed, 195 skipped
- [x] **6.3** Manual render verified via smoke tests

### Bug Fix: as_plain_text and custom pipeline

During smoke testing, discovered two issues:
1. Document YAML metadata stores filter names as `PandocInlines`, not `Scalar(String)`.
   Fixed `filter_resolve.rs` to use `as_plain_text()` instead of `as_str()`.
2. `render_qmd_to_html()` had a custom pipeline path (when CSS paths are provided)
   that didn't include `UserFiltersStage`. Added filter stages to both code paths.

---

## Out of Scope

- **WASM support**: Lua filters are disabled for WASM builds. Adding user filter
  stages to the WASM pipeline is a separate effort.
- **Extension resolution**: Filter names that reference extensions (e.g., `lightbox`)
  require extension infrastructure that doesn't exist yet.
- **Finer-grained pipeline positions**: We accept all 8 entry point values but map
  them to only 2 pipeline positions (Pre/Post). Finer-grained execution positions
  can be added later by splitting `AstTransformsStage`.
- **Filter caching**: No caching of Lua state between renders. Each filter
  application creates a fresh Lua interpreter.

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/pampa/src/lib.rs` | Modify | Export `unified_filter` and `json_filter` modules |
| `crates/pampa/src/main.rs` | Modify | Remove private `mod` declarations, use library exports |
| `crates/quarto-core/Cargo.toml` | Modify | Enable `lua-filter` + `json-filter` features on pampa |
| `crates/quarto-core/src/filter_resolve.rs` | Create | Parse `filters` metadata, sort by entry point, split Pre/Post |
| `crates/quarto-core/src/lib.rs` | Modify | Register `filter_resolve` module |
| `crates/quarto-core/src/stage/stages/user_filters.rs` | Create | `UserFiltersStage` implementation |
| `crates/quarto-core/src/stage/stages/mod.rs` | Modify | Register `user_filters` module |
| `crates/quarto-core/src/pipeline.rs` | Modify | Insert filter stages into HTML pipeline |
| `crates/quarto/tests/smoke-all/filters/` | Create | Smoke test directory with filter + qmd files |

## Verified Assumptions

- **Metadata access**: After `MetadataMergeStage`, merged metadata (including `filters`)
  is in `doc.ast.meta` (assigned at `metadata_merge.rs:213`). Access via
  `doc.ast.meta.get("filters")`.
- **Document directory**: `ctx.document.input.parent()` gives the document's directory
  for resolving relative filter paths.
- **Format string**: `ctx.format.identifier.as_str()` returns e.g. `"html"`.
- **`apply_filters` signature**: `(Pandoc, ASTContext, &[FilterSpec], &str) →
  Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), FilterError>` — sync function,
  called inside async stage (acceptable; Lua/JSON filters block the thread).
- **`DocumentAst` fields**: `ast: Pandoc`, `ast_context: ASTContext`, `warnings: Vec<DiagnosticMessage>`.
- **`unified_filter` is binary-only**: Must be moved to `lib.rs` exports (Phase 3).
- **pampa features disabled**: Workspace sets `default-features = false`; quarto-core
  must explicitly enable `lua-filter` and `json-filter` (Phase 3).

## References

- `crates/pampa/src/unified_filter.rs` — `FilterSpec`, `apply_filters`
- `crates/pampa/src/lua/filter.rs` — Lua filter engine
- `crates/quarto-core/src/pipeline.rs:137-147` — current HTML pipeline stages
- `crates/quarto-core/src/stage/stages/ast_transforms.rs` — model for new stage
- `crates/quarto-core/src/stage/data.rs:288-300` — `DocumentAst` struct
- `crates/quarto-core/src/stage/traits.rs:81-139` — `PipelineStage` trait
- `crates/quarto-core/src/stage/stages/metadata_merge.rs:213` — metadata assignment
- `claude-notes/plans/2025-12-05-unified-filter-cli.md` — unified filter CLI design
- `claude-notes/plans/2026-01-24-lua-filter-analysis.md` — filter chain analysis

### TS Quarto References (for entry point compatibility)

- `~/src/quarto-cli/src/command/render/filters.ts:724-802` — `resolveFilters` function
- `~/src/quarto-cli/src/config/constants.ts:807-808` — `kQuartoPre = "pre-quarto"`,
  `kQuartoPost = "post-render"`
- `~/src/quarto-cli/src/config/types.ts:350-370` — `QuartoFilterEntryPoint` type
- `~/src/quarto-cli/src/resources/filters/main.lua:703-729` — entry point execution order
- `~/src/quarto-cli/src/resources/filters/ast/emulatedfilter.lua:39-79` — entry point injection
