# Plan: Remove Format.metadata and extract_format_metadata()

## Overview

After migrating all consumers to `doc.ast.meta` and completing the
css-in-pipeline work, `Format.metadata` (`serde_json::Value`) is populated
but never read. This plan removes it and the machinery that populates it.

This is dead code cleanup — no behavior change.

## Prerequisites (both complete)

- **Migrate consumers** (`2026-03-09-migrate-format-metadata-consumers.md`)
  — all pipeline consumers read from `doc.ast.meta`. Complete (one
  unchecked smoke test item is non-blocking).
- **CSS in pipeline** — the original plan (`2026-03-09-css-in-pipeline.md`)
  was split into sub-plans (a-core, b1-migration, b2-tests, b3-wasm-fix,
  c-tests), all of which are complete. Theme CSS is now compiled inside
  `CompileThemeCssStage`; `render_to_file.rs` reads the `css:default`
  artifact after the pipeline. The functions `extract_theme_config()` and
  `write_themed_resources()` mentioned in the original plan never existed
  on this branch — the pre-pipeline theme extraction was always done via
  `extract_format_metadata()` + `with_metadata()`, which is exactly what
  this plan removes.

## Work Items

### Phase 1: Remove extract_format_metadata() and its call sites

Remove the function, its re-export, all tests, and every call site
(including the `with_metadata()` calls that consume its output).

- [x] Remove `extract_format_metadata()` from `crates/quarto-core/src/format.rs`
- [x] Remove its re-export from `crates/quarto-core/src/lib.rs`
- [x] Remove all `extract_format_metadata` tests from `format.rs`
- [x] In `crates/quarto-core/src/render_to_file.rs`: remove the
  `extract_format_metadata` import, the `format_metadata` variable,
  the `.with_metadata(format_metadata)` call, and now-unused `input_str`
  and `warn` import. `render_format` becomes just `format_from_name(format)`.
- [x] In `crates/wasm-quarto-hub-client/src/lib.rs`: remove the
  `extract_format_metadata` import and the three
  `extract_format_metadata` + `with_metadata` pairs. Each becomes just
  `Format::html()`.
- [x] Remove `serde_yaml` dependency from `crates/quarto-core/Cargo.toml`
  (only used by `extract_format_metadata`)

### Phase 2: Remove Format.metadata field and methods

- [x] Remove `metadata: serde_json::Value` field from `Format` struct
- [x] Remove `with_metadata()` method
- [x] Remove `get_metadata()`, `get_metadata_string()`, `get_metadata_bool()` methods
- [x] Remove `use_minimal_html()` method (callers already migrated to
  `is_minimal_html()` free function)
- [x] Update `Format::html()`, `Format::pdf()`, `Format::docx()` constructors
  to no longer initialize `metadata`
- [x] Remove all `use_minimal_html`, `with_metadata`, and `get_metadata*`
  tests from `format.rs`
- [x] Update tests that assert `format.metadata == Value::Null`
  (`test_format_html`, `test_format_pdf`, `test_format_docx`) and
  `test_format_clone` — remove the metadata assertions
- [x] Fix `crates/quarto/src/commands/render.rs` `resolve_format()` which
  constructed `Format` with a `metadata` field (not in original plan)

Note: `serde_json` is used pervasively in quarto-core (15+ files) and
cannot be removed. The removable dependency is `serde_yaml`, handled in
Phase 1.

### Phase 3: Remove format_metadata() helpers

- [x] Remove `format_metadata()` from `RenderContext`
  (`crates/quarto-core/src/render.rs`)
- [x] Remove `format_metadata()` from `StageContext`
  (`crates/quarto-core/src/stage/context.rs`)
- [x] Remove tests: `test_render_context_format_metadata_null` and
  `test_render_context_format_metadata_with_value` from `render.rs`

### Phase 4: Verify

- [x] `cargo build --workspace` — compiles cleanly
- [x] `cargo nextest run --workspace` — 6603 passed, 0 failures
- [x] `cargo xtask verify` — WASM builds and hub-client tests pass
- [x] Verify `Format` struct is now a simple identifier + output config
  with no metadata baggage
