# Plan: CSS in Pipeline — Part C: Integration & E2E Tests (Phases 5-6)

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: B1-B3 complete (commit `60750e13`).

This sub-plan adds focused integration tests that verify theme CSS
compilation through the pipeline. Most end-to-end scenarios are already
covered by the B2 smoke-all fixtures; this plan fills the remaining gaps.

## What Already Exists (from B1-B3)

Before writing new tests, understand what's already tested:

### Smoke-all fixtures (`crates/quarto/tests/smoke-all/metadata/theme-inheritance/`)

6 QMD files exercised by BOTH the native Rust runner (`smoke_all.rs`) and
the WASM runner (`smokeAll.wasm.test.ts`). All use `ensureCssRegexMatches`
to verify compiled CSS contains theme-specific patterns:

| Fixture | Tests | Theme Source |
|---|---|---|
| `root-doc.qmd` | darkly `#375a7f` | `_quarto.yml` project config |
| `chapters/chapter1.qmd` | flatly `#2c3e50` | `chapters/_metadata.yml` directory metadata |
| `chapters/chapter2.qmd` | cosmo `#2780e3` | Document frontmatter `theme: cosmo` |
| `chapters/deep/deep-doc.qmd` | flatly `#2c3e50` | Inherited from `chapters/_metadata.yml` |
| `appendix/appendix-doc.qmd` | darkly `#375a7f` | Falls through to project config |
| `appendix/custom/custom-doc.qmd` | sketchy `Neucha` | `custom/_metadata.yml` |

### Unit tests in `compile_theme_css.rs`

- `test_no_theme_uses_default_css` — empty metadata → DEFAULT_CSS
- `test_builtin_theme_compiles_css` — `theme: cosmo` → compiled CSS with `.btn`
- `test_cache_hit_skips_compilation` — second compile uses cached result
- `test_invalid_theme_falls_back_to_default` — bad theme name → DEFAULT_CSS
- `test_null_theme_uses_default_css` — `theme: null` → DEFAULT_CSS
- Cache key determinism/differentiation tests

### Unit tests in `metadata_merge.rs`

- Project metadata merging, document overrides, format-specific settings
- Runtime metadata applied, overrides document, overrides project
- Format-specific runtime metadata flattened correctly
- No-project-config + runtime metadata still merges

### Other relevant tests

- `wasmRenderer.test.ts` — `toSimpleYaml` tests (only remaining after Phase 4 cleanup)
- `runtimeMetadata.wasm.test.ts` — 15 tests verifying runtime metadata merges
  correctly in WASM renders (but does NOT test theme CSS compilation specifically)

## What's NOT Tested (Gaps)

1. **Runtime metadata theme override → CSS compilation**: No test verifies
   that setting `theme: darkly` via runtime metadata produces darkly CSS.
   The individual pieces work (runtime metadata merges, theme CSS compiles)
   but the combination is untested.

2. **No-theme → DEFAULT_CSS through full pipeline**: The unit test in
   `compile_theme_css.rs` tests the stage in isolation. No smoke-all
   fixture explicitly verifies that a document with NO theme anywhere
   produces DEFAULT_CSS through the full render. (Other smoke-all tests
   without themes implicitly do this, but there's no explicit assertion.)

3. **Native integration tests through full pipeline**: The existing unit
   tests use `CompileThemeCssStage::run()` directly with constructed
   `StageContext`. No test runs `render_qmd_to_html()` or
   `render_document_to_file()` with a real `NativeRuntime` and verifies
   the CSS artifact or output file. The smoke-all tests do this, but
   they're driven by fixture files, not programmatic assertions.

## Architecture Reference

Understanding how the pieces fit together:

### Pipeline flow

**Native** (7 stages):
```
Parse → EngineExecution → MetadataMerge → CompileThemeCss → AstTransforms → RenderHtmlBody → ApplyTemplate
```

**WASM** (6 stages, no engine execution):
```
Parse → MetadataMerge → CompileThemeCss → AstTransforms → RenderHtmlBody → ApplyTemplate
```

`MetadataMergeStage` merges metadata layers (project → directory → document
→ runtime) and flattens for the target format. After this stage,
`doc.ast.meta` has a top-level `theme` key (if any layer specified one).

`CompileThemeCssStage` reads `theme` from `doc.ast.meta`, assembles SCSS
(~240KB for a Bootswatch theme), and compiles via platform-specific path:
- **Native**: `compile_scss_with_embedded()` (grass compiler, sync)
- **WASM**: `ctx.runtime.compile_sass()` → JS bridge → dart-sass

The stage stores the result as the `"css:default"` artifact. On ANY error,
it silently falls back to `DEFAULT_CSS` (logged via `trace_event!` but not
visible in WASM tests).

### Key files

- `crates/quarto-core/src/stage/stages/compile_theme_css.rs` — the stage + unit tests
- `crates/quarto-core/src/stage/stages/metadata_merge.rs` — metadata merge + unit tests
- `crates/quarto-core/src/pipeline.rs` — pipeline builders, `DEFAULT_CSS_ARTIFACT_PATH`
- `crates/quarto-core/src/render_to_file.rs` — native render entry point
- `crates/wasm-quarto-hub-client/src/lib.rs` — WASM `render_qmd()` (line ~648)
- `hub-client/src/services/smokeAll.wasm.test.ts` — WASM smoke-all runner
- `hub-client/src/wasm-js-bridge/sass.js` — dart-sass JS bridge with VFS importer
- `crates/quarto-sass/src/config.rs` — `ThemeConfig::from_config_value()`
- `crates/quarto-sass/src/compile.rs` — `assemble_theme_scss()`, `compile_theme_css()`
- `crates/quarto-core/src/resources.rs` — `DEFAULT_CSS`, `prepare_html_resources()`

### Theme detection patterns (verified in B2/B3)

Each Bootswatch theme has a unique `--bs-primary` color:
- **darkly**: `--bs-primary:.*#375a7f`
- **flatly**: `--bs-primary:.*#2c3e50`
- **cosmo**: `--bs-primary:.*#2780e3`
- **sketchy**: `Neucha` font family (most distinctive signal)
- **default** (no theme): `DEFAULT_CSS` is 4102 bytes, contains
  `/* ===== Base Styles ===== */`

### WASM SASS bridge — critical setup requirement

The dart-sass compiler in WASM uses a custom VFS importer
(`hub-client/src/wasm-js-bridge/sass.js`) that resolves `@use`/`@import`
against the VFS. This requires `setVfsCallbacks()` to be called BEFORE
any SASS compilation. The callbacks wire `vfs_read_file` and `vfs_is_file`
from the WASM module into the JS importer.

- **Production**: `wasmRenderer.ts:setupSassVfsCallbacks()` does this
  during `initWasm()`
- **Smoke-all test**: `smokeAll.wasm.test.ts:beforeAll()` does this
  (added in B3 fix, commit `60750e13`)
- **Any new WASM test that does theme CSS compilation** MUST also call
  `setVfsCallbacks()` — otherwise SASS compilation silently fails and
  falls back to DEFAULT_CSS

Setup pattern for new WASM tests:
```typescript
import { setVfsCallbacks } from '../wasm-js-bridge/sass.js';

beforeAll(async () => {
  // ... load WASM module ...

  setVfsCallbacks(
    (path: string): string | null => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path));
        return result.success && result.content !== undefined ? result.content : null;
      } catch { return null; }
    },
    (path: string): boolean => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path));
        return result.success && result.content !== undefined;
      } catch { return false; }
    },
  );
});
```

### Runtime metadata in WASM

`WasmRuntime` supports runtime metadata via `vfs_set_runtime_metadata(yaml)`.
This stores a `serde_json::Value` that `MetadataMergeStage` reads via
`ctx.runtime.runtime_metadata()`. Runtime metadata has the HIGHEST
precedence — it overrides document, directory, and project metadata.

The WASM function is `vfs_set_runtime_metadata(yaml: &str)` in `lib.rs`.
From JS: `wasm.vfs_set_runtime_metadata('theme: darkly\n')`.
(The parameter accepts YAML; existing tests use YAML syntax consistently.)

## Phase 5: Remaining Tests

### 5a: Runtime metadata theme override (WASM)

This is the primary gap. Create a new WASM test file.

- [x] **Runtime metadata overrides document theme**: Set up VFS with
  `_quarto.yml` (no theme), `doc.qmd` with `theme: flatly` in frontmatter.
  Call `wasm.vfs_set_runtime_metadata('theme: darkly\n')`.
  Render. Assert CSS contains `#375a7f` (darkly), NOT `#2c3e50` (flatly).

**Where to put it**: New file `hub-client/src/services/themeCss.wasm.test.ts`.
The existing `runtimeMetadata.wasm.test.ts` doesn't call `setVfsCallbacks()`
(it doesn't need SASS compilation), so adding theme CSS tests there would
change its character. A dedicated file keeps concerns separated and makes
the SASS bridge setup requirement explicit.

### 5b: Native integration tests (optional, lower priority)

The smoke-all tests already exercise the full native pipeline. These would
add programmatic tests that construct `RenderContext` and call
`render_qmd_to_html()` directly, which is useful for faster iteration but
not strictly necessary for coverage.

- [x] `test_render_pipeline_theme_from_project` — project config has
  `theme: darkly`, bare document. Assert `css:default` artifact contains
  `#375a7f`.
- [x] `test_render_pipeline_theme_from_document_overrides_project` — project
  has `theme: darkly`, document has `theme: flatly`. Assert `css:default`
  contains `#2c3e50`, not `#375a7f`.
- [x] `test_render_pipeline_no_theme_uses_default` — no theme anywhere.
  Assert `css:default` artifact equals `DEFAULT_CSS`.

**Where to put them**: In `crates/quarto-core/src/pipeline.rs` tests
(alongside existing full-pipeline tests like `test_render_simple_document`).
These tests use `render_qmd_to_html()` with a `RenderContext`, not
`CompileThemeCssStage::run()` directly. For project config with a theme,
use `ProjectConfig::with_metadata()` (see `metadata_merge.rs` tests).

## Phase 6: Verification

- [x] `cargo nextest run --workspace` — all 6605 tests pass
- [x] `cargo xtask verify` — WASM build + 40 hub-client tests pass
- [ ] Manual: `theme: darkly` in `_quarto.yml`, verify in hub-client
- [ ] Manual: `theme: sketchy` in frontmatter overrides project theme
- [ ] Manual: native CLI `quarto render` with theme in `_quarto.yml`

## Build & Test Commands

```bash
# Run all Rust tests
cargo nextest run --workspace

# Run only WASM smoke-all tests
cd hub-client && npx vitest run --config vitest.wasm.config.ts src/services/smokeAll.wasm.test.ts

# Run all hub-client tests
cd hub-client && npm run test:ci

# Full verification (Rust + WASM build + hub-client tests)
cargo xtask verify

# WASM rebuild (needed after changing Rust code in quarto-core or wasm-quarto-hub-client)
cd hub-client && npm run build:all
```

## Review Notes (2026-03-09)

Corrections applied after source investigation:

1. **Pipeline order fixed**: Original diagram had stages in wrong order
   (AstTransforms before CompileThemeCss, EngineExecution after
   CompileThemeCss, missing RenderHtmlBody). Corrected to match
   `pipeline.rs` lines 138-147 (native) and 196-204 (WASM).

2. **YAML not JSON**: `vfs_set_runtime_metadata` accepts YAML strings.
   Existing tests in `runtimeMetadata.wasm.test.ts` consistently use YAML
   syntax. Updated all references from `JSON.stringify(...)` to YAML.

3. **Test file location**: Phase 5a → new `themeCss.wasm.test.ts` file.
   Phase 5b → `pipeline.rs` tests (not `compile_theme_css.rs`), since
   these are full-pipeline tests using `render_qmd_to_html()`.

## Reference

See parent plan for:
- Cache key correctness and known limitations (Risk 2)
- Custom .scss file resolution in WASM (Risk 3)

See B3 plan (`2026-03-09-css-in-pipeline-b3-wasm-fix.md`) for:
- Full diagnosis of the WASM SASS bridge issue
- How `web_sys::console::log_1` can be used for Rust-side WASM debugging
  (add `web-sys` as a `cfg(target_arch = "wasm32")` dep to `quarto-core`)
