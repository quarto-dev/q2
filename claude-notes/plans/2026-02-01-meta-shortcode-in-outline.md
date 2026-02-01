# Meta Shortcode Resolution in Document Outline

## Overview

Add support for resolving `{{< meta key >}}` shortcodes when computing document outlines in `quarto lsp` and hub-client. Currently, if a header contains a meta shortcode like `# {{< meta title >}}`, the outline displays the literal shortcode text instead of the resolved value from the document's frontmatter.

## Problem Statement

Given this QMD document:

```yaml
---
title: "My Document"
author: "Alice"
---

# {{< meta title >}}

## Section by {{< meta author >}}
```

**Current behavior:** Outline shows:
- (empty or literal shortcode text)
- `Section by` (shortcode omitted)

**Desired behavior:** Outline shows:
- `My Document`
- `Section by Alice`

## Architecture Decision: The `quarto-analysis` Crate

After discussion, we've decided to create a new `quarto-analysis` crate that provides:

1. **`AnalysisContext` trait** - Shared interface for analysis operations
2. **`DocumentAnalysisContext` struct** - Lightweight implementation for LSP
3. **Analysis transforms** - AST transforms that run at "LSP speed" (no I/O, no execution)

This design:
- Separates analysis concerns from full rendering
- Allows transforms to work with either LSP context or full render context
- Provides a home for future analysis transforms (crossref resolution, etc.)
- Avoids code duplication between quarto-core and quarto-lsp-core

### Dependency Graph

```
quarto-pandoc-types
        ↑
quarto-analysis (NEW)
   ↑         ↑
quarto-core  quarto-lsp-core
```

### AnalysisContext Trait Design (Final)

```rust
/// Trait for contexts that support document analysis operations.
///
/// This trait defines the minimal interface for analysis transforms.
/// Transforms receive the Pandoc AST directly (with pandoc.meta)
/// and use this trait only for reporting diagnostics.
pub trait AnalysisContext {
    /// Report a diagnostic (warning, error, or info) during analysis.
    fn add_diagnostic(&mut self, msg: DiagnosticMessage);
}
```

**Design Decision**: The trait was simplified to a single method:
- Metadata is accessed via `pandoc.meta` directly, not through the context
- `source_context()` was removed as it was never called
- This eliminates redundancy and keeps the trait minimal
- Both `DocumentAnalysisContext` and `RenderContext` implement this trait

### Rust Pattern: Trait-based Abstraction

We use trait-based abstraction (Pattern 1 from discussion):

- `AnalysisContext` trait defines shared interface
- `DocumentAnalysisContext` (in quarto-analysis) - lightweight implementation for LSP
- `RenderContext` (in quarto-core) - full implementation, also implements `AnalysisContext`
- Transforms are written against `&mut dyn AnalysisContext` or generic over the trait

## Implementation Plan

### Phase 1: Create quarto-analysis crate structure ✅

- [x] Create `crates/quarto-analysis/` directory structure
- [x] Create `Cargo.toml` with minimal dependencies
- [x] Create `src/lib.rs` with module declarations
- [x] Add crate to workspace `Cargo.toml`

### Phase 2: Implement AnalysisContext trait and DocumentAnalysisContext ✅

- [x] Create `src/context.rs` with `AnalysisContext` trait
- [x] Implement `DocumentAnalysisContext` struct
- [x] Add builder pattern for `DocumentAnalysisContext`
- [x] Add unit tests for context

### Phase 3: Add ConfigValue::get_nested() method ✅

- [x] Add `get_nested(&self, key: &str) -> Option<&ConfigValue>` to ConfigValue
- [ ] Add unit tests for `get_nested()` in quarto-pandoc-types (TODO: add dedicated tests)

### Phase 4: Implement meta shortcode analysis transform ✅

- [x] Create `src/transforms/mod.rs` with `AnalysisTransform` trait
- [x] Create `src/transforms/shortcode.rs` with `MetaShortcodeTransform`
- [x] Implement shortcode resolution using `ConfigValue::get_nested()`
- [x] Add unit tests for the transform

### Phase 5: Integrate quarto-analysis into quarto-lsp-core ✅

- [x] Add quarto-analysis dependency to quarto-lsp-core
- [x] Update `analyze_document()` to run analysis transforms before symbol extraction
- [x] Update `get_symbols()` to run analysis transforms
- [x] Verify symbols are extracted from transformed AST
- [x] Add integration tests (`meta_shortcode_resolved_in_outline`, `meta_shortcode_missing_key_graceful`)

### Phase 6: Integrate quarto-analysis into quarto-core ✅

- [x] Add quarto-analysis dependency to quarto-core
- [x] Refactor `ShortcodeResolveTransform` to use `ConfigValue::get_nested()` (removed duplicate `get_nested_metadata()` function)
- [x] Ensure render pipeline still works correctly

**Design Refinements**:

1. **Simplified AnalysisContext**: We removed both `metadata()` and `source_context()` from `AnalysisContext`:
   - Transforms receive `&mut Pandoc` (with `.meta`) and `&mut dyn AnalysisContext`
   - `source_context()` was never called - removed to simplify the trait
   - Now `AnalysisContext` only provides `add_diagnostic()`
   - Transforms access metadata directly via `pandoc.meta`, which is the single source of truth

2. **RenderContext implements AnalysisContext**: This enables shared transform code between LSP and render pipelines

3. **warnings → diagnostics naming**: Renamed all `warnings` fields/methods to `diagnostics` throughout quarto-core and related crates to future-proof for info/error diagnostics:
   - `StageContext.warnings` → `StageContext.diagnostics`
   - `RenderContext.warnings` → `RenderContext.diagnostics`
   - `RenderOutput.warnings` → `RenderOutput.diagnostics`
   - `add_warning()` → `add_diagnostic()`

### Phase 7: Documentation and cleanup

- [ ] Add comprehensive doc comments to all public items
- [ ] Add crate-level documentation to quarto-analysis
- [ ] Update CLAUDE.md if needed
- [ ] Clean up any TODO comments

## Technical Details

### Crate Structure

```
crates/quarto-analysis/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports, crate docs
    ├── context.rs          # AnalysisContext trait + DocumentAnalysisContext
    └── transforms/
        ├── mod.rs          # AnalysisTransform trait, transform pipeline
        └── shortcode.rs    # MetaShortcodeTransform
```

### Key Type Definitions (Final)

```rust
// In context.rs
// Note: metadata is accessed via pandoc.meta, not the context
pub trait AnalysisContext {
    fn add_diagnostic(&mut self, msg: DiagnosticMessage);
}

// Lightweight implementation for LSP (no constructor params needed)
pub struct DocumentAnalysisContext {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DocumentAnalysisContext {
    pub fn new() -> Self {
        Self { diagnostics: Vec::new() }
    }
}

// In transforms/mod.rs
pub trait AnalysisTransform: Send + Sync {
    /// Name of the transform (for debugging/logging).
    fn name(&self) -> &str;

    /// Apply the transform to the AST.
    fn transform(&self, pandoc: &mut Pandoc, ctx: &mut dyn AnalysisContext) -> Result<()>;
}

/// Run a sequence of analysis transforms.
pub fn run_analysis_transforms(
    pandoc: &mut Pandoc,
    ctx: &mut dyn AnalysisContext,
    transforms: &[&dyn AnalysisTransform],
) -> Result<()>;

// In transforms/shortcode.rs
pub struct MetaShortcodeTransform;

impl AnalysisTransform for MetaShortcodeTransform { ... }
```

### ConfigValue::get_nested()

```rust
// In quarto-pandoc-types/src/config_value.rs
impl ConfigValue {
    /// Navigate nested metadata using dot notation.
    ///
    /// # Example
    /// ```
    /// // For metadata `{ author: { name: "Alice" } }`:
    /// assert!(meta.get_nested("author.name").is_some());
    /// assert!(meta.get_nested("author.email").is_none());
    /// ```
    pub fn get_nested(&self, key: &str) -> Option<&ConfigValue> {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = self;

        for part in parts {
            match &current.value {
                ConfigValueKind::Map(entries) => {
                    current = entries.iter()
                        .find(|e| e.key == part)
                        .map(|e| &e.value)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }
}
```

### Integration with quarto-lsp-core

The `analyze_document()` function:
1. Parses the document with pampa
2. Creates a `DocumentAnalysisContext` (no params needed)
3. Runs analysis transforms (which access `pandoc.meta` directly)
4. Extracts symbols from the transformed AST

```rust
pub fn analyze_document(doc: &Document) -> DocumentAnalysis {
    let source_context = doc.create_source_context();

    let result = pampa::readers::qmd::read(...);

    match result {
        Ok((mut pandoc, _ast_context, warnings)) => {
            // Create analysis context (no params - metadata accessed via pandoc.meta)
            let mut ctx = DocumentAnalysisContext::new();

            // Run analysis transforms
            let transforms: Vec<&dyn AnalysisTransform> = vec![
                &MetaShortcodeTransform,
            ];
            let _ = run_analysis_transforms(&mut pandoc, &mut ctx, &transforms);

            // Extract symbols from transformed AST
            let symbols = extract_symbols(&pandoc, &source_context, doc.content());
            // ... rest of analysis
        }
        // ...
    }
}
```

## Files to Create/Modify

### New Files
1. `crates/quarto-analysis/Cargo.toml`
2. `crates/quarto-analysis/src/lib.rs`
3. `crates/quarto-analysis/src/context.rs`
4. `crates/quarto-analysis/src/transforms/mod.rs`
5. `crates/quarto-analysis/src/transforms/shortcode.rs`

### Modified Files
1. `Cargo.toml` (workspace) - Add quarto-analysis to members
2. `crates/quarto-pandoc-types/src/config_value.rs` - Add `get_nested()` method
3. `crates/quarto-lsp-core/Cargo.toml` - Add quarto-analysis dependency
4. `crates/quarto-lsp-core/src/analysis.rs` - Run transforms before symbol extraction
5. `crates/quarto-core/Cargo.toml` - Add quarto-analysis dependency (Phase 6)
6. `crates/quarto-core/src/render/context.rs` - Implement AnalysisContext (Phase 6)

## Risk Assessment

**Low Risk:**
- New crate is additive, doesn't break existing code
- Trait-based design allows gradual adoption
- Transforms are opt-in

**Medium Risk:**
- Need to ensure transform doesn't modify AST in ways that break downstream code
- Source location tracking must be preserved through transforms

**Mitigation:**
- Comprehensive unit tests for each component
- Integration tests with real documents
- Manual testing with hub-client

## Future Extensions

This architecture supports future analysis transforms:

1. **Crossref resolution** - Resolve `@fig-name` to figure titles in outline
2. **Variable shortcodes** - `{{< var name >}}` resolution
3. **Environment shortcodes** - `{{< env VAR >}}` resolution (if appropriate for LSP)
4. **Include expansion** - Partial expansion for outline purposes (careful with I/O)
