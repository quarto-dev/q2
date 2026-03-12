# Plan: Default Project for Single-File Renders

## Overview

Currently, single-file renders (no `_quarto.yml`) get `config: None` in
`ProjectContext`. This causes the metadata merge gate in `MetadataMergeStage`
to skip format flattening, leaving `doc.ast.meta` with nested `format.html.*`
keys instead of flattened top-level keys.

We fix this by making `ProjectContext.config` non-optional (`ProjectConfig`
instead of `Option<ProjectConfig>`). Every `ProjectContext` always has a
config — real projects get their parsed `_quarto.yml`, single-file renders get
`ProjectConfig::default()`. This ensures `resolve_format_config` always runs
and `doc.ast.meta` is always flattened. The type system prevents the bug from
being reintroduced.

## Background: How the bug works

### The rendering pipeline

Quarto renders documents through a staged pipeline defined in
`crates/quarto-core/src/pipeline.rs`. The key stages for this bug are:

1. `ParseDocumentStage` — parses QMD frontmatter + body into a Pandoc AST.
   Frontmatter like `format: { html: { toc: true } }` ends up as nested keys
   in `doc.ast.meta`.
2. `MetadataMergeStage` — merges project config, directory `_metadata.yml`
   layers, document frontmatter, and runtime metadata. **Crucially, this stage
   flattens format-specific keys** via `resolve_format_config()` — e.g.,
   `format.html.toc: true` becomes top-level `toc: true`.
3. `CompileThemeCssStage` — reads `doc.ast.meta["theme"]` to compile SCSS.
4. `AstTransformsStage` — runs transforms (callouts, TOC, footnotes, etc.)
   that read flattened keys from `doc.ast.meta`.

### The gate

In `MetadataMergeStage::run()` (`metadata_merge.rs:~147-149`):
```rust
let has_project_config = ctx.project.config.is_some();
if has_project_config || runtime_meta_json.is_some() {
    // ... flattening and merging happens here
}
```

When `config` is `None` (single-file render) AND there's no runtime metadata,
this entire block is skipped. The document AST passes through unflattened.
Downstream stages looking for `doc.ast.meta["theme"]` or `doc.ast.meta["toc"]`
find nothing — those values are buried under `format.html.*`.

### Format flattening

`resolve_format_config()` in `crates/quarto-config/src/format.rs:72` takes a
`ConfigValue` metadata tree and a target format (e.g., "html"). It:
1. Extracts `format.{target}.*` keys
2. Merges them over top-level keys (format-specific wins)
3. Removes the `format` key from the result

So `{ title: "Hello", format: { html: { toc: true } } }` becomes
`{ title: "Hello", toc: true }`.

### Key types and locations

- `ProjectContext` struct: `crates/quarto-core/src/project.rs:~344`
- `ProjectConfig` struct: `crates/quarto-core/src/project.rs:~261`
  (derives `Default` — `project_type: Default`, `output_dir: None`,
  `render_patterns: []`, `metadata: None`)
- `ProjectContext::discover()`: `project.rs:~368` — finds `_quarto.yml`,
  creates context. Line ~404: `is_single_file = config.is_none() && input_file.is_some()`
- `ProjectContext::single_file()`: `project.rs:~433` — creates single-file
  context directly, hardcodes `config: None`
- `MetadataMergeStage::run()`: `stage/stages/metadata_merge.rs:~115`
- `directory_metadata_for_document()`: `project.rs:~83` — uses
  `project.config.is_none()` to early-return
- `project_type()`: `project.rs:~549` — unwraps through Option
- WASM construction: `wasm-quarto-hub-client/src/lib.rs:~486` —
  `create_wasm_project_context()` hardcodes `config: None`

### Weakened test

`render_to_file.rs:~414` `test_render_to_file_with_theme` — renders a
single-file QMD with `format.html.theme: cosmo`. The assertion was weakened
from `assert!(css.contains(".btn"))` (compiled Bootstrap) to
`assert!(!css.is_empty())` (just checks DEFAULT_CSS fallback) because the
theme key is invisible due to this bug. Lines 442-447 have a NOTE comment
explaining this.

## Prerequisites

None. This is a standalone change.

## Downstream impact

This unblocks the migration of pipeline consumers from `Format.metadata` to
`doc.ast.meta` (see `2026-03-04-format-metadata-merge.md`). Without this
change, transforms reading flattened keys from `doc.ast.meta` would silently
get `None` in single-file renders.

## Work Items

### Phase 1: Tests first

Write tests against the CURRENT (`Option<ProjectConfig>`) API that demonstrate
the bug by failing. These tests will be updated in Phase 4 after the type change.

- [x] Add test in `metadata_merge.rs`: single-file render with
  `format.html.toc: true` in frontmatter and `config: None` — assert
  `doc.ast.meta.get("toc")` is `Some(true)` after merge stage runs.
  Currently fails because the merge gate skips flattening when config is None.
- [x] Add test in `project.rs`: `ProjectContext::discover()` for a path with
  no `_quarto.yml` — assert `ctx.config.is_some()` (i.e. a default config is
  present). Currently fails because `config` is `None`.
- [x] Add test in `project.rs`: `ProjectContext::single_file()` — assert
  `ctx.config.is_some()`. Currently fails for the same reason.
- [x] Run tests — all 3 FAIL as expected

### Phase 2: Make `config` non-optional — DONE

- [x] Struct definition changed
- [x] `discover()`, `single_file()`, WASM construction updated
- [x] `project_type()` accessor simplified

### Phase 3: Update all access sites — DONE

- [x] `directory_metadata_for_document()` — changed to `project.is_single_file`
- [x] `MetadataMergeStage` — removed gate, simplified config access, dedented
- [x] Verified `output_dir` resolution in `discover()` unchanged (local `Option`)

### Phase 4: Update all test constructions — DONE

- [x] All `config: None` → `ProjectConfig::default()` (35+ sites)
- [x] All `config: Some(ProjectConfig::with_metadata(...))` → unwrapped
- [x] All `config: Some(ProjectConfig { ... })` → unwrapped
- [x] Phase 1 test assertions updated for non-optional type

### Phase 5: Restore weakened test — DONE

- [x] `render_to_file.rs` assertion restored to `css.contains(".btn")`
- [x] NOTE comment removed

### Phase 6: Verify — DONE

- [x] `cargo nextest run --workspace` — 6621 tests pass, 0 failures
- [x] `cargo xtask verify --skip-hub-tests` — all steps pass (including WASM build)
- [x] Spot-check: single-file render with `format.html.toc: true` produces
  `<nav id="TOC">` in output HTML — confirmed working
