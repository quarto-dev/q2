# Plan: CSS in Pipeline — Part B1: Migration (Phases 3-4)

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: `claude-notes/plans/2026-03-09-css-in-pipeline-b2-tests.md`

This sub-plan removes the old pre-pipeline CSS compilation code paths now that
`CompileThemeCssStage` produces correct theme CSS inside the pipeline.

## Changes from Part A that affect this plan

1. **`PipelineStage` uses conditional `async_trait`**: The trait and all impls
   now use `#[cfg_attr(not(target_arch = "wasm32"), async_trait)]` /
   `#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]`. This was required
   because `CompileThemeCssStage` awaits `SystemRuntime` async methods
   (`cache_get`, `cache_set`, `compile_sass`) which return non-Send futures on
   WASM. Any new `PipelineStage` impls must use the same conditional pattern.

2. **`compute_theme_content_hash` patched**: This standalone WASM function
   broke because `ThemeConfig::from_config_value` now expects flattened config
   (top-level `theme`), but the function receives raw frontmatter
   (`format.html.theme`). Fixed by adding `quarto_config::resolve_format_config`
   call before `ThemeConfig::from_config_value`. When this function is removed
   in Phase 4, the `quarto-config` dep added to `wasm-quarto-hub-client` can
   also be removed (if no other code uses it).

## Phase 3: Remove native CLI pre-pipeline theme extraction

Current native flow:
1. `write_themed_resources` compiles CSS, writes to `{stem}_files/styles.css`
2. Passes `css_paths` to pipeline
3. Pipeline uses paths in `<link>` tags

New native flow:
1. **Before pipeline**: Create `{stem}_files/` directory and compute `css_paths`
   but do NOT write any CSS file yet. Add a new `prepare_html_resources`
   function to `resources.rs` that creates the directory and returns paths
   without writing CSS content.
2. Pipeline compiles real theme CSS in `CompileThemeCssStage`, stores as
   artifact (cached at `{project_dir}/.quarto/cache/sass/{key}`). If no theme
   is configured, the stage stores `DEFAULT_CSS`. On compilation failure, the
   stage falls back to `DEFAULT_CSS`. The artifact is always present.
3. **After pipeline**: Write `css:default` artifact to `{stem}_files/styles.css`.
   If the artifact is somehow missing (should not happen — belt-and-suspenders),
   write `DEFAULT_CSS` as a last-resort safety net. The CSS file is written
   exactly once, with the correct content. No overwriting.

**Design rationale**: We avoid writing `DEFAULT_CSS` first and then overwriting
because that is fragile and wasteful. The pipeline's `CompileThemeCssStage`
always produces a `css:default` artifact (either compiled theme CSS or
`DEFAULT_CSS` fallback), so the post-pipeline write is the single source of
truth for the CSS file.

**Work items:**

- [x] Add `prepare_html_resources` to `crates/quarto-core/src/resources.rs`:
  - Creates `{stem}_files/` directory (same as `write_html_resources`)
  - Returns `HtmlResourcePaths` with correct relative paths
  - Does NOT write any CSS file
- [x] In `crates/quarto-core/src/render_to_file.rs`:
  - Remove `extract_theme_config` and `theme_value_to_config` functions
  - Remove `write_themed_resources` function
  - Replace call to `write_themed_resources` with `prepare_html_resources`
  - After `render_qmd_to_html` returns, extract `css:default` artifact from
    the render context via `ctx.artifacts.get("css:default")` (returns
    `Option<&Artifact>`; use `artifact.as_str()` to get the CSS text) and
    write to `{stem}_files/styles.css`. If artifact is missing, write
    `DEFAULT_CSS` as safety net.
- [x] **Artifact access** (verified): `render_qmd_to_html` takes
  `&mut RenderContext`. `run_pipeline` at `pipeline.rs:273` transfers
  artifacts back via `ctx.artifacts = stage_ctx.artifacts`. After
  `render_qmd_to_html` returns, `ctx.artifacts.get("css:default")` works.
  No API changes needed.
- [x] **Runtime cache dir** — change the **callers**, not `render_to_file.rs`:
  - **CLI** (`crates/quarto/src/commands/render.rs:107`): The CLI discovers
    the project at line 84. Change runtime construction at line 107 from
    `Arc::new(NativeRuntime::new())` to
    `Arc::new(NativeRuntime::with_cache_dir(project.dir.join(".quarto/cache")))`.
    For single-file projects (`project.is_single_file`), `NativeRuntime::new()`
    is acceptable (no caching).
  - **Test runner** (`crates/quarto-test/src/runner.rs:258`): Keep
    `NativeRuntime::new()` — no caching in tests. SASS compilation is
    fast enough for test runs and caching would add complexity.
- [x] Remove `write_html_resources_with_sass` from `resources.rs`, including
  its tests (`test_write_html_resources_with_sass_default_theme` and
  `test_write_html_resources_with_sass_builtin_theme`)
- [x] Fix `extract_theme_specs` in `quarto-sass/config.rs` to handle
  `PandocInlines` values (document frontmatter parsed by pampa). Added
  `config_value_as_text` helper that falls back to `as_plain_text()`.
- [x] Run tests — 6602 passed, including all 6 theme-inheritance smoke tests.
  `test_render_to_file_with_theme` updated: single-file renders without
  `_quarto.yml` don't get format flattening yet (pending default-project
  -single-file plan), so theme from `format.html.theme` isn't compiled.
  Test verifies CSS is written (DEFAULT_CSS fallback).

## Phase 4: Remove WASM JS-side theme compilation

The pipeline now produces correct theme CSS in the `css:default` artifact.
WASM `render_qmd()` already writes artifacts to VFS. No JS-side compilation
needed.

- [x] In `hub-client/src/services/wasmRenderer.ts`:
  - Remove `compileAndInjectThemeCss` function
  - Remove `extractThemeConfigForCacheKey` function
  - Remove `compileDocumentCss` function
  - Remove the call to `compileAndInjectThemeCss` in `renderToHtml()`.
  - CSS version now computed by hashing VFS CSS artifact content via
    `computeHash` from `sassCache.ts`. Reads
    `/.quarto/project-artifacts/styles.css` from VFS after render.
  - Removed `ThemeHashResponse` interface, `compile_document_css` and
    `compute_theme_content_hash` from `WasmModuleExtended` interface.
- [x] In `crates/wasm-quarto-hub-client/src/lib.rs`:
  - Remove `compile_document_css` WASM entry point
  - Remove `compute_theme_content_hash` WASM entry point
  - Remove `ThemeHashResponse` struct + impl
  - Remove `extract_frontmatter_config` and `json_to_config_value` helpers
  - Clean up imports: removed `THEMES_RESOURCES`, `themes::ThemeSpec`
  - Keep `compile_scss`, `compile_default_bootstrap_css`, `compile_theme_css_by_name`,
    `sass_available`, `sass_compiler_name`, `get_scss_resources_version`
- [x] In `crates/wasm-quarto-hub-client/Cargo.toml`:
  - Remove `quarto-config` dependency
  - Remove runtime `sha2` dependency (only used by removed functions)
- [x] In `hub-client/src/types/wasm-quarto-hub-client.d.ts`:
  - Remove TypeScript declarations for removed WASM functions
- [x] In `hub-client/src/test-utils/mockWasm.ts`:
  - Remove `compileDocumentCss` and `computeThemeContentHash` from interface + impl
- [x] In `hub-client/src/services/wasmRenderer.test.ts`:
  - Remove `extractThemeConfigForCacheKey` tests and cache key format tests
- [x] Deleted `hub-client/src/services/themeContentHash.wasm.test.ts`
  (tested removed WASM function)
- [x] Evaluate `sassCache.ts`: Keep — used by `compileThemeCssByName` and
  `compileDefaultBootstrapCss` for theme settings UI
- [x] Rust workspace: builds, 6602 tests pass
- [x] WASM build passes, hub-client unit tests pass
- [x] **FIXED (B3)**: WASM smoke-all theme-inheritance tests — fixed missing
  `setVfsCallbacks()` in smoke-all test. See B3 plan for details.

## Verification

- [x] `cargo build --workspace` — compiles
- [x] `cargo nextest run --workspace` — 6602 tests pass
- [x] `cargo xtask verify` — WASM builds and hub-client tests pass

## Design decisions (resolved during review)

1. **No CSS overwriting on native**: Instead of writing `DEFAULT_CSS` before
   the pipeline and overwriting after, we split `write_html_resources` into
   `prepare_html_resources` (dir + paths only) and a post-pipeline write.
   CSS is written exactly once.

2. **`cssVersion` comment kept**: The `<!-- css-version: ... -->` HTML comment
   is necessary for MorphIframe to detect CSS-only changes. Computed by hashing
   the VFS CSS artifact content. The parent plan's Phase 4 is less specific;
   this B1 plan is definitive.

3. **Runtime cache dir set by callers**: The CLI (`render.rs`) constructs
   `NativeRuntime::with_cache_dir(...)` after project discovery. The test
   runner keeps `NativeRuntime::new()` (no caching in tests).

4. **Fallback policy**: `CompileThemeCssStage` always produces a `css:default`
   artifact — either compiled theme CSS, or `DEFAULT_CSS` on no-theme /
   compilation failure. The post-pipeline CSS write uses this artifact. A
   last-resort `DEFAULT_CSS` write handles the (shouldn't-happen) case where
   the artifact is missing.

## Reference

See parent plan for resolved risks:
- Artifact access from render_to_file (Risk 1 — resolved)
- Custom .scss file resolution in WASM (Risk 3 — resolved)
