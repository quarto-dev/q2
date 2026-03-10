# Plan: CSS in Pipeline — Part B3: Fix WASM CompileThemeCssStage

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: B1 Phase 3 (native migration) complete, B1 Phase 4 (JS-side removal) complete.

## Problem

After removing the JS-side `compileAndInjectThemeCss` overwrite (B1 Phase 4),
the 6 theme-inheritance smoke-all WASM tests fail. The pipeline's
`CompileThemeCssStage` is falling back to `DEFAULT_CSS` instead of producing
compiled theme CSS on WASM. Native works correctly (all 6602 Rust tests pass).

The 6 failing tests (all `ensureCssRegexMatches` assertions):
- `metadata/theme-inheritance/root-doc.qmd` — wants darkly (`#375a7f`)
- `metadata/theme-inheritance/chapters/chapter1.qmd` — wants flatly (`#2c3e50`)
- `metadata/theme-inheritance/chapters/chapter2.qmd` — wants cosmo (`#2780e3`)
- `metadata/theme-inheritance/chapters/deep/deep-doc.qmd` — wants flatly (`#2c3e50`)
- `metadata/theme-inheritance/appendix/appendix-doc.qmd` — wants darkly (`#375a7f`)
- `metadata/theme-inheritance/appendix/custom/custom-doc.qmd` — wants sketchy (`Neucha`)

## Background: How WASM Rendering Works

Understanding the full flow is critical:

1. **Test setup** (`smokeAll.wasm.test.ts`): `populateVfs()` finds the project
   root by walking up looking for `_quarto.yml`. Then `readAllFiles()` recursively
   reads ALL files in the project and adds them to VFS at `/project/{relative}`.
   So `_quarto.yml`, all `_metadata.yml` files, and all `.qmd` files are in VFS.

2. **WASM render entry** (`lib.rs:648`): `render_qmd(path)` reads the file from
   VFS, calls `ProjectContext::discover(path, runtime)` to find project config,
   then calls `render_qmd_to_html()`.

3. **Pipeline selection** (`pipeline.rs:373`): When `HtmlRenderConfig::default()`
   is used (WASM path — empty `css_paths`, no template), it calls
   `build_html_pipeline_stages()` (NOT `build_wasm_html_pipeline()`). Both include
   `CompileThemeCssStage`. The difference is `build_html_pipeline_stages` also has
   `EngineExecutionStage` (which is a no-op for WASM in practice).

4. **Artifact flow** (`lib.rs:721-727`): After pipeline completes, artifacts are
   written to VFS: `runtime.add_file(artifact_path, artifact.content.clone())`.
   The CSS artifact path is `/.quarto/project-artifacts/styles.css`.

5. **CSS assertion** (`smokeAll.wasm.test.ts:414-451`): Parses rendered HTML for
   `<link rel="stylesheet">` hrefs, reads each from VFS, concatenates, and runs
   regex assertions.

## Root Cause Hypothesis

`CompileThemeCssStage` (at `crates/quarto-core/src/stage/stages/compile_theme_css.rs`)
has a `run()` method that:
1. Extracts `ThemeConfig` from `doc.ast.meta` (line 99)
2. If no themes → stores `DEFAULT_CSS` (line 114-121)
3. Assembles SCSS (line 132)
4. Checks cache (line 149)
5. Compiles via platform-specific helper (line 171)
6. On ANY error → falls back to `DEFAULT_CSS` (lines 101-110, 134-143, 180-187)

The stage has **three silent fallback paths** that produce `DEFAULT_CSS`:
- `ThemeConfig::from_config_value` returns error → DEFAULT_CSS (with warn trace)
- `assemble_theme_scss` returns error → DEFAULT_CSS (with warn trace)
- `compile_scss` returns error → DEFAULT_CSS (with warn trace)

On WASM, the compile path calls `ctx.runtime.compile_sass()` (line 243-244).
This is the `WasmRuntime::compile_sass` method which bridges to the dart-sass
JS compiler via `js_compile_sass_impl`.

**Critical detail**: The `SystemRuntime` trait's DEFAULT implementation of
`compile_sass` returns `Err(RuntimeError::NotSupported(...))`. Only `WasmRuntime`
and `NativeRuntime` override it. Make sure the runtime passed through the pipeline
is actually a `WasmRuntime`, not something else with the default impl.

**Also critical**: The old JS-side `compileAndInjectThemeCss` worked — SASS
compilation IS available in WASM. And `compile_theme_css_by_name` (still in
`lib.rs`) also works. So the SASS bridge is functional; the question is whether
it works from within the pipeline context.

The stage logs warnings via `trace_event!` but these are NOT visible in WASM
test output by default. This makes failures completely silent.

## Investigation Steps

### Step 1: Make stage errors visible in WASM tests

Add temporary console logging to `CompileThemeCssStage::run()` to see which
path is taken. Alternatively, write a focused WASM test that renders a
single document with a theme and inspects the VFS CSS content directly.

Quick diagnostic approach: In the WASM smoke-all test runner
(`hub-client/src/services/smokeAll.wasm.test.ts`), after rendering
`root-doc.qmd`, read `/.quarto/project-artifacts/styles.css` from VFS and
log the first 200 chars of its content. If it's `DEFAULT_CSS`, the pipeline
is falling back.

### Step 2: Check if MetadataMergeStage produces correct merged metadata

The stage reads `theme` from `doc.ast.meta`. If `MetadataMergeStage` isn't
running or isn't merging project config correctly in WASM, the theme won't
be in the metadata.

Key question: Does `ProjectContext::discover()` find `_quarto.yml` in VFS?

Check `crates/quarto-system-runtime/src/wasm.rs` for how `discover` works
with VFS paths. The smoke-all test calls `populateVfs(testFile)` which adds
project files to VFS. Verify that `_quarto.yml` gets added at the right path
and that `ProjectContext::discover()` can find it.

The `render_qmd` function in `crates/wasm-quarto-hub-client/src/lib.rs` at
line 668 calls `ProjectContext::discover(path, runtime)`. Check if this
finds the `_quarto.yml` for the theme-inheritance fixtures.

**Important detail for theme inheritance**: The 6 failing tests are
specifically about theme *inheritance* across subdirectories. Each subdir
has its own `_metadata.yml` specifying a different theme. `MetadataMergeStage`
at line 162 calls `directory_metadata_for_document(&ctx.project, &ctx.document.input, ctx.runtime.as_ref())`
to discover `_metadata.yml` files between the project root and the document
directory. This function (in `crates/quarto-core/src/project.rs`) needs to
list directories and read files on VFS. If VFS doesn't support directory
listing properly, the directory metadata layers won't be found — and the
per-subdirectory theme overrides won't reach the merged metadata. This
could explain why ALL 6 tests fail even though the project-level
`_quarto.yml` has a default theme (the root-level theme might not be under
`theme:` directly but rather the subdirectory `_metadata.yml` files provide
the themes).

### Step 3: Check if compile_sass works in pipeline context

The WASM `compile_scss` helper (line 236-247 of `compile_theme_css.rs`):
```rust
#[cfg(target_arch = "wasm32")]
async fn compile_scss(
    ctx: &StageContext,
    scss: &str,
    load_paths: &[PathBuf],
    minified: bool,
) -> Result<String, String> {
    ctx.runtime
        .compile_sass(scss, load_paths, minified)
        .await
        .map_err(|e| e.to_string())
}
```

This calls `SystemRuntime::compile_sass()`. Check `WasmRuntime`'s impl of
this method. It likely bridges to a JS function. Verify it's wired up and
actually called. The old JS-side `compileAndInjectThemeCss` also used SASS
compilation and worked — so SASS is available, but maybe the runtime context
differs.

### Step 4: Check `assemble_theme_scss` on WASM

`assemble_theme_scss` in `quarto-sass` resolves theme files. On WASM, custom
theme resolution uses VFS. Built-in themes use embedded resources
(`THEMES_RESOURCES`, `BOOTSTRAP_RESOURCES`). These should work identically
on both platforms, but verify.

### Step 5: Check artifact flow from pipeline to VFS

In `crates/wasm-quarto-hub-client/src/lib.rs` lines 721-727:
```rust
for (_key, artifact) in ctx.artifacts.iter() {
    if let Some(artifact_path) = &artifact.path {
        runtime.add_file(artifact_path, artifact.content.clone());
    }
}
```

Verify the `css:default` artifact has a `path` set. It should — both
`store_default_css` and `store_css` in `compile_theme_css.rs` use
`.with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH))` where
`DEFAULT_CSS_ARTIFACT_PATH = "/.quarto/project-artifacts/styles.css"`.

### Step 6: Check CSS resolution in smoke-all test

The test reads CSS from VFS via `<link>` hrefs in the HTML. When
`HtmlRenderConfig::default()` is used (WASM path), `css_paths` is empty,
so `ApplyTemplateStage` uses `DEFAULT_CSS_ARTIFACT_PATH` as the href.
The test resolver at `smokeAll.wasm.test.ts:423`:
```typescript
const vfsPath = href.startsWith('/') ? href : `/project/${href}`;
```
For `/.quarto/project-artifacts/styles.css`, this keeps the path as-is.

## Key Files

- `crates/quarto-core/src/stage/stages/compile_theme_css.rs` — the stage
- `crates/quarto-core/src/pipeline.rs` — pipeline builders, `DEFAULT_CSS_ARTIFACT_PATH`
- `crates/quarto-core/src/stage/stages/metadata_merge.rs` — metadata merge
- `crates/wasm-quarto-hub-client/src/lib.rs:648-765` — `render_qmd` function
- `crates/quarto-system-runtime/src/wasm.rs` — `WasmRuntime` impl
- `hub-client/src/services/smokeAll.wasm.test.ts` — WASM test runner
- `crates/quarto/tests/smoke-all/metadata/theme-inheritance/` — test fixtures (untracked)

## Recommended Approach

1. Write a minimal focused WASM test (in `hub-client/src/services/`) that:
   - Adds `_quarto.yml` with `theme: darkly` and a bare `doc.qmd` to VFS
   - Calls `render_qmd()`
   - Reads `/.quarto/project-artifacts/styles.css` from VFS
   - Logs the first 500 chars of the CSS
   - Asserts it contains `#375a7f` (darkly's primary color)

2. If the CSS is `DEFAULT_CSS`, add `console.log` instrumentation in the
   Rust `CompileThemeCssStage::run()` using `web_sys::console::log_1`
   (available in the WASM crate) to trace which path is taken.

3. Fix the root cause. Likely candidates:
   - `compile_sass` bridge not working in pipeline context
   - `MetadataMergeStage` not finding/merging `_quarto.yml` theme
   - `assemble_theme_scss` failing on WASM for path resolution reasons

4. Once the focused test passes, run the full smoke-all suite:
   ```bash
   cd hub-client && npx vitest run --config vitest.wasm.config.ts src/services/smokeAll.wasm.test.ts
   ```

5. Then run full verification:
   ```bash
   cargo xtask verify
   ```

## Current State of the Codebase

Phase 4 code changes are complete (but not committed):
- `hub-client/src/services/wasmRenderer.ts` — removed `compileAndInjectThemeCss`,
  `extractThemeConfigForCacheKey`, `compileDocumentCss`. CSS version now
  computed by hashing VFS CSS artifact content via `computeHash`.
- `crates/wasm-quarto-hub-client/src/lib.rs` — removed `compile_document_css`,
  `compute_theme_content_hash`, `ThemeHashResponse`, `extract_frontmatter_config`,
  `json_to_config_value`. Cleaned up imports (removed `THEMES_RESOURCES`,
  `themes::ThemeSpec`, `quarto_config`).
- `crates/wasm-quarto-hub-client/Cargo.toml` — removed `quarto-config` and
  runtime `sha2` dependencies.
- `hub-client/src/types/wasm-quarto-hub-client.d.ts` — removed declarations.
- `hub-client/src/test-utils/mockWasm.ts` — removed mock functions.
- `hub-client/src/services/wasmRenderer.test.ts` — removed tests for removed functions.
- `hub-client/src/services/themeContentHash.wasm.test.ts` — deleted (tested
  removed WASM function).

Rust workspace: `cargo build --workspace` passes, `cargo nextest run --workspace`
passes (6602 tests). WASM build passes. Hub-client unit tests pass. Only the
6 WASM smoke-all theme-inheritance tests fail.

## Root Cause (Resolved)

The WASM smoke-all test (`smokeAll.wasm.test.ts`) loaded the WASM module
directly without calling `setVfsCallbacks()` on the SASS bridge. This meant
the dart-sass compiler's custom VFS importer was never initialized, so
`@use`/`@import` directives in the assembled SCSS (which reference Bootstrap
files at `/__quarto_resources__/bootstrap/scss/`) couldn't be resolved —
even though those files were present in the VFS.

The fix was one-line in nature: add `setVfsCallbacks()` in the smoke-all
test's `beforeAll()`, wiring VFS read/isFile operations to the WASM module's
`vfs_read_file` function.

The metadata merge was working correctly — themes were being extracted from
`_quarto.yml` and `_metadata.yml` files. The SCSS was being assembled
correctly (~240KB). Only the final compilation step failed silently
(falling back to `DEFAULT_CSS`).

## Work Items

- [x] Diagnose why `CompileThemeCssStage` falls back to DEFAULT_CSS on WASM
- [x] Fix the root cause
- [x] Verify all 6 theme-inheritance smoke-all WASM tests pass
- [x] Run `cargo nextest run --workspace` — 6602 passed
- [x] Run hub-client tests — 39 passed (5 test files)
- [x] Run `cargo xtask verify` — full green
- [x] Update B1 plan Phase 4 checklist
- [x] Commit all Phase 4 + B3 changes together (`60750e13`)
