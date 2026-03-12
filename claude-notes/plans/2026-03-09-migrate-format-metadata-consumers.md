# Plan: Migrate All Consumers from Format.metadata to doc.ast.meta

## Overview

Pipeline transforms and template selection currently read metadata via
`ctx.format_metadata(key)`, which returns `&serde_json::Value` from the
`Format.metadata` field. This field is populated before the pipeline from
the document's `format.html.*` block only — it misses project, directory,
and runtime metadata layers.

After MetadataMergeStage, `doc.ast.meta` (`ConfigValue`) contains the fully
merged and format-flattened metadata from all layers. All consumers should
read from there instead.

After this plan, `Format.metadata` is still populated but never read by
pipeline code. A subsequent plan removes it entirely.

## Prerequisites

- **Default project for single-file renders** (`2026-03-09-default-project-single-file.md`)
  — ensures flattening always happens, so `doc.ast.meta` has top-level keys
  even for single-file renders.

## API Change

Consumers move from `serde_json::Value` methods to `ConfigValue` methods.
Key equivalences:

| `serde_json::Value` | `ConfigValue` |
|---------------------|---------------|
| `.as_bool()` → `Option<bool>` | `.as_bool()` → `Option<bool>` |
| `.as_str()` → `Option<&str>` | `.as_str()` → `Option<&str>` |
| `.get("key")` → `Option<&Value>` | `.get("key")` → `Option<&ConfigValue>` |
| `.as_object()` → `Option<&Map>` | **No equivalent** — use `.get("key")` directly |

The types differ (`Option<&serde_json::Value>` vs `Option<&ConfigValue>`)
but the method names are largely the same. Callers need mechanical updates.

**Important:** `ConfigValue` has no `as_object()`. Code that does
`v.as_object().and_then(|obj| obj.get("field"))` simplifies to just
`v.get("field")` since `ConfigValue::get()` handles maps directly. This
affects `create_license_section`, `create_copyright_section`, and
`create_citation_section` in Phase 4.

## Scope boundary

This plan migrates *readers* of `Format.metadata` to read from
`doc.ast.meta` instead. Functions like `render_with_format` keep their
`&Format` parameter for now — the parameter becomes unused for metadata
purposes but is not removed. The subsequent plan
(`2026-03-09-remove-format-metadata.md`) removes `Format.metadata`, the
`&Format` parameters, and all supporting machinery.

## Work Items

### Phase 1: Add `is_minimal_html` free function

`Format::use_minimal_html()` reads `minimal` and `theme` from
`Format.metadata`. Both callers (`ApplyTemplateStage`, `TitleBlockTransform`)
run after MetadataMergeStage and have access to `doc.ast.meta`.

**Tests first:**

- [x] Add `is_minimal_html(meta: &ConfigValue) -> bool` in an appropriate
  module (likely `format.rs` or `template.rs`)
- [x] Add tests:
  - `test_is_minimal_html_default` — empty meta → false
  - `test_is_minimal_html_minimal_true` — `{ minimal: true }` → true
  - `test_is_minimal_html_theme_none` — `{ theme: "none" }` → true
  - `test_is_minimal_html_theme_pandoc` — `{ theme: "pandoc" }` → true
  - `test_is_minimal_html_theme_bootstrap` — `{ theme: "cosmo" }` → false
  - `test_is_minimal_html_minimal_overrides_theme` — `{ minimal: true, theme: "cosmo" }` → true
- [x] Run tests — should PASS (new function, not migration yet)

### Phase 2: Migrate template selection

**Implement:**

- [x] Change `select_template(format: &Format)` to
  `select_template(minimal: bool)` — this is the only thing it checks.
- [x] Update `render_with_format()` to compute `is_minimal_html(meta)` and
  pass the result to `select_template(minimal)` and use it for
  `use_full_template`. The `&Format` parameter stays (removed in the next
  plan); it just stops being used for the minimal check.
- [x] `ApplyTemplateStage::run()` needs no changes — it already passes
  both `meta` and `format` to `render_with_format()`.
- [x] Update template selection tests
- [x] WASM does not call `render_with_format` or `select_template`
  directly (verified), so no WASM changes needed.

### Phase 3: Migrate TitleBlockTransform

- [x] Update `TitleBlockTransform::should_add_h1()` to call
  `is_minimal_html(&ast.meta)` instead of `ctx.format.use_minimal_html()`.
  The `AstTransform::transform()` method receives `ast: &mut Pandoc` —
  `ast.meta` is the `ConfigValue` with merged/flattened metadata.
- [x] `should_add_h1` signature changes to take `&ConfigValue` (or
  `&Pandoc`) instead of `&RenderContext`, since it only needs metadata
  and the format's `is_html()` check. The `is_html()` check still comes
  from `ctx.format`.
- [x] Update tests

### Phase 4: Migrate AppendixTransform and FootnoteTransform

These transforms read from `ctx.format_metadata(key)` which returns
`Option<&serde_json::Value>`. They need to read from `doc.ast.meta` instead.

**Transforms read these keys:**

| Key | Transform | Method |
|-----|-----------|--------|
| `appendix-style` | AppendixTransform | `get_appendix_style()` |
| `reference-location` | AppendixTransform | `get_reference_location()` |
| `book` | AppendixTransform | `is_book_format()` |
| `license` | AppendixTransform | `create_license_section()` |
| `copyright` | AppendixTransform | `create_copyright_section()` |
| `citation` | AppendixTransform | `create_citation_section()` |
| `reference-location` | FootnoteTransform | `get_reference_location()` |

**How transforms access metadata:** `AstTransform::transform()` receives
`ast: &mut Pandoc` and `ctx: &mut RenderContext`. The merged metadata is
`ast.meta` (`ConfigValue`). All helpers change from taking `&RenderContext`
to taking `&ConfigValue` (i.e., `&ast.meta`).

**`as_object()` migration:** The `create_license_section`,
`create_copyright_section`, and `create_citation_section` functions
currently use `v.as_object().and_then(|obj| obj.get("field"))`. Since
`ConfigValue::get("field")` works directly on maps, this simplifies to
`v.get("field")`. The intermediate `as_object()` call is eliminated.

**Implement:**

- [x] Change `get_appendix_style`, `get_reference_location`, and
  `is_book_format` helper methods to take `meta: &ConfigValue` instead of
  `ctx: &RenderContext`. Call `meta.get(key)` instead of
  `ctx.format_metadata(key)`.
- [x] Change `create_license_section`, `create_copyright_section`, and
  `create_citation_section` to take `meta: &ConfigValue`. Replace
  `ctx.format_metadata("license")` with `meta.get("license")`, and replace
  `.as_object().and_then(|obj| obj.get("field"))` with just `.get("field")`.
- [x] Update `FootnoteTransform::get_reference_location()` similarly —
  take `meta: &ConfigValue`, call `meta.get("reference-location")`.
- [x] Update `transform()` call sites: pass `&ast.meta` to the helpers.
- [x] Update tests for both transforms

### Phase 5: Verify

- [x] `cargo nextest run --workspace` — 6626 tests passed, 0 failures
- [x] `cargo xtask verify` — not needed (no WASM-touching changes)
- [ ] Smoke test: `appendix-style` set in `_quarto.yml` reaches
  AppendixTransform (was previously invisible because `Format.metadata`
  only saw document frontmatter)
