# Plan: Move Theme CSS Compilation into the Render Pipeline

## Overview

Theme CSS compilation currently runs **outside** the render pipeline — in
`render_to_file.rs` (native) and `wasmRenderer.ts` (WASM). Both paths extract
theme config from document frontmatter only, ignoring `_quarto.yml` and
`_metadata.yml`. This means `theme: darkly` in `_quarto.yml` has no effect on
CSS output.

The fix: compile theme CSS inside a new `CompileThemeCssStage` that runs after
`MetadataMergeStage`, where the fully merged metadata (project + directory +
document + runtime) is available. CSS compilation results are cached via the
`SystemRuntime` cache interface to avoid expensive recompilation across renders.

## Prerequisites

- **MetadataMergeStage** must be extracted first (see
  `claude-notes/plans/2026-03-09-metadata-merge-stage.md`). After that stage
  runs, `doc.ast.meta` contains the format-flattened merged config where `theme`
  sits at top level (not nested under `format.html.theme`).
- **SystemRuntime cache interface** must be implemented first (see
  `claude-notes/plans/2026-03-09-runtime-cache.md`). SASS compilation is
  expensive (~200-500ms native, ~1-2s WASM) and the existing codebase caches
  results. The cache interface provides platform-abstracted persistent caching:
  per-project filesystem at `{project_dir}/.quarto/cache/` on native, IndexedDB
  on WASM. The native runtime is configured with the cache dir after project
  discovery via `NativeRuntime::with_cache_dir()`.

## Key Findings from Investigation

### 1. CSS compilation code (`quarto-sass`)

- **Native `compile_theme_css`**: sync (uses grass via `compile_scss_with_embedded`)
- **WASM `compile_theme_css`**: async (uses dart-sass via `runtime.compile_sass()`)
- Both take `ThemeConfig` + `ThemeContext` and return `Result<String, SassError>`
- `ThemeContext` needs: document directory (`PathBuf`) + runtime (`&dyn SystemRuntime`)
- `ThemeConfig::from_config_value` currently reads `format.html.theme` — must
  change to top-level `theme` for flattened metadata

### 2. Dependency chain

- `quarto-sass` in `quarto-core/Cargo.toml` is **native-only** (line 42,
  under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`)
- `quarto-sass` itself has **no native-only main dependencies** — it uses
  `quarto-system-runtime` for platform abstraction
- `wasm-quarto-hub-client` already depends on `quarto-sass` directly
- **Action**: Move `quarto-sass` to general deps in `quarto-core/Cargo.toml`

### 3. Sync/async in pipeline stages

- All pipeline stages implement `#[async_trait]` with async `run()` methods
- Native sync `compile_theme_css` can be called from async context (no problem)
- WASM async `compile_theme_css` needs `.await`
- **Solution (implemented)**: Changed `PipelineStage` trait and all impls to use
  conditional async_trait: `#[cfg_attr(not(target_arch = "wasm32"), async_trait)]`
  / `#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]`. This matches the
  pattern used by `SystemRuntime` and allows WASM stages to await non-Send
  futures from `SystemRuntime` methods (`cache_get`, `cache_set`, `compile_sass`).
  Compilation within the stage uses `#[cfg]` helper functions for platform-specific
  compilation calls.

### 4. Artifact flow

- **WASM**: `render_qmd()` in `lib.rs` writes all pipeline artifacts to VFS
  after completion (lines 722-728). CSS artifact at
  `/.quarto/project-artifacts/styles.css` flows automatically.
- **Native**: `render_to_file.rs` writes CSS to `{stem}_files/styles.css`
  *before* the pipeline. After pipeline, we overwrite that file with the
  artifact content.
- `ApplyTemplateStage` already creates the `css:default` artifact (lines
  138-143 of `apply_template.rs`) with `DEFAULT_CSS`. We replace the content
  with compiled theme CSS.

### 5. JS-side overwrite risk

- `compileAndInjectThemeCss` in `wasmRenderer.ts` runs AFTER `render_qmd()`
  and writes to the same VFS path (`/.quarto/project-artifacts/styles.css`).
- If pipeline produces correct CSS, the JS call **overwrites it** with
  frontmatter-only CSS.
- **Solution**: Remove the JS-side call entirely (Phase 4).

### 6. WASM test infrastructure

- Tests load `.wasm` from disk, `sass` npm package is available
- No graceful fallback if sass unavailable — pipeline should fall back to
  `DEFAULT_CSS` on compilation failure
- Current WASM tests don't exercise theme CSS compilation through the pipeline

### 7. Previous attempt difficulties (all resolved)

| Difficulty | Resolution |
|-----------|------------|
| Merged metadata unavailable in pipeline | MetadataMergeStage (prerequisite) |
| `ThemeConfig` reads `format.html.theme` | Change to top-level `theme` (Phase 1) |
| `quarto-sass` is native-only in quarto-core | Move to general deps (Phase 2) |
| Native compile is sync, WASM is async | Conditional `async_trait(?Send)` on `PipelineStage` + `#[cfg]` helper fns (Phase 2) |
| CSS recompiled every render | RuntimeCache caching (Phase 2, prerequisite) |
| Native double-write to disk | Overwrite after pipeline (Phase 3) |
| JS-side overwrites pipeline CSS | Remove JS call (Phase 4) |

## Work Items

### Phase 1: Fix ThemeConfig to read flattened metadata

After MetadataMergeStage, the merged config is format-flattened: `theme` is at
top level, not under `format.html.theme`.

**Tests first:**

- [ ] In `crates/quarto-sass/src/config.rs`, add new test helpers that put
  `theme` at top level (not nested under `format.html`):
  - `flattened_config_with_theme_string(theme: &str) -> ConfigValue`
  - `flattened_config_with_theme_array(themes: &[&str]) -> ConfigValue`
- [ ] Add tests using flattened helpers:
  - `test_from_flattened_config_single_theme` — `{ theme: "darkly" }` works
  - `test_from_flattened_config_array_theme` — `{ theme: ["cosmo", "custom.scss"] }` works
  - `test_from_flattened_config_no_theme` — `{}` returns `default_bootstrap()`
  - `test_from_flattened_config_null_theme` — `{ theme: null }` returns `default_bootstrap()`
- [ ] Run tests — they should FAIL (still reading `format.html.theme`)

**Implement:**

- [ ] Change `ThemeConfig::from_config_value` to look for top-level `theme`:
  ```rust
  // Before:
  let theme_value = config.get("format")
      .and_then(|f| f.get("html"))
      .and_then(|h| h.get("theme"));
  // After:
  let theme_value = config.get("theme");
  ```
- [ ] Update existing tests that use nested `format.html.theme` helpers to use
  flattened helpers. The old helpers can be removed.
- [ ] Update `compile_css_from_config` doc comments to state it expects
  flattened config. Update `test_compile_css_from_config_with_theme` to pass
  flattened config.
- [ ] Run tests — should PASS

### Phase 2: New CompileThemeCssStage with caching

A new pipeline stage that compiles theme CSS and stores it as an artifact.
Runs after MetadataMergeStage (needs merged metadata) and before
AstTransformsStage (no dependency on AST transforms or HTML rendering).

**Pipeline after this change:**
```
1. ParseDocumentStage    (LoadedSource → DocumentAst)
2. EngineExecutionStage  (DocumentAst → DocumentAst)     [native only]
3. MetadataMergeStage    (DocumentAst → DocumentAst)
4. CompileThemeCssStage  (DocumentAst → DocumentAst)     ← NEW
5. AstTransformsStage    (DocumentAst → DocumentAst)
6. RenderHtmlBodyStage   (DocumentAst → RenderedOutput)
7. ApplyTemplateStage    (RenderedOutput → RenderedOutput)
```

The stage reads `doc.ast.meta` (merged config), compiles CSS, and stores the
result in `ctx.artifacts` as `"css:default"`. `ApplyTemplateStage` is updated
to use the existing artifact instead of always storing `DEFAULT_CSS`.

**Caching strategy:**

The stage uses `ctx.runtime.cache_get("sass", &key)` /
`ctx.runtime.cache_set("sass", &key, css)` to avoid recompilation. The cache
key is a hash of the assembled SCSS bundle (which is deterministic given the
theme config + custom file contents + Bootstrap version). Two pages with the
same effective theme produce the same assembled SCSS → same cache key → one
compilation.

On native, cached CSS is stored at `{project_dir}/.quarto/cache/sass/{key}`,
persisting across render sessions. On WASM, it's stored in IndexedDB. For
single-file renders without a project, the runtime has no cache dir and the
cache methods are no-ops — each render compiles fresh (acceptable since
single-file renders are typically one-off).

The caching wraps the raw SCSS compilation at the call site in the stage.
The stage calls `assemble_theme_scss` to get the SCSS + load paths, then
compiles directly via `compile_scss_with_embedded` (native) or
`runtime.compile_sass` (WASM) — NOT via `compile_theme_css`, which would
redundantly re-assemble. This keeps quarto-sass free of runtime cache
dependencies and makes the caching explicit and testable.

To compute the cache key without duplicating assembly work, we need a way to
get the assembled SCSS without compiling it. Options:
- Add `assemble_theme_scss(config, context) -> String` to quarto-sass public API
  (extracts the assembly step from `compile_theme_css`)
- Or: hash the `ThemeConfig` deterministically (theme names + custom file
  contents). This is simpler but slightly less precise (doesn't capture
  Bootstrap resource version changes).

**Preferred**: Add `assemble_theme_scss` — it's a clean factoring that exposes
an existing internal step. The cache key is then `sha256(assembled_scss + minified)`.

**Tests first:**

- [ ] Create `crates/quarto-core/src/stage/stages/compile_theme_css.rs`
- [ ] Add tests using a mock runtime with controllable `compile_sass` and
  `cache_get`/`cache_set`:
  - `test_no_theme_uses_default_css` — metadata has no `theme`, artifact
    contains `DEFAULT_CSS`
  - `test_builtin_theme_compiles_css` — metadata has `theme: "cosmo"`,
    artifact is NOT `DEFAULT_CSS`
  - `test_compile_error_falls_back_to_default` — `compile_sass` returns
    error, artifact contains `DEFAULT_CSS`
  - `test_cache_hit_skips_compilation` — `cache_get` returns CSS,
    `compile_sass` is NOT called, artifact contains cached CSS
  - `test_cache_miss_compiles_and_stores` — `cache_get` returns None,
    `compile_sass` is called, result stored via `cache_set`
  - `test_cache_error_still_compiles` — `cache_get` fails, compilation
    proceeds normally (cache is best-effort)
- [ ] Run tests — should FAIL

**Implement:**

- [ ] Move `quarto-sass` from native-only to general deps in
  `crates/quarto-core/Cargo.toml`:
  ```toml
  # Move from [target.'cfg(not(...))'.dependencies] to [dependencies]
  quarto-sass.workspace = true
  ```
- [ ] Add `assemble_theme_scss` public function to `quarto-sass`:
  - Signature: `fn assemble_theme_scss(config: &ThemeConfig, context: &ThemeContext) -> Result<(String, Vec<PathBuf>), SassError>`
  - Returns the assembled SCSS string **and** the load paths needed for
    compilation (custom theme directories, etc.)
  - Only called when `config.has_themes()` is true; the no-theme path
    short-circuits to `DEFAULT_CSS` in the stage without calling this
  - This is a refactoring of existing logic in `compile_theme_css` — extract
    the assembly step before the `compile_scss_with_embedded` / `runtime.compile_sass` call
- [ ] Implement `CompileThemeCssStage`:
  - `input_kind() → DocumentAst`, `output_kind() → DocumentAst`
  - In `run()`:
    1. Extract `ThemeConfig` from `doc.ast.meta`
    2. If no themes (`!config.has_themes()`): store `DEFAULT_CSS` as
       artifact and return early (no compilation needed)
    3. Assemble SCSS via `assemble_theme_scss` → `(scss, load_paths)`
    4. Compute cache key: `sha256(scss + ":minified=" + minified)`
    5. Check cache: `ctx.runtime.cache_get("sass", &key).await`
    6. On hit: use cached CSS
    7. On miss: compile the assembled SCSS directly —
       native: `compile_scss_with_embedded(runtime, &resources, &scss, &load_paths, minified)`
       WASM: `runtime.compile_sass(&scss, &load_paths, minified).await`
       (NOT `compile_theme_css`, which would re-assemble)
    8. Store in cache: `ctx.runtime.cache_set("sass", &key, css).await`
    9. On compile error: fall back to `DEFAULT_CSS`
    10. Store as artifact: `ctx.artifacts.store("css:default", ...)`
  - Pass `DocumentAst` through unchanged (stage only produces a side-effect
    artifact)
- [ ] Update `ApplyTemplateStage` to check for existing `"css:default"` artifact
  before storing `DEFAULT_CSS`. If the artifact already exists (set by
  `CompileThemeCssStage`), skip the default. If not (e.g., stage was skipped),
  store `DEFAULT_CSS` as fallback.
- [ ] Wire `CompileThemeCssStage` into pipeline builders in `pipeline.rs`:
  - `build_html_pipeline_stages()`: insert after MetadataMergeStage
  - `build_wasm_html_pipeline()`: insert after MetadataMergeStage
  - `render_qmd_to_html()`: also insert in the inline stage list at
    lines 372-379 (the branch for custom CSS/template), after MetadataMergeStage
  - `parse_qmd_to_ast()`: do NOT insert (AST-only pipeline, no CSS needed)
- [ ] Update pipeline stage count assertions in tests
- [ ] Run tests — should PASS

### Phase 3: Remove native CLI pre-pipeline theme extraction

Current native flow:
1. `write_themed_resources` compiles CSS, writes to `{stem}_files/styles.css`
2. Passes `css_paths` to pipeline
3. Pipeline uses paths in `<link>` tags

New native flow:
1. `write_html_resources` creates `{stem}_files/` dir, writes DEFAULT_CSS
   placeholder, returns css_paths
2. Pipeline compiles real theme CSS in `CompileThemeCssStage`, stores as
   artifact (cached at `{project_dir}/.quarto/cache/sass/{key}`)
3. After pipeline returns, extract `css:default` artifact and overwrite
   `{stem}_files/styles.css`

**Work items:**

- [ ] In `crates/quarto-core/src/render_to_file.rs`:
  - Remove `extract_theme_config` and `theme_value_to_config` functions
  - Remove `write_themed_resources` function
  - Replace call to `write_themed_resources` with `write_html_resources`
  - After `render_qmd_to_html` returns, extract `css:default` artifact from
    the render context and overwrite `{stem}_files/styles.css` with its content
  - **Runtime setup**: Change runtime construction to use
    `NativeRuntime::with_cache_dir(project.dir.join(".quarto/cache"))` so the
    pipeline's `CompileThemeCssStage` can use the cache. For single-file renders
    (no project), use `NativeRuntime::new()` (no caching — acceptable).
- [ ] **Artifact access**: `render_qmd_to_html` currently returns `RenderOutput`
  but artifacts live in `RenderContext`. Check how artifacts are returned.
  The `run_pipeline` function in `pipeline.rs` transfers artifacts back to
  `RenderContext` (line ~262: `ctx.artifacts = stage_ctx.artifacts`). So after
  `render_qmd_to_html`, artifacts should be accessible via `ctx.artifacts`.
  If `render_qmd_to_html` doesn't return the context, we may need to modify it
  to also return the artifact store (or return the full context).
- [ ] Remove `write_html_resources_with_sass` from `resources.rs`
- [ ] Run tests — verify native rendering still works

### Phase 4: Remove WASM JS-side theme compilation

The pipeline now produces correct theme CSS in the `css:default` artifact.
WASM `render_qmd()` already writes artifacts to VFS. No JS-side compilation
needed.

- [ ] In `hub-client/src/services/wasmRenderer.ts`:
  - Remove `compileAndInjectThemeCss` function
  - Remove `extractThemeConfigForCacheKey` function
  - Remove the call to `compileAndInjectThemeCss` in `renderToHtml()` (around
    lines 706-728). The `renderQmd()` call already produces correct CSS.
  - Update `themeVersion` tracking — the `renderToHtml` function uses the
    return value of `compileAndInjectThemeCss` as a change-detection key. After
    removal, theme changes are detected through the normal render path (theme
    config is in the merged metadata, which affects the HTML output hash).
- [ ] In `crates/wasm-quarto-hub-client/src/lib.rs`:
  - Remove `compile_document_css` WASM entry point
  - Remove `compute_theme_content_hash` WASM entry point
  - Keep `compile_scss`, `compile_default_bootstrap_css`, `compile_theme_css_by_name`,
    `sass_available`, `sass_compiler_name`, `get_scss_resources_version` — these
    may still be used by other code paths (settings panel, manual compilation)
- [ ] In `hub-client/src/types/wasm-quarto-hub-client.d.ts`:
  - Remove TypeScript declarations for removed WASM functions
- [ ] Evaluate `hub-client/src/services/sassCache.ts`:
  - The cache is used by `compileScss`, `compileDocumentCss`,
    `compileThemeCssByName`, `compileDefaultBootstrapCss`
  - If only `compileDocumentCss` is removed but others remain, keep the cache
  - If all callers are removed, remove the cache entirely
  - **Likely outcome**: Keep it — `compileThemeCssByName` and others are used
    by the theme settings UI
- [ ] Run hub-client tests

### Phase 5: Integration and E2E tests

#### Native integration tests (`crates/quarto-core/`)

Using full pipeline with `NativeRuntime` + grass SASS compiler:

- [ ] `test_render_pipeline_theme_from_project` — `_quarto.yml` has
  `format: { html: { theme: darkly } }`, bare `doc.qmd`. Assert CSS artifact
  is NOT `DEFAULT_CSS` and contains darkly-specific values.
- [ ] `test_render_pipeline_theme_from_document_overrides_project` — project
  has `theme: darkly`, document has `theme: flatly`. Assert CSS contains
  flatly values, not darkly.
- [ ] `test_render_pipeline_no_theme_uses_compiled_default` — no theme
  anywhere. Assert CSS is compiled Bootstrap (from `compile_default_css`).

#### WASM E2E tests (`hub-client/src/services/`)

New file `themeInheritance.wasm.test.ts` following existing patterns:

- [ ] **Project theme**: `_quarto.yml` has `theme: darkly`, `doc.qmd` has none.
  Assert CSS artifact contains darkly-specific values.
- [ ] **Document overrides project**: `_quarto.yml` has `theme: darkly`,
  `doc.qmd` has `theme: flatly`. Assert CSS contains flatly, not darkly.
- [ ] **Directory metadata theme**: `chapters/_metadata.yml` has `theme: sketchy`,
  `chapters/doc.qmd` has none. Assert CSS contains sketchy.
- [ ] **No theme anywhere**: Assert CSS is default Bootstrap.
- [ ] **Runtime metadata overrides all**: `vfs_set_runtime_metadata` with
  `theme: darkly`, document has `theme: flatly`. Assert CSS contains darkly.

**Detection strategy**: Each Bootswatch theme produces distinctive CSS. Before
writing tests, compile a few themes to identify reliable detection strings
(e.g., darkly uses `$body-bg: #222`, sketchy has hand-drawn borders).

### Phase 6: Verification

- [ ] `cargo nextest run --workspace` — all tests pass
- [ ] `cargo xtask verify` — WASM and hub-client build and test
- [ ] Manual: `theme: darkly` in `_quarto.yml`, verify in hub-client
- [ ] Manual: `theme: sketchy` in frontmatter overrides project theme
- [ ] Manual: native CLI `quarto render` with theme in `_quarto.yml`

## Resolved Risks

### 1. Artifact access from render_to_file (Phase 3)

**Status: Resolved.** `render_qmd_to_html` takes `ctx: &mut RenderContext<'_>`.
After the call returns, `ctx.artifacts` contains the pipeline's artifacts
(transferred back at `pipeline.rs:262` via `ctx.artifacts = stage_ctx.artifacts`).
The `ctx` variable remains in scope after line 233 of `render_to_file.rs`, so
we can directly access `ctx.artifacts.get("css:default")` and overwrite the
CSS file. No API changes needed.

### 2. Cache key correctness (known limitation)

The cache key is `sha256(assembled_scss + ":minified=" + minified)`. This is
correct as long as the assembled SCSS is fully deterministic given the inputs.
If Bootstrap resources change (e.g., after a `wasm-quarto-hub-client` update),
the assembled SCSS will be different → new cache key → automatic invalidation.

**Known limitation**: Custom `.scss` files are resolved relative to
`document_dir`. If a user edits `custom.scss`, the assembled SCSS changes →
new cache key. But if they edit a file `@import`-ed by `custom.scss`, the
assembled SCSS may NOT change (we only read top-level files into layers). TS
Quarto handles this by using a session cache (cleared per render session) for
SCSS with `@import`.

**Accepted for v1**: Custom `@import` chains are rare and the per-project cache
at `.quarto/cache/sass/` is easily cleared (`rm -rf .quarto/cache` or future
`quarto clean`). A future enhancement could hash all transitively imported
files.

### 3. Custom .scss file resolution in WASM

**Status: Resolved (no new risk).** Hub-client already populates project files
in VFS before rendering. Custom `.scss` files in the project will be available
via VFS. `ThemeContext` resolves paths relative to the document directory, which
works in both native (real filesystem) and WASM (VFS). This is existing
behavior — the pipeline change doesn't affect it.

### 4. `assemble_theme_scss` refactoring

**Status: Resolved (clean separation confirmed).** In `compile_theme_css`
(both native and WASM versions), lines 96-111 are the assembly step:
`process_theme_specs` → `load_title_block_layer` → `assemble_with_user_layers`.
This is platform-independent code that produces a SCSS string. The compilation
step that follows (native: `compile_scss_with_embedded`, WASM:
`runtime.compile_sass`) is platform-specific.

The refactoring is: extract the assembly into `assemble_theme_scss(config,
context) -> Result<(String, Vec<PathBuf>), SassError>` returning the SCSS
string and load paths. Both `compile_theme_css` variants call it internally,
so they can't diverge. The function is platform-independent (no `#[cfg]`).

## Files Modified (Summary)

| File | Phase | Change |
|------|-------|--------|
| `crates/quarto-sass/src/config.rs` | 1 | Read top-level `theme` instead of `format.html.theme` |
| `crates/quarto-sass/src/compile.rs` | 2 | Extract `assemble_theme_scss` public function |
| `crates/quarto-core/Cargo.toml` | 2 | Move `quarto-sass` to general deps |
| New: `crates/quarto-core/src/stage/stages/compile_theme_css.rs` | 2 | New pipeline stage with caching |
| `crates/quarto-core/src/stage/stages/mod.rs` | 2 | Register new stage module |
| `crates/quarto-core/src/pipeline.rs` | 2 | Insert CompileThemeCssStage in pipeline builders |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | 2 | Use existing `css:default` artifact if present |
| `crates/quarto-core/src/stage/traits.rs` | 2 | Conditional `async_trait(?Send)` for WASM |
| All `impl PipelineStage` files | 2 | Same conditional `async_trait` on each impl |
| `crates/wasm-quarto-hub-client/src/lib.rs` | 2 | `resolve_format_config` in `compute_theme_content_hash` |
| `crates/wasm-quarto-hub-client/Cargo.toml` | 2 | Added `quarto-config` dep (for above) |
| `crates/quarto-core/src/render_to_file.rs` | 3 | Remove pre-pipeline theme extraction, overwrite CSS after pipeline, configure runtime with cache dir |
| `crates/quarto-core/src/resources.rs` | 3 | Remove `write_html_resources_with_sass` |
| `hub-client/src/services/wasmRenderer.ts` | 4 | Remove `compileAndInjectThemeCss` and related |
| `crates/wasm-quarto-hub-client/src/lib.rs` | 4 | Remove `compile_document_css`, `compute_theme_content_hash` |
| `hub-client/src/types/wasm-quarto-hub-client.d.ts` | 4 | Remove TS declarations for removed functions |
| New: `hub-client/src/services/themeInheritance.wasm.test.ts` | 5 | WASM E2E tests for theme inheritance |
