# Plan: CSS in Pipeline — Part B: Migration (Phases 3-4)

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: `claude-notes/plans/2026-03-09-css-in-pipeline-a-core.md`

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

## Phase 4: Remove WASM JS-side theme compilation

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
  - Remove `compute_theme_content_hash` WASM entry point (and its
    `resolve_format_config` call added in Part A)
  - Keep `compile_scss`, `compile_default_bootstrap_css`, `compile_theme_css_by_name`,
    `sass_available`, `sass_compiler_name`, `get_scss_resources_version` — these
    may still be used by other code paths (settings panel, manual compilation)
- [ ] In `crates/wasm-quarto-hub-client/Cargo.toml`:
  - Remove `quarto-config` dependency (added in Part A only for
    `compute_theme_content_hash`; verify no other code uses it first)
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

## Verification

- [ ] `cargo build --workspace` — compiles
- [ ] `cargo nextest run --workspace` — all tests pass
- [ ] `cargo xtask verify` — WASM builds and hub-client tests pass

## Reference

See parent plan for resolved risks:
- Artifact access from render_to_file (Risk 1 — resolved)
- Custom .scss file resolution in WASM (Risk 3 — resolved)
