# Plan: Unified Render Pipeline for quarto and wasm-quarto-hub-client

**Issue**: k-dnfd
**Date**: 2025-12-27
**Status**: Draft

## Problem Statement

The `wasm-quarto-hub-client` crate bypasses the `quarto-core` transform pipeline entirely, calling directly into `pampa::wasm_entry_points::parse_and_render_qmd()`. This means WASM-rendered documents are missing critical Quarto-specific features:

- **Callouts** - `.callout-note`, `.callout-warning`, etc. render as plain divs instead of structured callout HTML
- **Metadata normalization** - `pagetitle` not derived from `title`
- **Resource collection** - Image dependencies not tracked

## Goal

**One code path for both CLI and WASM.** When we improve `quarto render` functionality in the CLI, we want the collaborative writer to get the same behavior automatically. This means minimizing multiple code paths that achieve similar things.

## Current Architecture

### CLI Pipeline (`crates/quarto/src/commands/render.rs`)

```
QMD Source
    ↓
pampa::readers::qmd::read()          ← Parse to Pandoc AST
    ↓
build_transform_pipeline()           ← Creates TransformPipeline
    ↓
TransformPipeline::execute():
  1. CalloutTransform                ← Div.callout-* → CustomNode("Callout")
  2. CalloutResolveTransform         ← CustomNode → structured Div for HTML
  3. MetadataNormalizeTransform      ← Add derived metadata (pagetitle, etc.)
  4. ResourceCollectorTransform      ← Collect image dependencies
    ↓
pampa::writers::html::write()        ← Render AST to HTML body
    ↓
quarto_core::template::render_with_resources()  ← Wrap with template
    ↓
Write to disk
```

### WASM Pipeline (`crates/wasm-quarto-hub-client/src/lib.rs`)

```
QMD Source (from VFS)
    ↓
pampa::wasm_entry_points::parse_and_render_qmd()
    ├─ qmd_to_pandoc()               ← Parse to Pandoc AST
    └─ render_with_template_bundle() ← Directly to template (NO TRANSFORMS!)
    ↓
HTML String (returned to JavaScript)
```

## Key Findings

### 1. RenderContext Works in WASM

Despite initial concerns, `RenderContext` has no native-only dependencies:

```rust
pub struct RenderContext<'a> {
    pub artifacts: ArtifactStore,        // In-memory HashMap
    pub project: &'a ProjectContext,     // Just paths and config
    pub document: &'a DocumentInfo,      // Just input/output paths
    pub format: &'a Format,              // Output format metadata
    pub binaries: &'a BinaryDependencies, // Can be empty for WASM
    pub options: RenderOptions,          // Boolean flags
}
```

For WASM, we simply construct minimal versions:
- `project` - Single-file project pointing to VFS path
- `document` - Source path in VFS
- `format` - `Format::html()`
- `binaries` - Empty (no external binaries in browser)

### 2. Existing Abstractions Already Support This

- **`quarto-system-runtime`** provides `SystemRuntime` trait with both `NativeRuntime` and `WasmRuntime`
- **`wasm-quarto-hub-client`** already depends on `quarto-core` (Cargo.toml line 14) but doesn't use it
- **`quarto_core::template`** functions work without filesystem access

### 3. The Gap is Simply Missing Code

The WASM client doesn't use `quarto-core` transforms - it's not that they can't work, it's that they're not called. The fix is to:
1. Extract the pipeline into a shared function
2. Have both CLI and WASM call that function

## Proposed Solution: Single Unified Pipeline

The key insight is that `RenderContext` and its dependencies don't require native-only features:

```rust
pub struct RenderContext<'a> {
    pub artifacts: ArtifactStore,        // In-memory HashMap - works in WASM
    pub project: &'a ProjectContext,     // Just paths and config - works in WASM
    pub document: &'a DocumentInfo,      // Just input/output paths - works in WASM
    pub format: &'a Format,              // Just output format data - works in WASM
    pub binaries: &'a BinaryDependencies, // Can be empty for WASM
    pub options: RenderOptions,          // Just boolean flags - works in WASM
}
```

The `SystemRuntime` abstraction already handles filesystem differences via `NativeRuntime` and `WasmRuntime`.

### Architecture

Create a **single render function** in `quarto-core` that both CLI and WASM use:

```rust
// crates/quarto-core/src/pipeline.rs

/// Unified render pipeline - used by both CLI and WASM
pub fn render_to_html(
    content: &[u8],
    source_path: &Path,
    ctx: &mut RenderContext,
) -> Result<RenderOutput> {
    // 1. Parse QMD to AST
    let (mut pandoc, ast_context) = parse_qmd(content, source_path)?;

    // 2. Run transform pipeline (same for CLI and WASM)
    let pipeline = build_transform_pipeline();
    pipeline.execute(&mut pandoc, ctx)?;

    // 3. Render body HTML
    let body = render_body(&pandoc, &ast_context)?;

    // 4. Apply template
    let html = template::render_with_resources(&body, &pandoc.meta, &ctx.css_paths())?;

    Ok(RenderOutput { html, artifacts: ctx.artifacts.clone() })
}

fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));
    pipeline
}
```

### CLI Usage

```rust
// crates/quarto/src/commands/render.rs

fn render_document(...) -> Result<()> {
    let content = runtime.file_read(&doc_info.input)?;

    // Build context (as before)
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &binaries);

    // Use unified pipeline
    let output = quarto_core::pipeline::render_to_html(&content, &doc_info.input, &mut ctx)?;

    // Write output
    runtime.file_write(&output_path, output.html.as_bytes())?;
}
```

### WASM Usage

```rust
// crates/wasm-quarto-hub-client/src/lib.rs

pub fn render_qmd_content(content: &str, template_bundle: &str) -> String {
    // Build minimal context for WASM
    let project = ProjectContext::single_file(Path::new("/project/input.qmd"));
    let doc = DocumentInfo::from_path("/project/input.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new(); // empty - no binaries in WASM

    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    // Use THE SAME unified pipeline
    match quarto_core::pipeline::render_to_html(content.as_bytes(), Path::new("/project/input.qmd"), &mut ctx) {
        Ok(output) => format_success_response(&output.html),
        Err(e) => format_error_response(e),
    }
}
```

### Benefits

1. **Single code path** - When CLI improves, WASM improves automatically
2. **Same transforms** - Callouts, metadata, etc. work identically
3. **Testable** - One pipeline to test, not two
4. **Maintainable** - No divergence over time

## Implementation Steps

### Step 1: Add Helper Constructors to ProjectContext
- [ ] Add `ProjectContext::single_file(path: &Path)` constructor for simple single-file projects
- [ ] This makes it easy for WASM to construct minimal project contexts

### Step 2: Create Pipeline Module in quarto-core
- [ ] Create `crates/quarto-core/src/pipeline.rs`
- [ ] Implement `RenderOutput` struct (html + artifacts)
- [ ] Implement `render_to_html()` function that:
  - Parses QMD (using pampa)
  - Runs transform pipeline
  - Renders body HTML
  - Applies template
- [ ] Implement `build_transform_pipeline()` (extracted from render.rs)
- [ ] Export from `lib.rs`
- [ ] Add tests

### Step 3: Refactor CLI to Use Pipeline
- [ ] Update `quarto/src/commands/render.rs` to use `quarto_core::pipeline::render_to_html()`
- [ ] Remove duplicated pipeline construction code
- [ ] Verify CLI still works correctly
- [ ] Run existing tests

### Step 4: Update wasm-quarto-hub-client
- [ ] Update `render_qmd()` to use `quarto_core::pipeline::render_to_html()`
- [ ] Update `render_qmd_content()` to use the same pipeline
- [ ] Construct appropriate `RenderContext` for WASM environment
- [ ] Handle template bundles appropriately

### Step 5: Verify Feature Parity
- [ ] Create test document with callouts
- [ ] Render with CLI and capture output
- [ ] Render with WASM and capture output
- [ ] Compare outputs - should be identical (modulo resource paths)
- [ ] Document any intentional differences

## Design Decisions

### Q: Why not create a simpler API that avoids RenderContext?
**A**: The goal is **one code path**, not two similar ones. When we improve the CLI, we want WASM to get the same improvements automatically. Separate APIs would diverge over time.

### Q: Does RenderContext work in WASM?
**A**: Yes. Looking at its fields:
- `ArtifactStore` - In-memory HashMap, works in WASM
- `ProjectContext` - Just paths and config data
- `DocumentInfo` - Just input/output path info
- `Format` - Just output format metadata
- `BinaryDependencies` - Empty for WASM (no external binaries)
- `RenderOptions` - Just boolean flags

None require native-only features. The `SystemRuntime` abstraction handles filesystem differences.

### Q: Should ResourceCollectorTransform work in WASM?
**A**: Yes, it should run. It collects image paths into the `ArtifactStore`. In WASM:
- The paths are in the VFS, so resolution works
- The artifacts can be returned to JS if needed (e.g., for fetching images)
- Even if not used immediately, running it ensures identical behavior

### Q: Where should the unified function live?
**A**: In `quarto-core`, not in `quarto` crate. The `quarto` crate is the CLI binary. The `quarto-core` crate is the library that both CLI and WASM can use.

### Q: What about template bundles in WASM?
**A**: The WASM client currently accepts a `template_bundle` parameter. We need to ensure the unified pipeline can accept either:
- A built-in template (for both CLI and WASM)
- A custom template bundle (primarily for WASM)

This may require adding template configuration to `RenderContext` or as a parameter to `render_to_html()`.

## Files to Modify

1. **New file**: `crates/quarto-core/src/pipeline.rs` - Unified pipeline module
2. **Modify**: `crates/quarto-core/src/lib.rs` - Export pipeline module
3. **Modify**: `crates/quarto-core/src/project.rs` - Add `ProjectContext::single_file()` helper
4. **Modify**: `crates/quarto/src/commands/render.rs` - Refactor to use unified pipeline
5. **Modify**: `crates/wasm-quarto-hub-client/src/lib.rs` - Use unified pipeline

## Success Criteria

1. WASM-rendered documents have properly structured callout HTML
2. WASM-rendered documents have `pagetitle` in metadata
3. CLI rendering continues to work identically
4. No regression in CLI performance
5. WASM bundle size increase is minimal

## Open Questions

1. **Template configuration**: Should template be part of `RenderContext`, a parameter to `render_to_html()`, or configured via `Format`?
2. **CSS paths**: The CLI writes CSS files to disk and references them. WASM may want inline CSS or different paths. How do we handle this?
3. **ASTContext**: Should we expose `ASTContext` from pampa in the output, or hide it as an implementation detail?

## References

- Issue: k-dnfd
- Related code:
  - `crates/quarto/src/commands/render.rs:176-259` - Current CLI pipeline
  - `crates/wasm-quarto-hub-client/src/lib.rs:209-244` - Current WASM pipeline
  - `crates/quarto-core/src/transforms/` - Transform implementations
  - `crates/quarto-core/src/template.rs` - Template rendering
