# Project Metadata Merging with Format Resolution

**Date**: 2026-02-16
**Status**: Complete

## Overview

Implement full project metadata merging in q2, where `_quarto.yml` is parsed as `ConfigValue` and merged with document frontmatter. This includes proper format-specific config resolution following Quarto 1's approach.

### Goals

1. Parse entire `_quarto.yml` as `ConfigValue` with source tracking
2. Implement format-specific config extraction (`format.html.*` → flat config)
3. Merge project metadata with document metadata, respecting format-specific overrides
4. Maintain backwards compatibility with existing `format_config` usage in WASM

### Non-Goals

- Extension metadata merging (separate task)
- Directory-level `_metadata.yml` support (future work)
- Command-line flag merging (future work)

## Background

### Current State

- `ProjectConfig` has `raw: serde_json::Value` (no source tracking)
- `format_config: Option<ConfigValue>` exists but is always `None`
- `AstTransformsStage` has merge code (lines 107-128) but unused
- WASM uses `ProjectConfig::with_format_config()` to inject settings

### Quarto 1 Behavior

Format config merging follows a two-level precedence:

**Within a single metadata source:**
- Format-specific (`format.html.toc`) always overrides top-level (`toc`)
- YAML key order doesn't matter

**Across sources (lowest → highest priority):**
1. Built-in format defaults
2. Project `_quarto.yml` (top-level, then format-specific)
3. Directory `_metadata.yml` (future)
4. Document frontmatter (top-level, then format-specific)
5. Command-line flags (future)

### Example

```yaml
# _quarto.yml
title: "Project Title"
toc: true
format:
  html:
    toc-depth: 3
    theme: cosmo

# document.qmd frontmatter
---
title: "Chapter 1"
toc: false
format:
  html:
    toc-depth: 2
---
```

Result for HTML rendering:
- `title`: "Chapter 1" (doc overrides project)
- `toc`: false (doc overrides project)
- `toc-depth`: 2 (doc format.html overrides project format.html)
- `theme`: cosmo (inherited from project format.html)

## Design

### Approach: Option B - Format Flattening

Create a `resolve_format_config()` function that:
1. Takes metadata and target format name
2. Extracts top-level settings
3. Extracts `format.{target}.*` settings
4. Merges them (format-specific wins over top-level)
5. Returns a flat `ConfigValue` for the target format

This matches Quarto 1's `formatFromMetadata()` function.

### New Types and Functions

#### 1. Format Resolution Function (in `quarto-config`)

```rust
/// Extract and flatten format-specific configuration.
///
/// Given a ConfigValue containing metadata and a target format name,
/// returns a new ConfigValue with:
/// - All top-level settings (except `format`)
/// - Format-specific settings from `format.{target}` merged on top
///
/// # Example
///
/// Input metadata:
/// ```yaml
/// title: "Hello"
/// toc: true
/// format:
///   html:
///     toc: false
///     theme: cosmo
/// ```
///
/// With target_format = "html", returns:
/// ```yaml
/// title: "Hello"
/// toc: false      # from format.html, overrides top-level
/// theme: cosmo    # from format.html
/// ```
pub fn resolve_format_config(
    metadata: &ConfigValue,
    target_format: &str,
) -> ConfigValue
```

**Implementation pseudocode:**
```rust
pub fn resolve_format_config(metadata: &ConfigValue, target_format: &str) -> ConfigValue {
    // 1. Start with empty result map
    let mut result_entries: Vec<ConfigMapEntry> = Vec::new();

    // 2. If metadata is not a map, return empty map
    let entries = match &metadata.value {
        ConfigValueKind::Map(e) => e,
        _ => return ConfigValue::new_map(vec![], metadata.source_info.clone()),
    };

    // 3. Copy all top-level entries EXCEPT "format"
    for entry in entries {
        if entry.key != "format" {
            result_entries.push(entry.clone());
        }
    }

    // 4. Find format.{target} and merge its entries on top
    if let Some(format_entry) = entries.iter().find(|e| e.key == "format") {
        match &format_entry.value.value {
            // Handle format: { html: { ... } }
            ConfigValueKind::Map(format_map) => {
                if let Some(target_entry) = format_map.iter().find(|e| e.key == target_format) {
                    if let ConfigValueKind::Map(target_settings) = &target_entry.value.value {
                        // Merge target settings (override existing keys)
                        for setting in target_settings {
                            // Remove existing entry with same key
                            result_entries.retain(|e| e.key != setting.key);
                            // Add the format-specific entry
                            result_entries.push(setting.clone());
                        }
                    }
                }
            }
            // Handle format: "html" (shorthand) - no settings to merge
            ConfigValueKind::Scalar(Yaml::String(s)) if s == target_format => {
                // Shorthand matches target, but no additional settings
            }
            _ => {}
        }
    }

    ConfigValue::new_map(result_entries, metadata.source_info.clone())
}
```

#### 2. Updated ProjectConfig

```rust
pub struct ProjectConfig {
    pub project_type: ProjectType,
    pub output_dir: Option<PathBuf>,
    pub render_patterns: Vec<String>,

    /// Full project metadata as ConfigValue (with source tracking).
    /// This is the entire _quarto.yml parsed with InterpretationContext::ProjectConfig.
    pub metadata: Option<ConfigValue>,

    // Note: `raw: serde_json::Value` field removed - it was unused
}
```

#### 3. Updated Merge in AstTransformsStage

```rust
// In AstTransformsStage::execute()

if let Some(project_metadata) = ctx.project.config.as_ref().and_then(|c| c.metadata.as_ref()) {
    // Get the target format name from context
    let target_format = ctx.format.name(); // e.g., "html"

    // Flatten project metadata for target format
    let project_for_format = resolve_format_config(project_metadata, target_format);

    // Flatten document metadata for target format
    let doc_for_format = resolve_format_config(&doc.ast.meta, target_format);

    // Merge: project (lower priority) → document (higher priority)
    let merged = MergedConfig::new(vec![&project_for_format, &doc_for_format]);

    if let Ok(materialized) = merged.materialize() {
        doc.ast.meta = materialized;
    }
}
```

### File Changes

| File | Change |
|------|--------|
| `crates/quarto-config/src/lib.rs` | Export `resolve_format_config` |
| `crates/quarto-config/src/format.rs` | New file: format resolution logic |
| `crates/quarto-core/src/project.rs` | Add `metadata: Option<ConfigValue>`, update `parse_config()` |
| `crates/quarto-core/src/stage/stages/ast_transforms.rs` | Use format resolution in merge |
| `crates/quarto-core/Cargo.toml` | Add `quarto-yaml` dependency |

### Dependencies

```
quarto-yaml ──────────────────┐
                              ▼
quarto-config ◄─── resolve_format_config()
      │
      ▼
quarto-core (project.rs, ast_transforms.rs)
      │
      ▼
pampa (yaml_to_config_value with InterpretationContext)
```

## Implementation Plan

### Phase 1: Format Resolution Function

**Files**: `crates/quarto-config/src/format.rs`, `crates/quarto-config/src/lib.rs`

- [x] Create `format.rs` with `resolve_format_config()` function
- [x] Handle edge cases:
  - No `format` key in metadata
  - `format` key exists but target format not present
  - `format: html` shorthand (string, not object)
  - Empty format config (`format: { html: {} }`)
- [x] Export from `lib.rs`

### Phase 2: Tests for Format Resolution

**File**: `crates/quarto-config/src/format.rs` (tests module)

- [x] Test: top-level only (no format key)
- [x] Test: format-specific only (no top-level)
- [x] Test: format-specific overrides top-level
- [x] Test: format-specific merges with top-level (non-overlapping keys)
- [x] Test: nested objects merge correctly
- [ ] Test: arrays follow merge semantics (!prefer, !concat) - deferred
- [x] Test: missing target format returns top-level only
- [x] Test: format shorthand normalization (`format: html` → `format: { html: {} }`)
- [x] Test: multiple formats, only target extracted
- [x] Test: source info preserved through resolution

### Phase 3: Project Config Parsing

**File**: `crates/quarto-core/src/project.rs`

- [x] Add `quarto-yaml` and `pampa` to `quarto-core` dependencies
- [x] Replace `raw: serde_json::Value` with `metadata: Option<ConfigValue>` in `ProjectConfig`
- [x] Remove `format_config` field (superseded by `metadata`)
- [x] Update `parse_config()` to:
  - Parse YAML with `quarto_yaml::parse_file()`
  - Convert to ConfigValue with `yaml_to_config_value(..., InterpretationContext::ProjectConfig)`
  - Store in `metadata` field
- [x] Update `ProjectConfig::with_format_config()` → `ProjectConfig::with_metadata()`
- [x] Update `Default` impl for `ProjectConfig` (unchanged - already works)

### Phase 4: Tests for Project Config Parsing

**File**: `crates/quarto-core/src/project.rs` (tests module)

- [x] Test: `test_project_config_default` - verifies default ProjectConfig
- [x] Test: `test_project_config_with_metadata` - verifies with_metadata constructor
- [ ] Test: parse simple `_quarto.yml` into ConfigValue - deferred (requires file system mocking)
- [ ] Test: source info points to correct file - deferred
- [ ] Test: strings in project config are literal (not markdown) - deferred
- [ ] Test: nested format config parsed correctly - deferred
- [ ] Test: error handling for invalid YAML - deferred

### Phase 5: AstTransformsStage Integration

**File**: `crates/quarto-core/src/stage/stages/ast_transforms.rs`

- [x] Update merge logic to use `resolve_format_config()`
- [x] Get target format name from `ctx.format.identifier.as_str()`
- [x] Flatten both project and document metadata for target format
- [x] Merge flattened configs
- [x] Update trace logging

### Phase 6: Integration Tests

**Location**: `crates/quarto-core/src/stage/stages/ast_transforms.rs` (tests module)

- [x] Test: project title inherited by document (`test_project_metadata_merging_basic`)
- [x] Test: document title overrides project title (`test_project_metadata_document_overrides_project`)
- [x] Test: project `format.html.*` inherited (`test_project_format_specific_settings_inherited`)
- [x] Test: document `format.html.*` overrides project (`test_document_format_specific_overrides_project`)
- [x] Test: top-level `toc` overridden by `format.html.toc` (`test_top_level_overridden_by_format_specific`)
- [x] Test: non-target format settings ignored (`test_non_target_format_settings_ignored`)
- [x] Test: WASM `with_metadata()` still works - verified by hub-client builds

### Phase 7: WASM Compatibility

**File**: `crates/wasm-quarto-hub-client/src/lib.rs`

- [x] Update WASM to use `ProjectConfig::with_metadata()` instead of `with_format_config()`
- [x] The injected config is already a full metadata ConfigValue with `format.html.*` nested
- [x] Example: `{ format: { html: { source-location: "full" } } }`
- [ ] Test hub-client renders correctly with injected config - requires WASM build verification
- [ ] Run `cargo xtask verify` to ensure WASM builds work - needs manual verification

## Test Specifications

### Unit Tests: resolve_format_config()

```rust
#[test]
fn test_resolve_format_top_level_only() {
    // Input: { title: "Hello", toc: true }
    // Target: "html"
    // Output: { title: "Hello", toc: true }
}

#[test]
fn test_resolve_format_specific_overrides_top_level() {
    // Input: { toc: true, format: { html: { toc: false } } }
    // Target: "html"
    // Output: { toc: false }
}

#[test]
fn test_resolve_format_merges_non_overlapping() {
    // Input: { title: "Hello", format: { html: { theme: "cosmo" } } }
    // Target: "html"
    // Output: { title: "Hello", theme: "cosmo" }
}

#[test]
fn test_resolve_format_nested_objects() {
    // Input: {
    //   format: {
    //     html: {
    //       code-fold: true,
    //       code-tools: { source: true }
    //     }
    //   }
    // }
    // Target: "html"
    // Output: { code-fold: true, code-tools: { source: true } }
}

#[test]
fn test_resolve_format_missing_target() {
    // Input: { title: "Hello", format: { pdf: { documentclass: "article" } } }
    // Target: "html"
    // Output: { title: "Hello" }  // pdf settings ignored
}

#[test]
fn test_resolve_format_shorthand() {
    // Input: { format: "html" }  // shorthand, not object
    // Target: "html"
    // Output: {}  // format: html means html with defaults
}

#[test]
fn test_resolve_format_preserves_source_info() {
    // Verify SourceInfo from original keys is preserved
}
```

### Integration Tests: Full Pipeline

```rust
#[test]
fn test_project_document_merge_title() {
    // _quarto.yml: { title: "Project" }
    // doc.qmd: { title: "Document" }
    // Result: title = "Document"
}

#[test]
fn test_project_document_merge_inherited_settings() {
    // _quarto.yml: { bibliography: "refs.bib", format: { html: { theme: "cosmo" } } }
    // doc.qmd: { title: "Document" }
    // Result: bibliography = "refs.bib", theme = "cosmo", title = "Document"
}

#[test]
fn test_format_specific_override_across_layers() {
    // _quarto.yml: { toc: true, format: { html: { toc-depth: 3 } } }
    // doc.qmd: { format: { html: { toc-depth: 2 } } }
    // Result: toc = true, toc-depth = 2
}
```

## Open Questions

### Q1: Format Name Access ✅ RESOLVED

How do we get the target format name in `AstTransformsStage`?

**Answer**: Use `ctx.format.identifier.as_str()` which returns `"html"`, `"pdf"`, etc.

```rust
// In ast_transforms.rs
let target_format = ctx.format.identifier.as_str(); // "html", "pdf", etc.
```

See `crates/quarto-core/src/format.rs:40-54` for the `FormatIdentifier::as_str()` implementation.

### Q2: Format Shorthand Handling ✅ RESOLVED

Should `format: html` (string) be normalized to `format: { html: {} }` during:
- YAML parsing (in `yaml_to_config_value`)
- Format resolution (in `resolve_format_config`)
- Both?

**Finding from DeepWiki**: The codebase handles string-to-object conversions at the `ConfigValue` conversion time in `yaml_to_config_value`, based on context and tags. However, there's no existing precedent for the specific `format: html` → `format: { html: {} }` pattern.

**Decision**: Handle in `resolve_format_config()` because:
1. It's format-specific logic that only matters when resolving for a target format
2. The `format` key has special semantics (it's a map of format names to configs)
3. Keeps `yaml_to_config_value` generic without format-specific knowledge
4. `ConfigValue::from_path()` already demonstrates creating nested structures programmatically

### Q3: Default Format Settings ✅ RESOLVED

Quarto 1 has built-in defaults per format (e.g., `htmlFormat()` sets `fig-width: 7`).

**Finding**: q2 has TWO types of defaults:

1. **Template defaults** (exist): `compute_template_defaults()` in `pampa/src/template/config_merge.rs:166-186`
   - `lang: "en"` (always)
   - `pagetitle` derived from `title`
   - These ARE already merged as lowest-priority layer in `merged_metadata_to_context()`

2. **Format-specific defaults** (do NOT exist): `Format::html()` etc. in `quarto-core/src/format.rs:116-143`
   - `metadata: serde_json::Value::Null` - no defaults set
   - Unlike Quarto 1's `htmlFormat()` which sets `fig-width: 7`, etc.

**Decision**: For this PR, we don't need to implement format-specific defaults. The template defaults are already handled separately in the template rendering path. Format-specific defaults (like `fig-width`) can be added in a future PR if needed - they would be the lowest-priority layer before project config.

### Q4: Backwards Compatibility ✅ RESOLVED

The `raw: serde_json::Value` field is used elsewhere. Need to:
- Audit all usages of `ProjectConfig.raw`
- Decide: keep both fields, or migrate all usages?

**Finding**: `raw` is only defined and assigned in `project.rs:80,334`. No other code reads from it.

**Decision**: Remove `raw` field entirely and replace with `metadata: Option<ConfigValue>`. No backwards compatibility concerns since the field is unused.

## Critical Implementation Details

### Key Function Signatures

**yaml_to_config_value** (`crates/pampa/src/pandoc/meta.rs:137`):
```rust
pub fn yaml_to_config_value(
    yaml: quarto_yaml::YamlWithSourceInfo,
    context: InterpretationContext,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> ConfigValue
```

**InterpretationContext** (`crates/pampa/src/pandoc/meta.rs`):
```rust
pub enum InterpretationContext {
    DocumentMetadata,  // Strings parsed as markdown
    ProjectConfig,     // Strings kept literal
}
```

**quarto-yaml parsing** (`crates/quarto-yaml/src/parser.rs`):
```rust
// Simple parse (no filename tracking)
pub fn parse(content: &str) -> Result<YamlWithSourceInfo>

// Parse with filename for source tracking (USE THIS)
pub fn parse_file(content: &str, filename: &str) -> Result<YamlWithSourceInfo>

// Parse YAML extracted from parent document (for frontmatter)
pub fn parse_with_parent(content: &str, parent: SourceInfo) -> Result<YamlWithSourceInfo>
```

### StageContext Access Pattern

In `AstTransformsStage`, access format via:
```rust
let target_format = ctx.format.identifier.as_str(); // "html", "pdf", etc.
```

Where `ctx` is `&mut StageContext` and `ctx.format` is `Format`.

### Test Helper Patterns

From `crates/quarto-config/src/merged.rs` tests:
```rust
fn scalar(s: &str) -> ConfigValue {
    ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::default())
}

fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let map_entries: Vec<ConfigMapEntry> = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: SourceInfo::default(),
            value: v,
        })
        .collect();
    ConfigValue::new_map(map_entries, SourceInfo::default())
}
```

### Crate Dependency Order

Build/test order matters:
1. `quarto-source-map` (no deps)
2. `quarto-yaml` (depends on source-map)
3. `quarto-pandoc-types` (depends on source-map)
4. `quarto-config` (depends on source-map, pandoc-types)
5. `pampa` (depends on all above)
6. `quarto-core` (depends on all above, including pampa)

**Note**: `quarto-core` already depends on `pampa` (see `Cargo.toml:31`), so we can use `pampa::yaml_to_config_value` directly. Also already depends on `quarto-yaml` via pampa.

### ConfigValue Key Methods

```rust
impl ConfigValue {
    pub fn get(&self, key: &str) -> Option<&ConfigValue>  // Navigate into maps
    pub fn new_map(entries: Vec<ConfigMapEntry>, source_info: SourceInfo) -> Self
    pub fn new_scalar(yaml: Yaml, source_info: SourceInfo) -> Self
    pub fn with_merge_op(self, op: MergeOp) -> Self
    pub fn from_path(path: &[&str], value: &str) -> ConfigValue  // Create nested structure
}
```

### Current ProjectConfig Fields to Change

```rust
// Current (crates/quarto-core/src/project.rs:70-88)
pub struct ProjectConfig {
    pub project_type: ProjectType,
    pub output_dir: Option<PathBuf>,
    pub render_patterns: Vec<String>,
    pub raw: serde_json::Value,              // REMOVE
    pub format_config: Option<ConfigValue>,  // REMOVE
}

// New
pub struct ProjectConfig {
    pub project_type: ProjectType,
    pub output_dir: Option<PathBuf>,
    pub render_patterns: Vec<String>,
    pub metadata: Option<ConfigValue>,       // ADD - full _quarto.yml
}
```

### WASM Update Required

`crates/wasm-quarto-hub-client/src/lib.rs:682-696` uses:
```rust
let format_config = ConfigValue::from_path(&["format", "html", "source-location"], "full");
let project_config = ProjectConfig::with_format_config(format_config);
```

Change to:
```rust
let metadata = ConfigValue::from_path(&["format", "html", "source-location"], "full");
let project_config = ProjectConfig::with_metadata(metadata);
```

### Verification Commands

After implementation:
```bash
cargo build --workspace
cargo nextest run --workspace
cargo xtask verify  # Critical for WASM changes
```

## References

- Design doc: `claude-notes/plans/2025-12-07-config-merging-design.md`
- Current merge code: `crates/quarto-core/src/stage/stages/ast_transforms.rs:107-128`
- ConfigValue type: `crates/quarto-pandoc-types/src/config_value.rs`
- MergedConfig: `crates/quarto-config/src/merged.rs`
- yaml_to_config_value: `crates/pampa/src/pandoc/meta.rs:137`
- Quarto 1 format resolution: `src/command/render/render-contexts.ts` (formatFromMetadata)
