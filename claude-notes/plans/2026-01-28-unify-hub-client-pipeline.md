# Unify hub-client Rendering with quarto-core Pipeline

**Beads Issue**: kyoto-5hi
**Created**: 2026-01-28
**Status**: In Progress

---

## Session Progress

### Session 4 (2026-01-28) - Implemented Shared Format Metadata Extraction

**Status**: ✅ Phase A Complete

#### Changes Made

1. **Added `extract_format_metadata()` to `quarto-core/src/format.rs`**:
   - Parses YAML frontmatter and extracts `format.<format_name>` section
   - Returns `serde_json::Value::Null` if no format metadata is specified
   - Includes TODO marker for future ConfigValue migration
   - Exported from `quarto-core::lib.rs`

2. **Updated native CLI** (`crates/quarto/src/commands/render.rs`):
   - Now imports `extract_format_metadata` from `quarto_core`
   - Deleted the duplicated local function (37 lines removed)

3. **Updated WASM client** (`crates/wasm-quarto-hub-client/src/lib.rs`):
   - Now imports `extract_format_metadata` from `quarto_core`
   - Updated all three render functions:
     - `render_qmd()` - extracts metadata from VFS file content
     - `render_qmd_content()` - extracts metadata from content parameter
     - `render_qmd_content_with_options()` - extracts metadata from content parameter
   - All now use `Format::html().with_metadata(format_metadata)`

4. **Added tests** (8 new tests in `quarto-core/src/format.rs`):
   - `test_extract_format_metadata_basic` - basic toc/toc-depth extraction
   - `test_extract_format_metadata_no_frontmatter` - returns Null
   - `test_extract_format_metadata_no_format_section` - returns Null
   - `test_extract_format_metadata_different_format` - html vs pdf
   - `test_extract_format_metadata_unclosed_frontmatter` - returns Null (graceful)
   - `test_extract_format_metadata_all_toc_options` - toc-title, toc-location
   - `test_extract_format_metadata_leading_whitespace` - handles whitespace
   - `test_extract_format_metadata_empty_format_section` - empty section

#### Test Results

- ✅ All 594 quarto-core tests pass
- ✅ All 29 quarto binary tests pass
- ✅ WASM build completes successfully (`npm run build:all`)

#### Remaining Work

- [ ] Manual verification: Test TOC rendering in hub-client browser preview
- [ ] Manual verification: Test other format options (toc-depth, toc-title)

---

### Session 3 (2026-01-28) - Root Cause Found: Format Metadata Not Extracted

**Root Cause Identified**: The pipeline IS unified, but **format metadata is not being extracted from frontmatter** in the WASM renderer.

#### Evidence

Tested in hub-client with a document containing:
```yaml
---
title: "Carlos's quarto-hub experiments"
format:
  html:
    theme: cosmo
    toc: true
---
```

DOM inspection confirmed:
- `#quarto-content` exists (full template IS being used)
- `#quarto-margin-sidebar` does NOT exist (no TOC sidebar)
- No `<nav id="TOC">` element
- The template's `$if(rendered.navigation.toc)$` conditional evaluates to false

#### The Problem

**Native CLI** (`crates/quarto/src/commands/render.rs` lines 177-188):
```rust
// BEFORE calling render_qmd_to_html:
let format_metadata = extract_format_metadata(input_str, "html")?;
let format_with_metadata = Format { ..., metadata: format_metadata };
// Creates Format WITH metadata, so TocGenerateTransform sees toc: true
```

**WASM** (`crates/wasm-quarto-hub-client/src/lib.rs` lines 463, 545):
```rust
// Creates Format with EMPTY metadata
let format = Format::html();
// TocGenerateTransform checks ctx.format_metadata("toc") → returns None
// → TOC generation is skipped
```

The `TocGenerateTransform` (line 78-86 in `toc_generate.rs`) checks:
```rust
let should_generate = match ctx.format_metadata("toc") {
    Some(v) if v.as_bool() == Some(true) => true,
    Some(v) if v.as_str() == Some("auto") => true,
    _ => false,  // ← WASM hits this because format.metadata is empty
};
```

#### Prior Art

Commit `afebe4ce703bfd76e608826bb035e01f9675447f` added `extract_format_metadata()` to the native CLI, but this was NOT added to the WASM client.

---

### Session 2 (2026-01-28) - Architecture Clarification

**Key Discovery**: The pipeline is already unified. The original premise of this issue was incorrect.

#### What We Found:
1. **hub-client uses `wasm-quarto-hub-client`** (NOT `wasm-qmd-parser` as incorrectly stated in the original plan)
2. **`wasm-quarto-hub-client` already depends on `quarto-core`** (Cargo.toml line 15)
3. **The rendering already uses `render_qmd_to_html()`** (lib.rs lines 481, 564-571, 695-702)
4. **All transforms are already running** - TOC, callouts, sectionize, etc.

#### Committed Changes:
- `094c62a4` - Add pipeline builder functions for customizable stage composition

#### Status:
The pipeline builder functions from Phase 2 are now committed:
- `build_html_pipeline_stages()` - returns stages as Vec
- `build_wasm_html_pipeline()` - 4-stage pipeline without EngineExecutionStage
- `build_html_pipeline_with_stages()` - accepts custom stages

However, these may not be needed since hub-client already uses the full pipeline via `render_qmd_to_html()`.

---

## Proposed Solution: Shared Format Metadata Extraction

### Problem Statement

Currently, format metadata extraction is duplicated:
1. **Native CLI** (`quarto/src/commands/render.rs`): `extract_format_metadata()` function
2. **WASM client** (`wasm-quarto-hub-client/src/lib.rs`): `extract_frontmatter_config()` function (for themes, but NOT format metadata)

Both implement similar YAML frontmatter parsing, violating DRY.

### Design Options Considered

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| **A. Shared function in `quarto-core::format`** | Add `Format::from_qmd_content()` or standalone `extract_format_metadata()` | Minimal change, clear ownership | Still parses frontmatter separately from main parse |
| **B. New `quarto-core::frontmatter` module** | Centralize all frontmatter handling | Single source of truth | Adds new module |
| **C. Extend `pampa`** | Export frontmatter utilities from pampa | pampa is the QMD parser | Format is in quarto-core, creates dependency concern |
| **D. Pipeline-level extraction** | Extract format metadata in `ParseDocumentStage` | No duplicate parsing | Requires pipeline architecture changes |
| **E. ConfigValue migration** | Long-term: merged project+document ConfigValue | Proper solution per TODO comments | More work, not immediate fix |

### Recommended Approach: Option A (Short-term) + Option E (Long-term)

**Short-term fix**: Add a shared `extract_format_metadata()` function to `quarto-core::format` module.

**Rationale**:
- Minimal code change
- Both `quarto` binary and `wasm-quarto-hub-client` already depend on `quarto-core`
- The function clearly belongs with the `Format` type
- The existing TODO comments indicate this is interim scaffolding before ConfigValue migration

### Implementation Plan

#### Phase A: Add shared function to quarto-core (Short-term fix)

1. **Add to `quarto-core/src/format.rs`**:
   ```rust
   /// Extract format-specific metadata from QMD frontmatter.
   ///
   /// Parses the YAML frontmatter and extracts the `format.<format_name>` section.
   /// Returns `serde_json::Value::Null` if no format metadata is specified.
   ///
   /// # Arguments
   /// * `content` - The QMD source content as a string
   /// * `format_name` - The format to extract (e.g., "html", "pdf")
   ///
   /// # Example
   /// ```
   /// let metadata = extract_format_metadata(qmd_content, "html")?;
   /// let format = Format::html().with_metadata(metadata);
   /// ```
   pub fn extract_format_metadata(content: &str, format_name: &str) -> Result<serde_json::Value, String>
   ```

2. **Update native CLI** (`quarto/src/commands/render.rs`):
   - Replace local `extract_format_metadata()` with `quarto_core::format::extract_format_metadata()`
   - Delete the duplicated function

3. **Update WASM client** (`wasm-quarto-hub-client/src/lib.rs`):
   - Import `quarto_core::format::extract_format_metadata`
   - In `render_qmd()`, `render_qmd_content()`, `render_qmd_content_with_options()`:
     - Extract format metadata from content
     - Create `Format::html().with_metadata(metadata)` instead of `Format::html()`

4. **Tests**:
   - Add unit tests to `quarto-core/src/format.rs`
   - Verify hub-client TOC rendering in browser

#### Phase B: ConfigValue Migration (Long-term, separate issue)

The TODO comments throughout the codebase indicate the proper solution:
```
TODO(ConfigValue): DELETE THIS FUNCTION. Replace with merged ConfigValue from RenderContext.
```

This involves:
1. Project config (`_quarto.yml`) parsed to ConfigValue
2. Document frontmatter parsed to ConfigValue
3. Merge project + document config
4. RenderContext carries the merged ConfigValue
5. Transforms read from merged config, not Format.metadata

This is a larger refactoring tracked separately.

---

### Session 1 (2026-01-28) - Initial Analysis

- **Phase 1: Analysis** - Confirmed quarto-core's stages and transforms are already WASM-compatible
  - Engine modules (`engine/jupyter/`, `engine/knitr/`) are properly gated with `#[cfg(not(target_arch = "wasm32"))]`
  - `EngineRegistry` already handles WASM builds (only registers markdown engine)
  - Resources module has native-only functions properly gated

- **Phase 2: Parameterize Pipeline** - Added new pipeline builder functions
  - Added `build_html_pipeline_stages()` - returns stages as Vec for customization
  - Added `build_wasm_html_pipeline()` - 4-stage pipeline without EngineExecutionStage
  - Added `build_html_pipeline_with_stages()` - accepts custom stages Vec
  - Updated `lib.rs` exports
  - All 586 quarto-core tests pass

### Outstanding Question

**Is this issue still needed?** The original premise was that hub-client misses AST transforms, but:
- `wasm-quarto-hub-client` already calls `quarto_core::render_qmd_to_html()`
- This runs all transforms including TOC, callouts, sectionize
- The `EngineExecutionStage` in WASM does markdown passthrough (which is correct behavior)

Possible remaining work:
- Verify TOC actually renders correctly in hub-client preview
- If there are issues, investigate why (may be template/CSS related, not pipeline)

---

## Overview

Currently, hub-client and the native `quarto` binary use completely separate rendering paths:

- **Native `quarto`**: Uses `quarto-core::pipeline::render_qmd_to_html()` with full transform pipeline
- **hub-client (WASM)**: Uses `pampa::wasm_entry_points::render_with_template_bundle()` directly

This means hub-client misses all AST transforms including:
- TOC generation (`TocGenerateTransform`, `TocRenderTransform`)
- Callout processing (`CalloutTransform`, `CalloutResolveTransform`)
- Sectionize transform (section IDs for cross-references)
- Metadata normalization
- Title block handling
- Footnotes processing
- Future cross-reference resolution

**Goal**: Refactor the rendering architecture so hub-client can use `quarto-core`'s pipeline stages, ensuring feature parity between native and WASM rendering.

---

## Current Architecture

### Native `quarto` binary path

```
LoadedSource
    ↓ ParseDocumentStage
DocumentAst
    ↓ EngineExecutionStage (knitr/jupyter/markdown)
DocumentAst (executed)
    ↓ AstTransformsStage (runs build_transform_pipeline())
        → CalloutTransform
        → CalloutResolveTransform
        → MetadataNormalizeTransform
        → TitleBlockTransform
        → SectionizeTransform
        → FootnotesTransform
        → TocGenerateTransform      ← MISSING IN WASM
        → TocRenderTransform        ← MISSING IN WASM
        → AppendixStructureTransform
        → ResourceCollectorTransform
DocumentAst (transformed)
    ↓ RenderHtmlBodyStage
RenderedOutput (body)
    ↓ ApplyTemplateStage
RenderedOutput (complete HTML)
```

### hub-client WASM path

```
QMD bytes
    ↓ pampa::readers::qmd::read()
Pandoc AST
    ↓ pampa::template::render_with_bundle()    ← NO TRANSFORMS
Complete HTML
```

**Key files:**
- `crates/quarto-core/src/pipeline.rs` - Native pipeline
- `crates/quarto-core/src/stage/` - Pipeline stage infrastructure
- `crates/pampa/src/wasm_entry_points/mod.rs` - Current WASM entry points
- `hub-client/src/services/wasmRenderer.ts` - TypeScript WASM wrapper

---

## Analysis: WASM Compatibility of quarto-core

### Stage-by-stage analysis

| Stage | WASM Compatible? | Notes |
|-------|------------------|-------|
| `ParseDocumentStage` | ✅ Yes | Uses pampa (already in WASM) |
| `EngineExecutionStage` | ⚠️ Partial | Has WASM fallback (markdown passthrough) |
| `AstTransformsStage` | ✅ Yes | Pure AST transforms, no native deps |
| `RenderHtmlBodyStage` | ✅ Yes | Uses pampa writers |
| `ApplyTemplateStage` | ✅ Yes | Uses quarto-doctemplate |

### Transform-by-transform analysis

All transforms in `build_transform_pipeline()` are WASM-compatible:

| Transform | WASM Compatible? | Notes |
|-----------|------------------|-------|
| `CalloutTransform` | ✅ Yes | Pure AST manipulation |
| `CalloutResolveTransform` | ✅ Yes | Pure AST manipulation |
| `MetadataNormalizeTransform` | ✅ Yes | Metadata operations |
| `TitleBlockTransform` | ✅ Yes | AST manipulation |
| `SectionizeTransform` | ✅ Yes | AST manipulation |
| `FootnotesTransform` | ✅ Yes | AST manipulation |
| `TocGenerateTransform` | ✅ Yes | AST + metadata |
| `TocRenderTransform` | ✅ Yes | Metadata to HTML string |
| `AppendixStructureTransform` | ✅ Yes | AST manipulation |
| `ResourceCollectorTransform` | ✅ Yes | Metadata extraction |

### quarto-core dependencies

From `crates/quarto-core/Cargo.toml`:

```toml
# Core dependencies - all platform-agnostic
async-trait, serde, serde_json, hashlink, etc.
pampa, quarto-doctemplate, quarto-pandoc-types, etc.

# Native-only (gated with cfg)
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
tempfile, include_dir, which, regex, quarto-sass
runtimelib, jupyter-protocol, tokio (full features)
```

**Conclusion**: The core pipeline infrastructure is already designed to be platform-agnostic. Native-only dependencies are properly gated.

---

## Design: Parameterized Pipeline

### Approach

Refactor `render_qmd_to_html()` to accept a `Vec<Box<dyn PipelineStage>>` instead of building the pipeline internally. This allows:

1. Native: Full pipeline with `EngineExecutionStage`
2. WASM: Pipeline without `EngineExecutionStage` (or with markdown-only fallback)

### New API

```rust
// In quarto-core/src/pipeline.rs

/// Render QMD to HTML using a provided pipeline.
///
/// This is the core rendering function used by both native CLI and WASM.
/// The caller controls which stages are included, enabling WASM to exclude
/// stages that require native features (like EngineExecutionStage).
pub async fn render_qmd_with_pipeline(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext,
    stages: Vec<Box<dyn PipelineStage>>,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<RenderOutput>;

/// Build the standard native HTML pipeline.
///
/// Includes all stages: parse, engine execution, transforms, render, template.
pub fn build_native_html_pipeline() -> Vec<Box<dyn PipelineStage>> {
    vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(EngineExecutionStage::new()),
        Box::new(AstTransformsStage::new()),
        Box::new(RenderHtmlBodyStage::new()),
        Box::new(ApplyTemplateStage::new()),
    ]
}

/// Build the WASM HTML pipeline.
///
/// Excludes EngineExecutionStage (no code execution in browser).
/// Includes all transforms for feature parity with native.
#[cfg(target_arch = "wasm32")]
pub fn build_wasm_html_pipeline() -> Vec<Box<dyn PipelineStage>> {
    vec![
        Box::new(ParseDocumentStage::new()),
        // No EngineExecutionStage - code cells pass through as-is
        Box::new(AstTransformsStage::new()),
        Box::new(RenderHtmlBodyStage::new()),
        Box::new(ApplyTemplateStage::new()),
    ]
}

// Existing function becomes a thin wrapper
pub async fn render_qmd_to_html(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext,
    config: &HtmlRenderConfig,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<RenderOutput> {
    let stages = build_native_html_pipeline();
    render_qmd_with_pipeline(content, source_name, ctx, stages, runtime).await
}
```

### WASM Entry Point Update

```rust
// In wasm-qmd-parser or new wasm-quarto-core crate

use quarto_core::pipeline::{build_wasm_html_pipeline, render_qmd_with_pipeline};

pub fn render_qmd_to_html_wasm(content: &[u8], options: &RenderOptions) -> String {
    let stages = build_wasm_html_pipeline();
    let runtime = Arc::new(WasmRuntime::new());

    // Create minimal RenderContext for WASM
    let mut ctx = RenderContext::for_wasm(options);

    // Run the pipeline
    let result = pollster::block_on(render_qmd_with_pipeline(
        content,
        "<input>",
        &mut ctx,
        stages,
        runtime,
    ));

    // Return JSON result
    match result {
        Ok(output) => serde_json::json!({"output": output.html}).to_string(),
        Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
    }
}
```

---

## Implementation Plan

### Phase 1: Feature-gate quarto-core for WASM (P0) - ✅ ALREADY DONE

**Goal**: Make quarto-core compile for wasm32-unknown-unknown target.

**Finding**: The codebase was already well-structured for WASM compatibility:

- [x] Gate native-only imports with `#[cfg(not(target_arch = "wasm32"))]` - Already done in `engine/mod.rs`
- [x] Gate `EngineExecutionStage` native implementation - Already handled via `EngineRegistry`
- [x] Provide WASM stub for `EngineExecutionStage` (markdown passthrough only) - Registry already does this
- [x] Update `SystemRuntime` trait usage to be WASM-compatible - Trait-based abstraction already in place
- [ ] ~~Add `wasm` feature to quarto-core~~ - Not needed, cfg gates work without feature
- [ ] Test via wasm-pack (direct cargo check requires tree-sitter wasm-sysroot setup)

### Phase 2: Parameterize Pipeline (P0) - ✅ COMPLETE

**Goal**: Allow callers to specify pipeline stages.

- [x] Add `build_html_pipeline_stages()` function - Returns Vec for customization
- [x] Add `build_wasm_html_pipeline()` helper - 4-stage pipeline without engine execution
- [x] Add `build_html_pipeline_with_stages()` - Accepts custom stages Vec
- [x] Keep `build_html_pipeline()` as convenience wrapper
- [x] Update lib.rs exports
- [x] Test: all 581 quarto-core tests pass

### Phase 3: Create WASM-Compatible Render Context (P1)

**Goal**: Enable RenderContext creation in WASM without full project/document setup.

- [ ] Add `RenderContext::for_wasm()` or similar constructor
- [ ] Handle missing project context gracefully
- [ ] Handle temp directory abstraction for WASM
- [ ] Test: RenderContext can be created in WASM environment

### Phase 4: Update WASM Entry Points (P1) - ✅ ALREADY DONE

**Status**: This was already implemented before this plan was written.

- [x] `wasm-quarto-hub-client` already depends on `quarto-core`
- [x] Already uses `render_qmd_to_html()` from quarto-core
- [x] All transforms run in WASM builds

**Note**: The original plan incorrectly referenced `wasm-qmd-parser`. The correct crate is `wasm-quarto-hub-client`.

### Phase 5: Verify hub-client Features (P1)

**Goal**: Verify that transforms are actually working in hub-client preview.

- [ ] Verify TOC rendering works (may need investigation if not showing)
- [ ] Verify callout rendering works
- [ ] Verify sectionize works (section IDs)
- [ ] Test: hub-client preview shows TOC

**Note**: hub-client already uses the unified pipeline. If features aren't working,
the issue is likely in template/CSS, not the pipeline itself.

### Phase 6: Integration Testing (P2)

**Goal**: Ensure feature parity between native and WASM.

- [ ] Create test documents with TOC, callouts, cross-refs
- [ ] Render same document via native `quarto` and hub-client
- [ ] Compare HTML structure (normalize whitespace)
- [ ] Document any intentional differences

---

## Technical Considerations

### Async Runtime in WASM

- quarto-core uses `async_trait` and async stages
- In WASM, we can use `wasm-bindgen-futures` or `pollster::block_on`
- The pipeline is already designed to be async-agnostic

### StageContext in WASM

`StageContext` requires:
- `Arc<dyn SystemRuntime>` - need WasmRuntime implementation
- `Format` - can be constructed directly
- `ProjectContext` - need minimal stub for single-document mode
- `DocumentInfo` - can be constructed with virtual path

The existing `quarto_system_runtime::WasmRuntime` may need to be created or extended.

### Template Bundle

Currently hub-client loads templates via `get_builtin_template()`. The new pipeline approach should:
- Use the same template loading mechanism
- Or allow ApplyTemplateStage to accept pre-loaded templates

### CSS Artifacts

`ApplyTemplateStage` stores CSS artifacts. In WASM:
- Artifacts should be accessible to JavaScript
- hub-client's SASS compilation flow should integrate

---

## File Changes Summary

### Actually Modified (committed)

| File | Changes |
|------|---------|
| `crates/quarto-core/src/pipeline.rs` | Added pipeline builder functions |
| `crates/quarto-core/src/lib.rs` | Export new pipeline builder functions |

### Originally Planned (now known to be unnecessary)

The following changes were planned but are not needed because `wasm-quarto-hub-client`
already uses `quarto-core`:

| File | Originally Planned | Status |
|------|-------------------|--------|
| `crates/wasm-qmd-parser/*` | Add quarto-core dependency | N/A - wrong crate |
| `crates/pampa/src/wasm_entry_points/mod.rs` | Deprecate old paths | Not needed |
| `hub-client/src/services/wasmRenderer.ts` | Use new entry points | Already using quarto-core |

---

## Success Criteria

1. ✅ `cargo build --target wasm32-unknown-unknown -p quarto-core` succeeds
2. ✅ Native `quarto render` produces same output as before
3. ⏳ **hub-client preview shows Table of Contents** ← FIX IMPLEMENTED (needs browser verification)
4. ⏳ hub-client preview shows callouts correctly (likely works, not tested)
5. ✅ hub-client preview shows sectionized headings with IDs (confirmed working in DOM)
6. ⏳ No significant WASM bundle size increase (measure before/after)

### Updated Success Criteria After Fix

After implementing Phase A (shared format metadata extraction):
- [x] `extract_format_metadata()` exists in `quarto-core::format`
- [x] Native CLI uses shared function (no duplicate code)
- [x] WASM client uses shared function
- [ ] Document with `toc: true` shows TOC sidebar in hub-client (needs browser test)
- [ ] Document with `toc-depth: 2` respects depth limit (needs browser test)
- [ ] Document with `toc-title: "Custom"` shows custom title (needs browser test)
- [x] All existing quarto-core tests pass (594 tests)
- [x] All existing quarto binary tests pass (29 tests)

---

## Risks and Mitigations

### Risk: WASM bundle size increases significantly

**Mitigation**:
- Profile bundle size before and after
- Use `wasm-opt` for optimization
- Consider tree-shaking unused transforms
- Can defer some transforms to Phase 2 if size is problematic

### Risk: Async runtime incompatibility

**Mitigation**:
- Test early with simple async function in WASM
- Use `pollster::block_on` for sync wrapper
- `async_trait` already works in WASM via `wasm-bindgen-futures`

### Risk: Breaking changes to existing WASM API

**Mitigation**:
- Keep old entry points working during transition
- Deprecate with warning, don't remove immediately
- Version the WASM API

---

## References

### Key Files for Fix

| File | Role |
|------|------|
| `crates/quarto-core/src/format.rs` | Add shared `extract_format_metadata()` here |
| `crates/quarto/src/commands/render.rs` | Has duplicate `extract_format_metadata()` to remove |
| `crates/wasm-quarto-hub-client/src/lib.rs` | Needs to call shared function and pass metadata to Format |
| `crates/quarto-core/src/transforms/toc_generate.rs` | Where `ctx.format_metadata("toc")` is checked |

### Other References

- TOC implementation: `claude-notes/plans/2026-01-28-phase6-toc-rendering.md`
- Pipeline infrastructure: `crates/quarto-core/src/stage/mod.rs`
- **WASM client crate**: `crates/wasm-quarto-hub-client/src/lib.rs` (NOT wasm-qmd-parser)
- hub-client renderer: `hub-client/src/services/wasmRenderer.ts`
- Prior art commit: `afebe4ce703bfd76e608826bb035e01f9675447f` - added format metadata extraction to native CLI
