# ConfigValue Integration into Render Pipeline

**Issue:** k-ic1o
**Date:** 2025-12-29
**Status:** Implementation
**Blocks:** k-suww (matched scrolling)

## Overview

Integrate project-level configuration into the render pipeline to enable settings like `format.html.source-location: full` to be injected by WASM (hub-client), which then gets merged with document metadata during rendering.

## Background

### Current State (Post k-2tu9 Refactoring)

1. **ConfigValue is now the unified type** - The k-2tu9 refactoring unified `MetaValueWithSourceInfo` and `ConfigValue`. They are now the same type (`ConfigValue`), defined in `quarto-pandoc-types`.

2. **quarto-config crate** - **FULLY IMPLEMENTED**:
   - `ConfigValue`, `ConfigValueKind`, `MergeOp`, `Interpretation` - core types
   - `MergedConfig<'a>`, `MergedCursor<'a>` - lazy cursor-based merging
   - `config_value_from_yaml()` - conversion from YAML with tag parsing
   - Tag parsing for `!prefer`, `!concat`, `!md`, `!path`, etc.

3. **HTML writer** (`pampa/src/writers/html.rs`):
   - Already uses `ConfigValue` for metadata
   - `extract_config_from_metadata(meta: &ConfigValue)` reads source-location config
   - Looks for `format.html.source-location: full`

4. **ProjectConfig** (`quarto-core/src/project.rs:68-80`) - **NEEDS UPDATE**:
   ```rust
   pub struct ProjectConfig {
       pub project_type: ProjectType,
       pub output_dir: Option<PathBuf>,
       pub render_patterns: Vec<String>,
       pub raw: serde_json::Value,  // ← Should be Option<ConfigValue>
   }
   ```

5. **render_qmd_to_html** (`quarto-core/src/pipeline.rs`):
   - Does not merge project config with document metadata
   - Needs to accept project config and merge it

6. **WASM renderer** (`wasm-quarto-hub-client/src/lib.rs`):
   - Creates `ProjectContext` with `config: None`
   - `render_qmd_content` doesn't accept config options

### Goal

Enable this flow:
```
WASM: inject format.html.source-location into ProjectContext
         ↓
render_qmd_to_html: merge project config with document metadata
         ↓
HTML writer: reads merged config, sees source-location: full
         ↓
Output: HTML with data-loc attributes
```

## Implementation Plan

### Phase 1: Update ProjectConfig to use ConfigValue

**Files:**
- `quarto-core/src/project.rs`

**Changes:**
1. Add dependency on `quarto-config` crate
2. Change `raw: serde_json::Value` to `format_config: Option<ConfigValue>`
3. Update `parse_config()` to use `config_value_from_yaml()` instead of `serde_yaml`
4. Add helper method `ProjectConfig::with_format_config()` for programmatic creation

**Notes:**
- No conversion functions needed since types are unified
- Source location tracking comes for free with `config_value_from_yaml()`

### Phase 2: Add config merging to render pipeline

**Files:**
- `quarto-core/src/pipeline.rs`

**Changes:**
1. Import `MergedConfig` from `quarto-config`
2. In `render_qmd_to_html`:
   - If project has format_config, merge with document metadata
   - Use `MergedConfig::new(vec![&project_config, &doc_meta])`
   - Document values override project values (later layers win)
3. Pass merged config to HTML writer (already uses ConfigValue)

**Key insight:** The HTML writer's `extract_config_from_metadata()` already takes `&ConfigValue`. We need to materialize the merged config before passing it.

### Phase 3: Add WASM API for injecting config

**Files:**
- `quarto-config/src/types.rs` - Add `ConfigValue::from_path()` helper
- `wasm-quarto-hub-client/src/lib.rs` - Modify API

**Changes:**

1. Add `ConfigValue::from_path()`:
   ```rust
   /// Create a nested map from a path and value
   /// Example: from_path(&["format", "html", "source-location"], "full")
   /// Creates: { format: { html: { source-location: "full" } } }
   pub fn from_path(path: &[&str], value: &str) -> ConfigValue
   ```

2. Modify `render_qmd_content()` to accept options:
   ```rust
   #[wasm_bindgen]
   pub fn render_qmd_content(content: &str, template_bundle: &str, options: &str) -> String
   ```
   Where `options` is JSON like: `{"source_location": true}`

3. Parse options and inject into ProjectConfig:
   ```rust
   if options.source_location {
       let config = ConfigValue::from_path(
           &["format", "html", "source-location"],
           "full"
       );
       project.config = Some(ProjectConfig {
           format_config: Some(config),
           ..Default::default()
       });
   }
   ```

### Phase 4: Update TypeScript bindings

**Files:**
- `hub-client/src/services/wasmRenderer.ts`

**Changes:**
1. Update `renderToHtml()` to accept options parameter
2. Define `RenderOptions` interface:
   ```typescript
   interface RenderOptions {
     sourceLocation?: boolean;
   }
   ```
3. Pass options to WASM `render_qmd_content()`

## File Changes Summary

### Modified Files
- `quarto-core/Cargo.toml` - Add quarto-config dependency
- `quarto-core/src/project.rs` - Update ProjectConfig to use ConfigValue
- `quarto-core/src/pipeline.rs` - Add config merging logic
- `quarto-config/src/types.rs` - Add `ConfigValue::from_path()`
- `wasm-quarto-hub-client/src/lib.rs` - Add options-aware render function
- `hub-client/src/services/wasmRenderer.ts` - TypeScript bindings

## Open Questions

1. **Materialization vs lazy merging**: Should we materialize the merged config or pass it lazily?
   - **Decision**: Materialize for now since HTML writer expects `&ConfigValue`

2. **Error handling for malformed options JSON**: How to handle parse errors?
   - **Decision**: Log warning and ignore invalid options, don't fail rendering

## Relationship to Other Issues

- **k-suww** (matched scrolling): This issue enables source location tracking needed for scroll sync
- **k-2tu9** (type unification): COMPLETED - simplified this implementation significantly
- **k-zvzm** (config merging design): This implements integration of that design
