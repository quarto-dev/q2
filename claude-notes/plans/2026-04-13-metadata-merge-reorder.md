---
title: 'Reorder MetadataMergeStage Before EngineExecutionStage'
date: 2026-04-13
---

## Overview

The current HTML pipeline has `EngineExecutionStage` (index 1) running before `MetadataMergeStage` (index 2). This is architecturally backwards -- engine execution reads metadata to determine which engine to use (`detect_engine(&doc_ast.ast.meta)`), but the project/directory metadata layers haven't been merged yet at that point.

This works today only because engine detection uses document-level YAML front matter, which is available after parsing. But project-level `engine: jupyter` in `_quarto.yml` would silently fail.

## The Change

In `crates/quarto-core/src/pipeline.rs`, `build_html_pipeline_stages()`:

**Before:**
```rust
vec![
    Box::new(ParseDocumentStage::new()),
    Box::new(EngineExecutionStage::new()),   // index 1
    Box::new(MetadataMergeStage::new()),     // index 2
    ...
]
```

**After:**
```rust
vec![
    Box::new(ParseDocumentStage::new()),
    Box::new(MetadataMergeStage::new()),     // index 1 (moved up)
    Box::new(EngineExecutionStage::new()),   // index 2 (moved down)
    ...
]
```

## Type Compatibility

Both stages have the same type signature: `DocumentAst -> DocumentAst`. The swap is type-safe at the pipeline level.

## Risk Assessment

Low risk. MetadataMergeStage adds more metadata on top of what the document already has. Engine detection that worked with document-only metadata will still work with the full merged metadata present. No metadata is removed or transformed in a breaking way.

## Work Items

- [x] Swap the two stages in `build_html_pipeline_stages()` in `crates/quarto-core/src/pipeline.rs`
- [x] Check if the same ordering issue exists in `build_wasm_html_pipeline_stages()` or any other pipeline construction function, and fix those too
- [x] Run `cargo nextest run --workspace` to verify no regressions
- [x] Run `cargo xtask verify` to ensure WASM builds still work
- [x] Update any tests that assert on stage ordering or stage index values

## Parent Issue

This is a prerequisite for pipeline tracing (`bd-3c6e`), which needs merged metadata to be available early so that `trace: true` in `_quarto.yml` is visible before most pipeline stages run.
