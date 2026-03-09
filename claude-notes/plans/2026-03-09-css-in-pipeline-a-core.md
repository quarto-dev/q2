# Plan: CSS in Pipeline — Part A: Core Implementation (Phases 1-2)

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`

This sub-plan covers:
- Phase 1: Fix ThemeConfig to read flattened metadata
- Phase 2: New CompileThemeCssStage with caching

After this plan, the new pipeline stage exists and works, but old code paths
(native pre-pipeline extraction, WASM JS-side compilation) are still in place.
They will be removed in Part B.

## Prerequisites (all completed)

- MetadataMergeStage extracted (commit `853c1c0d`)
- SystemRuntime cache — Rust impl (commit `f357f5ad`)
- SystemRuntime cache — WASM impl (commit `55591f83`)

## Codebase Orientation

Read these files before starting:

- `crates/quarto-sass/src/config.rs` — `ThemeConfig::from_config_value`,
  currently reads `format.html.theme` (needs to change to top-level `theme`)
- `crates/quarto-sass/src/compile.rs` — `compile_theme_css` (native + WASM
  versions), `compile_default_css`, assembly logic to extract
- `crates/quarto-core/Cargo.toml` — `quarto-sass` is native-only dep (line 42)
- `crates/quarto-core/src/stage/stages/apply_template.rs` — creates
  `css:default` artifact with `DEFAULT_CSS` (lines 138-143)
- `crates/quarto-core/src/pipeline.rs` — pipeline builder functions
- `crates/quarto-core/src/stage/stages/mod.rs` — stage module registry

See parent plan for full investigation notes (key findings, resolved risks,
artifact flow, sync/async strategy, etc.).

## Phase 1: Fix ThemeConfig to read flattened metadata

After MetadataMergeStage, the merged config is format-flattened: `theme` is at
top level, not under `format.html.theme`.

**Tests first:**

- [x] In `crates/quarto-sass/src/config.rs`, add new test helpers that put
  `theme` at top level (not nested under `format.html`):
  - `flattened_config_with_theme_string(theme: &str) -> ConfigValue`
  - `flattened_config_with_theme_array(themes: &[&str]) -> ConfigValue`
- [x] Add tests using flattened helpers:
  - `test_from_flattened_config_single_theme` — `{ theme: "darkly" }` works
  - `test_from_flattened_config_array_theme` — `{ theme: ["cosmo", "custom.scss"] }` works
  - `test_from_flattened_config_no_theme` — `{}` returns `default_bootstrap()`
  - `test_from_flattened_config_null_theme` — `{ theme: null }` returns `default_bootstrap()`
- [x] Run tests — they FAILED as expected (still reading `format.html.theme`)

**Implement:**

- [x] Change `ThemeConfig::from_config_value` to look for top-level `theme`
- [x] Update existing tests that use nested `format.html.theme` helpers to use
  flattened helpers. Old helpers removed.
- [x] Update `compile_css_from_config` doc comments to state it expects
  flattened config. Updated `test_compile_css_from_config_with_theme` to pass
  flattened config.
- [x] Run tests — all 139 quarto-sass tests PASS

## Phase 2: New CompileThemeCssStage with caching

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
theme config + custom file contents + Bootstrap version).

The caching wraps the raw SCSS compilation at the call site in the stage.
The stage calls `assemble_theme_scss` to get the SCSS + load paths, then
compiles directly via `compile_scss_with_embedded` (native) or
`runtime.compile_sass` (WASM) — NOT via `compile_theme_css`, which would
redundantly re-assemble. This keeps quarto-sass free of runtime cache
dependencies and makes the caching explicit and testable.

**Preferred approach**: Add `assemble_theme_scss` — a clean factoring that
exposes an existing internal step. The cache key is then
`sha256(assembled_scss + ":minified=" + minified)`.

**Tests first:**

- [x] Create `crates/quarto-core/src/stage/stages/compile_theme_css.rs`
- [x] Add tests:
  - `test_no_theme_uses_default_css`
  - `test_builtin_theme_compiles_css`
  - `test_invalid_theme_falls_back_to_default`
  - `test_cache_hit_skips_compilation`
  - `test_null_theme_uses_default_css`
  - `test_cache_key_deterministic` / `test_cache_key_differs_for_minified` / `test_cache_key_differs_for_content`
- [x] Tests pass (8/8)

**Implement:**

- [x] Move `quarto-sass` from native-only to general deps in `quarto-core/Cargo.toml`
- [x] Add `assemble_theme_scss` public function to `quarto-sass` (returns `(String, Vec<PathBuf>)`)
- [x] Refactor both native and WASM `compile_theme_css` to use `assemble_theme_scss`
- [x] Implement `CompileThemeCssStage` with caching, fallback to DEFAULT_CSS
- [x] Update `ApplyTemplateStage` to skip default if `css:default` already exists
- [x] Wire `CompileThemeCssStage` into:
  - `build_html_pipeline_stages()` (7 stages)
  - `build_wasm_html_pipeline()` (6 stages)
  - `render_qmd_to_html()` inline stage list
  - NOT `parse_qmd_to_ast()` (AST-only)
- [x] Update pipeline stage count assertions
- [x] All tests pass: 6593 workspace tests, 0 failures

## Verification

- [x] `cargo build --workspace` — compiles
- [x] `cargo nextest run --workspace` — 6593 tests pass
- [x] `cargo xtask verify` — WASM builds and hub-client tests pass
