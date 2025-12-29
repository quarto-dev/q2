# ConfigValue Integration into Render Pipeline

**Issue:** k-ic1o
**Date:** 2025-12-29
**Status:** Planning
**Blocks:** k-suww (matched scrolling)

## Overview

Integrate the existing `quarto-config` crate into the render pipeline to enable project-level control of rendering options. This allows WASM (hub-client) to inject settings like `format.html.source-location: full` into project configuration, which then gets merged with document metadata during rendering.

## Background

### Current State

1. **quarto-config crate** - **FULLY IMPLEMENTED**:
   - `ConfigValue`, `ConfigValueKind`, `MergeOp`, `Interpretation` - core types
   - `MergedConfig<'a>`, `MergedCursor<'a>` - lazy cursor-based merging
   - `config_value_from_yaml()` - conversion from YAML with tag parsing
   - `materialize()` - conversion to owned values
   - Tag parsing for `!prefer`, `!concat`, `!md`, `!path`, etc.
   - Comprehensive test suite

2. **ProjectConfig** (`quarto-core/src/project.rs:68-80`) - **NOT YET INTEGRATED**:
   ```rust
   pub struct ProjectConfig {
       pub project_type: ProjectType,
       pub output_dir: Option<PathBuf>,
       pub render_patterns: Vec<String>,
       pub raw: serde_json::Value,  // ← Still using raw JSON, not ConfigValue
   }
   ```

3. **render_qmd_to_html** (`quarto-core/src/pipeline.rs`):
   - Parses QMD content into Pandoc AST
   - Runs transform pipeline
   - Calls `pampa::writers::html::write()` which reads source location config from `pandoc.meta`
   - **Does not merge project config with document metadata**

4. **HTML writer** (`pampa/src/writers/html.rs`):
   - Reads `format.html.source-location` from document metadata
   - If value is `"full"`, enables source tracking with `data-loc` attributes

5. **WASM renderer** (`wasm-quarto-hub-client/src/lib.rs`):
   - Creates a minimal `ProjectContext` with no config
   - Calls `render_qmd_to_html`
   - No way to inject project-level settings

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

## Design

### Integration Tasks

The `quarto-config` crate is fully implemented. The work is **integration**:

1. **Replace `ProjectConfig.raw`** with `ConfigValue`
2. **Add conversion functions** between `MetaValueWithSourceInfo` and `ConfigValue`
3. **Add config merging** in `render_qmd_to_html` to combine project config with document metadata
4. **Add WASM API** to inject config values into `ProjectContext`

### Using MergedConfig for Project + Document

The existing `MergedConfig<'a>` is designed exactly for this use case:

```rust
// In render_qmd_to_html:
let project_config: &ConfigValue = &ctx.project.config.format_config;
let doc_config: ConfigValue = config_from_meta(&pandoc.meta);  // New conversion

let merged = MergedConfig::new(vec![project_config, &doc_config]);
// Document values override project values (later layers win)

// Read merged config for HTML writer
if merged.get_scalar(&["format", "html", "source-location"])
    .and_then(|s| s.value.as_yaml())
    .and_then(|y| y.as_str())
    == Some("full")
{
    // Enable source location tracking
}
```

### Missing Piece: MetaValueWithSourceInfo ↔ ConfigValue Conversion

We need bidirectional conversion between:
- `MetaValueWithSourceInfo` (Pandoc AST metadata)
- `ConfigValue` (quarto-config)

This allows:
1. **Project config** → merge → **effective config** (using `MergedConfig`)
2. **Document metadata** → `ConfigValue` → merge with project config
3. **Merged config** → `MetaValueWithSourceInfo` → pass to HTML writer

**Approach**: Convert merged config to `MetaValueWithSourceInfo` before calling HTML writer (less invasive, HTML writer unchanged)

### Conversion Functions

```rust
// New file: quarto-config/src/meta_convert.rs

/// Convert MetaValueWithSourceInfo to ConfigValue
pub fn config_from_meta(meta: &MetaValueWithSourceInfo) -> ConfigValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, source_info } => {
            ConfigValue::new_scalar(Yaml::String(value.clone()), source_info.clone())
        }
        MetaValueWithSourceInfo::MetaBool { value, source_info } => {
            ConfigValue::new_scalar(Yaml::Boolean(*value), source_info.clone())
        }
        MetaValueWithSourceInfo::MetaMap { entries, source_info } => {
            let map: IndexMap<String, ConfigValue> = entries
                .iter()
                .map(|e| (e.key.clone(), config_from_meta(&e.value)))
                .collect();
            ConfigValue::new_map(map, source_info.clone())
        }
        MetaValueWithSourceInfo::MetaList { items, source_info } => {
            let items: Vec<ConfigValue> = items
                .iter()
                .map(config_from_meta)
                .collect();
            ConfigValue::new_array(items, source_info.clone())
        }
        MetaValueWithSourceInfo::MetaInlines { inlines, source_info } => {
            ConfigValue::new_inlines(inlines.clone(), source_info.clone())
        }
        MetaValueWithSourceInfo::MetaBlocks { blocks, source_info } => {
            ConfigValue::new_blocks(blocks.clone(), source_info.clone())
        }
    }
}

/// Convert ConfigValue to MetaValueWithSourceInfo
pub fn meta_from_config(config: &ConfigValue) -> MetaValueWithSourceInfo {
    match &config.value {
        ConfigValueKind::Scalar(yaml) => match yaml {
            Yaml::String(s) => MetaValueWithSourceInfo::MetaString {
                value: s.clone(),
                source_info: config.source_info.clone(),
            },
            Yaml::Boolean(b) => MetaValueWithSourceInfo::MetaBool {
                value: *b,
                source_info: config.source_info.clone(),
            },
            // ... handle other Yaml variants
        },
        ConfigValueKind::Map(entries) => {
            let meta_entries: Vec<MetaMapEntry> = entries
                .iter()
                .map(|(k, v)| MetaMapEntry {
                    key: k.clone(),
                    key_source: SourceInfo::default(),
                    value: meta_from_config(v),
                })
                .collect();
            MetaValueWithSourceInfo::MetaMap {
                entries: meta_entries,
                source_info: config.source_info.clone(),
            }
        }
        ConfigValueKind::Array(items) => {
            let meta_items: Vec<MetaValueWithSourceInfo> = items
                .iter()
                .map(meta_from_config)
                .collect();
            MetaValueWithSourceInfo::MetaList {
                items: meta_items,
                source_info: config.source_info.clone(),
            }
        }
        ConfigValueKind::PandocInlines(inlines) => MetaValueWithSourceInfo::MetaInlines {
            inlines: inlines.clone(),
            source_info: config.source_info.clone(),
        },
        ConfigValueKind::PandocBlocks(blocks) => MetaValueWithSourceInfo::MetaBlocks {
            blocks: blocks.clone(),
            source_info: config.source_info.clone(),
        },
    }
}
```

### Programmatic ConfigValue Creation

For WASM to inject config without parsing YAML:

```rust
impl ConfigValue {
    /// Create a nested map structure from a path and value
    ///
    /// Example: `ConfigValue::from_path(&["format", "html", "source-location"], "full")`
    /// Creates: `{ format: { html: { source-location: "full" } } }`
    pub fn from_path(path: &[&str], value: &str) -> ConfigValue {
        if path.is_empty() {
            return ConfigValue::new_scalar(
                Yaml::String(value.to_string()),
                SourceInfo::default()
            );
        }

        let mut result = ConfigValue::new_scalar(
            Yaml::String(value.to_string()),
            SourceInfo::default()
        );

        for key in path.iter().rev() {
            let mut map = IndexMap::new();
            map.insert(key.to_string(), result);
            result = ConfigValue::new_map(map, SourceInfo::default());
        }

        result
    }
}
```

## Implementation Plan

### Phase 1: Conversion Functions

- [ ] Add `quarto-config/src/meta_convert.rs` with `config_from_meta()` and `meta_from_config()`
- [ ] Add `ConfigValue::from_path()` helper for programmatic construction
- [ ] Unit tests for conversion round-tripping
- [ ] Export from `quarto-config/src/lib.rs`

### Phase 2: ProjectConfig Integration

- [ ] Update `ProjectConfig` to use `ConfigValue` instead of `serde_json::Value`
- [ ] Update project loading code to use `config_value_from_yaml()`
- [ ] Update any code that creates `ProjectConfig` (including WASM)
- [ ] Integration tests

### Phase 3: Pipeline Config Merging

- [ ] Add `merge_config_into_pipeline()` function to `quarto-core/src/pipeline.rs`
- [ ] Modify `render_qmd_to_html` to:
  1. Convert document metadata to `ConfigValue`
  2. Create `MergedConfig` with project config + document config
  3. Convert merged config back to `MetaValueWithSourceInfo`
  4. Replace `pandoc.meta` with merged metadata
- [ ] Integration tests: verify project config merges with document metadata

### Phase 4: WASM API

- [ ] Add `render_qmd_content_with_options(content, template_bundle, options_json)` to WASM
- [ ] Parse options JSON, build `ConfigValue` using `from_path()`
- [ ] Inject into `ProjectContext.config`
- [ ] Update TypeScript bindings in `hub-client/src/services/wasmRenderer.ts`
- [ ] End-to-end test: verify `data-loc` attributes appear in output

### Phase 5: Hub-Client Integration (Part of k-suww)

- [ ] Update `renderToHtml()` to accept options parameter
- [ ] Wire through scroll sync toggle to enable source location tracking
- [ ] Verify matched scrolling works end-to-end

## File Changes Summary

### New Files
- `quarto-config/src/meta_convert.rs` - MetaValueWithSourceInfo ↔ ConfigValue conversion

### Modified Files
- `quarto-config/src/lib.rs` - Export new module
- `quarto-config/src/types.rs` - Add `ConfigValue::from_path()`
- `quarto-core/src/project.rs` - Update ProjectConfig to use ConfigValue
- `quarto-core/src/pipeline.rs` - Add config merging logic
- `wasm-quarto-hub-client/src/lib.rs` - Add options-aware render function
- `hub-client/src/services/wasmRenderer.ts` - TypeScript bindings

## Relationship to Other Issues

- **k-suww** (matched scrolling): This issue enables source location tracking needed for scroll sync
- **k-zvzm** (config merging design): This implements integration of that design
- **k-vpgx** (MergedConfig lifetime design): Already implemented in quarto-config

## Open Questions

1. **Dependency direction**: Should `quarto-config` depend on `quarto-pandoc-types` for the conversion, or should the conversion live elsewhere?
   - **Recommendation**: Put conversion in `quarto-config` since it already depends on `quarto-pandoc-types` (for `PandocInlines`/`PandocBlocks`)

2. **Error handling**: What if conversion fails (e.g., unexpected MetaValue variant)?
   - **Recommendation**: Return `Result<ConfigValue, ConfigError>` with clear error messages

3. **Performance**: Is there overhead from converting to ConfigValue and back?
   - **Mitigation**: Only convert when project config exists; optimize later if needed
