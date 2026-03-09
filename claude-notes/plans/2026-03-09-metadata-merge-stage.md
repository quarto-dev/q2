# Plan: Extract MetadataMergeStage from AstTransformsStage

## Overview

The metadata merge (project + directory + document + runtime → single flattened
config) currently lives inside `AstTransformsStage::run` (lines 155-239 of
`ast_transforms.rs`). This is architecturally wrong — metadata merge is a
configuration resolution step, not an AST transform. It also blocks future work:
any pipeline stage that needs the merged metadata (e.g., CSS compilation) must
run after AstTransformsStage, even if it doesn't need the AST transforms.

This plan extracts the merge into its own `MetadataMergeStage` that runs
immediately after `ParseDocumentStage`. This is a **pure refactor** — no
behavior change, no new dependencies, no cross-runtime concerns.

### Context: Why this matters

TS Quarto merges metadata early (`resolveFormats()`) before the render pipeline
runs. Everything downstream, including theme CSS compilation, uses the merged
result. Our pipeline merges metadata too late (inside AST transforms), which
is why theme CSS compilation can't see `_quarto.yml` settings. This extraction
is the first step toward fixing that.

## Pipeline Before and After

**Before:**
```
1. ParseDocumentStage    (LoadedSource → DocumentAst)
2. EngineExecutionStage  (DocumentAst → DocumentAst)
3. AstTransformsStage    (DocumentAst → DocumentAst)  ← merge + transforms
4. RenderHtmlBodyStage   (DocumentAst → RenderedOutput)
5. ApplyTemplateStage    (RenderedOutput → RenderedOutput)
```

**After:**
```
1. ParseDocumentStage    (LoadedSource → DocumentAst)
2. EngineExecutionStage  (DocumentAst → DocumentAst)
3. MetadataMergeStage    (DocumentAst → DocumentAst)  ← merge only
4. AstTransformsStage    (DocumentAst → DocumentAst)  ← transforms only
5. RenderHtmlBodyStage   (DocumentAst → RenderedOutput)
6. ApplyTemplateStage    (RenderedOutput → RenderedOutput)
```

Same `PipelineDataKind` types flow — `MetadataMergeStage` takes `DocumentAst`
and returns `DocumentAst` with `doc.ast.meta` replaced by the merged config.

## Files Modified

| File | Change |
|------|--------|
| `crates/quarto-core/src/stage/stages/metadata_merge.rs` | **New file** — MetadataMergeStage extracted from ast_transforms.rs |
| `crates/quarto-core/src/stage/stages/ast_transforms.rs` | Remove merge logic (lines 155-239), keep transforms only |
| `crates/quarto-core/src/stage/stages/mod.rs` | Add `mod metadata_merge` and re-export |
| `crates/quarto-core/src/stage/mod.rs` | Re-export `MetadataMergeStage` |
| `crates/quarto-core/src/pipeline.rs` | Insert `MetadataMergeStage` into all pipeline builders |

## Work Items

### Phase 1: Create MetadataMergeStage (tests first)

- [x] Create `crates/quarto-core/src/stage/stages/metadata_merge.rs` with:
  - The `json_to_config_value` helper (moved from ast_transforms.rs)
  - `MetadataMergeStage` struct implementing `PipelineStage`
  - `input_kind() → DocumentAst`, `output_kind() → DocumentAst`
  - `run()` containing the merge logic from ast_transforms.rs lines 155-239
- [x] Move the metadata merge tests from `ast_transforms.rs` to `metadata_merge.rs`:
  - `test_project_metadata_merging_basic`
  - `test_project_metadata_document_overrides_project`
  - `test_project_format_specific_settings_inherited`
  - `test_document_format_specific_overrides_project`
  - `test_non_target_format_settings_ignored`
  - `test_top_level_overridden_by_format_specific`
  - `test_runtime_metadata_applied`
  - `test_runtime_metadata_overrides_document`
  - `test_runtime_metadata_overrides_project`
  - `test_runtime_metadata_format_specific`
  - `test_runtime_metadata_none_no_change`
  - `test_runtime_metadata_without_project_config`
  - Also move the `MockRuntime`, `MockRuntimeWithMetadata`, `config_map`,
    `config_str`, `config_bool` test helpers
- [x] Update tests to create `MetadataMergeStage` instead of
  `AstTransformsStage::with_pipeline(TransformPipeline::new())`.
  The old tests used an empty transform pipeline to test only the merge —
  now the stage IS the merge, so this becomes cleaner.
- [x] Run tests — they should pass (the logic is identical, just moved)

### Phase 2: Remove merge from AstTransformsStage

- [x] Remove lines 155-239 (the merge block) from `AstTransformsStage::run`.
  The stage should now start at the `let transform_count = ...` line.
- [x] Remove the `json_to_config_value` function (moved to metadata_merge.rs)
- [x] Remove unused imports from ast_transforms.rs:
  - `quarto_config::{MergedConfig, resolve_format_config}`
  - `quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp}`
  - `quarto_source_map::SourceInfo`
  - `crate::project::directory_metadata_for_document`
- [x] Remove the metadata merge tests from ast_transforms.rs (already moved).
  Keep `test_ast_transforms_empty_pipeline` — update it to not depend on
  merge behavior (it currently passes with no project config, so it should
  still work).
- [x] Run ast_transforms tests — they should pass

### Phase 3: Wire into pipeline builders

- [x] Add `mod metadata_merge;` and `pub use metadata_merge::MetadataMergeStage;`
  to `crates/quarto-core/src/stage/stages/mod.rs`
- [x] Add `MetadataMergeStage` to re-exports in `crates/quarto-core/src/stage/mod.rs`
- [x] Update `build_html_pipeline_stages()` in `pipeline.rs` to insert
  `MetadataMergeStage` before `AstTransformsStage`:
  ```rust
  vec![
      Box::new(ParseDocumentStage::new()),
      Box::new(EngineExecutionStage::new()),
      Box::new(MetadataMergeStage::new()),  // NEW
      Box::new(AstTransformsStage::new()),
      Box::new(RenderHtmlBodyStage::new()),
      Box::new(ApplyTemplateStage::new()),
  ]
  ```
- [x] Update `build_wasm_html_pipeline()` similarly (no EngineExecutionStage):
  ```rust
  vec![
      Box::new(ParseDocumentStage::new()),
      Box::new(MetadataMergeStage::new()),  // NEW
      Box::new(AstTransformsStage::new()),
      Box::new(RenderHtmlBodyStage::new()),
      Box::new(ApplyTemplateStage::new()),
  ]
  ```
- [x] Update `parse_qmd_to_ast()` in pipeline.rs — it builds a 3-stage pipeline
  (parse + engine + ast_transforms). Add MetadataMergeStage before
  AstTransformsStage:
  ```rust
  vec![
      Box::new(ParseDocumentStage::new()),
      Box::new(EngineExecutionStage::new()),
      Box::new(MetadataMergeStage::new()),  // NEW
      Box::new(AstTransformsStage::new()),
  ]
  ```
- [x] Update the inline pipeline in `render_qmd_to_html()` (pipeline.rs lines
  361-376) — when `config.template.is_some() || !config.css_paths.is_empty()`,
  stages are built manually and bypass `build_html_pipeline_stages()`. Insert
  `MetadataMergeStage` there too:
  ```rust
  let stages: Vec<Box<dyn PipelineStage>> = vec![
      Box::new(ParseDocumentStage::new()),
      Box::new(EngineExecutionStage::new()),
      Box::new(MetadataMergeStage::new()),  // NEW
      Box::new(AstTransformsStage::new()),
      Box::new(RenderHtmlBodyStage::new()),
      Box::new(ApplyTemplateStage::with_config(apply_config)),
  ];
  ```
- [x] Update pipeline stage count assertions in tests:
  - `test_build_html_pipeline_stages`: 5 → 6 stages
  - `test_build_html_pipeline`: 5 → 6
  - `test_build_wasm_html_pipeline`: 4 → 5

### Phase 4: Full verification

- [x] `cargo nextest run -p quarto-core` — all quarto-core tests pass (644/644)
- [x] `cargo nextest run --workspace` — no regressions (6552/6552)
- [x] `cargo xtask verify` — WASM and hub-client builds work
- [x] Verify a manual render still works — verified via hub-client smoke tests
  (15 fixtures render correctly through WASM pipeline)

## Notes

- The `json_to_config_value` helper is only used by the merge logic, so it
  moves cleanly to the new module.
- The `directory_metadata_for_document` import also moves — it's only used
  during the merge.
- `AstTransformsStage` will become simpler: it just creates a `RenderContext`,
  runs the transform pipeline, and returns. No config merging.
- The mock runtimes in the test module are duplicated across several test files
  (ast_transforms, apply_template, context). This is a pre-existing issue —
  don't fix it in this change. A follow-up could extract a shared test mock.
- This plan does NOT change any behavior. The merged metadata ends up in
  `doc.ast.meta` at the same point in the pipeline. The only difference is
  that it's now done by a separate stage, which makes the pipeline's structure
  more explicit and enables future stages to be inserted between merge and
  transforms.
