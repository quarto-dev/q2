# Directory Metadata (_metadata.yml) Support

**Date**: 2026-02-17
**Status**: Core Implementation Complete (Path Resolution Deferred)
**Depends on**: `2026-02-16-project-metadata-merging.md` (Complete)

## Overview

Implement support for `_metadata.yml` files, which provide directory-level configuration that applies to all documents within that directory and its subdirectories. This fills in the missing layer between project config (`_quarto.yml`) and document frontmatter.

### Goals

1. Discover `_metadata.yml` files in the directory hierarchy from project root to document
2. Parse each file as `ConfigValue` with source tracking
3. Fail render on invalid YAML (syntax errors)
4. Merge directory metadata into the layering system between project and document
5. Convert relative paths in `_metadata.yml` to be document-relative

### Non-Goals

- WASM/VFS support (deferred - hub-client uses `with_metadata()` injection)
- Caching of directory metadata (can optimize later if needed)
- Command-line flag merging (separate future work)
- **Schema validation** - q2 doesn't have front-matter schemas ported yet (TS Quarto builds these programmatically from 60+ YAML files). For now we validate YAML syntax only. Schema validation can be added when schemas are available.

## Background

### Full Metadata Layering (TS Quarto)

From lowest to highest priority:
1. Built-in format defaults
2. Project `_quarto.yml` (top-level, then format-specific)
3. **Directory `_metadata.yml` files** (root → leaf, deeper wins) ← THIS PR
4. Document frontmatter (top-level, then format-specific)
5. Command-line flags

### TS Quarto Implementation

From `src/project/project-shared.ts`:

```typescript
async function directoryMetadataForInputFile(project, inputDir) {
  // Walk from project root to input directory
  const relativePath = relative(projectDir, inputDir);
  const dirs = relativePath.split(SEP_PATTERN);

  let config = {};
  let currentDir = projectDir;

  for (const dir of dirs) {
    currentDir = join(currentDir, dir);
    const file = metadataFile(currentDir); // _metadata.yml or _metadata.yaml
    if (file) {
      // Read, validate, normalize format, convert paths
      const yaml = await readAndValidateYamlFromFile(file, frontMatterSchema, errMsg);
      if (yaml.format) {
        yaml.format = normalizeFormatYaml(yaml.format);
      }
      config = mergeConfigs(config, toInputRelativePaths(..., yaml));
    }
  }
  return config;
}
```

Key behaviors:
- Walks directories root → leaf
- Looks for `_metadata.yml` or `_metadata.yaml`
- Validates against front-matter schema (fails render on error)
- Normalizes `format` key
- Converts paths to be relative to input document
- Deeper directories override (via `mergeConfigs`)

### Current q2 State

After `2026-02-16-project-metadata-merging.md`:
- `resolve_format_config()` flattens format-specific settings (in `quarto-config/src/format.rs`)
- `ProjectConfig.metadata` holds parsed `_quarto.yml` as `Option<ConfigValue>`
- `AstTransformsStage` merges: project → document (see `ast_transforms.rs:~150-200`)
- Schema validation infrastructure exists in `quarto-yaml-validation` (but no schemas ported)

**Important**: The merge happens in `AstTransformsStage::execute()`. The current flow is:
1. Get project metadata from `ctx.project.config.metadata`
2. Flatten with `resolve_format_config(&project_meta, target_format)`
3. Flatten document meta with `resolve_format_config(&doc.ast.meta, target_format)`
4. Merge with `MergedConfig::new(vec![&project_layer, &doc_layer])`
5. Materialize and assign to `doc.ast.meta`

This PR adds directory metadata layers between steps 2 and 3.

## Design

### Approach

Add a new function `directory_metadata_for_document()` that:
1. Takes project context and document path
2. Walks directory hierarchy from project root to document's directory
3. Finds and parses `_metadata.yml` files
4. Validates each against front-matter schema
5. Returns a list of `ConfigValue` layers (ordered root → leaf)

Then update `AstTransformsStage` to merge all layers:
```
project → dir_metadata[0] → dir_metadata[1] → ... → document
```

### New Types and Functions

#### 1. Directory Metadata Discovery

**File**: `crates/quarto-core/src/project.rs`

```rust
/// Find and parse all _metadata.yml files between project root and document directory.
///
/// Walks the directory hierarchy from project root to the document's parent directory,
/// looking for `_metadata.yml` or `_metadata.yaml` files. Each found file is parsed
/// and validated against the front-matter schema.
///
/// # Arguments
///
/// * `project` - The project context (provides project root directory)
/// * `document_path` - Path to the document being rendered
///
/// # Returns
///
/// A vector of `ConfigValue` layers, ordered from project root to document directory.
/// Each layer contains the parsed and validated metadata from that directory's
/// `_metadata.yml` file. Directories without `_metadata.yml` are skipped.
///
/// # Errors
///
/// Returns an error if:
/// - A `_metadata.yml` file contains invalid YAML
/// - A `_metadata.yml` file fails schema validation
/// - File I/O errors occur
///
/// # Example
///
/// Given project structure:
/// ```text
/// project/
///   _quarto.yml
///   _metadata.yml          # Layer 0: { theme: "cosmo" }
///   chapters/
///     _metadata.yml        # Layer 1: { toc: true }
///     intro/
///       _metadata.yml      # Layer 2: { toc-depth: 2 }
///       chapter1.qmd       # Document being rendered
/// ```
///
/// Returns: [layer0, layer1, layer2] - deeper directories later in vec
pub fn directory_metadata_for_document(
    project: &ProjectContext,
    document_path: &Path,
) -> Result<Vec<ConfigValue>>
```

#### 2. Path Resolution Helper

**File**: `crates/quarto-core/src/project.rs`

```rust
/// Convert relative paths in metadata to be relative to the target document.
///
/// Walks the ConfigValue tree and adjusts any `ConfigValueKind::Path` values.
/// When a `_metadata.yml` in `chapters/` has `template: !path templates/custom.tex`,
/// and we're rendering `chapters/intro/doc.qmd`, the path is adjusted to
/// `../templates/custom.tex` so it resolves correctly from the document's location.
///
/// # Arguments
///
/// * `metadata` - The ConfigValue containing paths to convert
/// * `metadata_dir` - Directory where the _metadata.yml file is located
/// * `document_dir` - Directory where the document being rendered is located
///
/// # Returns
///
/// A new ConfigValue with Path variants adjusted to be relative to document_dir.
/// Non-path values (Scalar strings, etc.) are unchanged.
///
/// # Note
///
/// Only `ConfigValueKind::Path` values (from `!path` tags) are converted.
/// Plain strings are not treated as paths. Users must use `!path` tags for
/// paths that need resolution:
///
/// ```yaml
/// template: !path templates/custom.tex  # Will be converted
/// title: "My Title"                      # Not converted (plain string)
/// ```
pub fn convert_paths_to_document_relative(
    metadata: ConfigValue,
    metadata_dir: &Path,
    document_dir: &Path,
) -> ConfigValue
```

#### 3. Updated Merge in AstTransformsStage

**File**: `crates/quarto-core/src/stage/stages/ast_transforms.rs`

```rust
// In AstTransformsStage::execute()

// Get target format
let target_format = ctx.format.identifier.as_str();

// Layer 1: Project metadata (flattened for format)
let project_layer = ctx.project.config
    .as_ref()
    .and_then(|c| c.metadata.as_ref())
    .map(|m| resolve_format_config(m, target_format));

// Layer 2: Directory metadata (multiple layers, each flattened)
let dir_layers: Vec<ConfigValue> = directory_metadata_for_document(&ctx.project, &ctx.document.path)?
    .into_iter()
    .map(|m| resolve_format_config(&m, target_format))
    .collect();

// Layer 3: Document metadata (flattened for format)
let doc_layer = resolve_format_config(&doc.ast.meta, target_format);

// Build merge layers: project → dir[0] → dir[1] → ... → document
let mut layers: Vec<&ConfigValue> = Vec::new();
if let Some(ref proj) = project_layer {
    layers.push(proj);
}
for dir_meta in &dir_layers {
    layers.push(dir_meta);
}
layers.push(&doc_layer);

// Merge all layers
let merged = MergedConfig::new(layers);
if let Ok(materialized) = merged.materialize() {
    doc.ast.meta = materialized;
}
```

### File Changes

| File | Change |
|------|--------|
| `crates/quarto-core/src/project.rs` | Add `directory_metadata_for_document()` |
| `crates/quarto-core/src/project.rs` | Add `convert_paths_to_document_relative()` |
| `crates/quarto-core/src/stage/stages/ast_transforms.rs` | Update merge to include directory layers |

### Error Handling

For now, we only validate YAML syntax (not schema). Parse errors from `quarto_yaml::parse_file()` will be caught and reported with file path context:

```rust
fn parse_metadata_file(path: &Path) -> Result<ConfigValue> {
    let content = fs::read_to_string(path)?;
    let filename = path.file_name().unwrap_or_default().to_string_lossy();

    let yaml = quarto_yaml::parse_file(&content, &filename)
        .map_err(|e| anyhow!(
            "Directory metadata validation failed for {}: {}",
            path.display(),
            e
        ))?;

    // Convert to ConfigValue
    let mut diagnostics = DiagnosticCollector::new();
    Ok(yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics))
}
```

**Future**: When front-matter schemas are ported to q2 (see investigation in `claude-notes/`), we can add schema validation using `quarto-yaml-validation`.

### Path Resolution Approach

**Key insight**: q2 has a better approach than TS Quarto.

**TS Quarto** (implicit): Walks entire metadata tree, probes filesystem with `existsSync()` for every string, converts if file exists.

**q2** (explicit): Uses `ConfigValueKind::Path` variant. Paths are explicitly marked via:
- `!path` YAML tag: `template: !path templates/custom.tex`
- `!glob` YAML tag: `resources: !glob images/*.png`

This means:
1. **No filesystem probing** required
2. **No list of path keys** to maintain
3. Walk the `ConfigValue` tree and adjust any `ConfigValueKind::Path` values

**User requirement**: To get path resolution in `_metadata.yml`, users must use `!path` tags:
```yaml
# _metadata.yml
template: !path templates/custom.tex
bibliography: !path refs.bib
css: !path styles/custom.css
```

**Future enhancement**: Schema-driven interpretation could automatically treat `schema: path` fields as paths without explicit tags. This would require parsing to be schema-aware.

## Implementation Plan

### Phase 1: Directory Metadata Discovery ✅ COMPLETE

**File**: `crates/quarto-core/src/project.rs`

- [x] Add `find_metadata_file()` helper (looks for `_metadata.yml` or `_metadata.yaml`)
- [x] Add `directory_metadata_for_document()` function
- [x] Walk directory hierarchy from project root to document directory
- [x] Parse each `_metadata.yml` with `quarto_yaml::parse_file()`
- [x] Convert to ConfigValue with `InterpretationContext::ProjectConfig`
- [x] Return vector of layers (root → leaf order)

### Phase 2: Error Handling ✅ COMPLETE

**File**: `crates/quarto-core/src/project.rs`

- [x] Handle YAML parse errors with descriptive messages
- [x] Include file path and source location in error messages
- [x] Fail render with "Directory metadata validation failed for {file}" message
- [ ] (Future) Add schema validation when front-matter schemas are ported

### Phase 3: Path Resolution - DEFERRED

**File**: `crates/quarto-core/src/project.rs`

Path resolution is deferred to a separate PR for the following reasons:

1. **Separate concern**: Path resolution is orthogonal to directory metadata discovery. The core functionality (finding and merging `_metadata.yml` files) works without it.

2. **Different design**: q2 uses explicit `!path` tags instead of TS Quarto's implicit filesystem probing. This requires:
   - Walking `ConfigValueKind::Path` variants only (not all strings)
   - No filesystem existence checks needed
   - Cleaner, more predictable behavior

3. **Testing complexity**: Path resolution tests need careful setup with actual file structures to verify relative path calculations work correctly.

4. **Future schema integration**: When front-matter schemas are ported, schema-driven path interpretation could automatically treat `schema: path` fields as paths without explicit tags.

**See**: `claude-notes/plans/2026-02-17-dir-metadata-path-resolution.md` for implementation plan.

- [ ] Add `convert_paths_to_document_relative()` function (separate PR)
- [ ] Walk ConfigValue tree recursively (separate PR)
- [ ] For `ConfigValueKind::Path` values, compute relative path adjustment (separate PR)
- [ ] Handle both single paths and arrays containing paths (separate PR)
- [ ] Preserve `ConfigValueKind::Glob` unchanged (separate PR)
- [ ] Apply path conversion after parsing, before returning layers (separate PR)

### Phase 4: Merge Integration ✅ COMPLETE

**File**: `crates/quarto-core/src/stage/stages/ast_transforms.rs`

- [x] Update `execute()` to call `directory_metadata_for_document()`
- [x] Apply `resolve_format_config()` to each directory layer
- [x] Build complete layer list: project → dirs → document
- [x] Update `MergedConfig::new()` call with all layers
- [x] Update tracing/logging to show directory layers

### Phase 5: Unit Tests ✅ COMPLETE

**File**: `crates/quarto-core/src/project.rs` (tests module)

- [x] Test: no `_metadata.yml` files returns empty vec
- [x] Test: single `_metadata.yml` in document's directory
- [x] Test: multiple `_metadata.yml` files in hierarchy
- [x] Test: `_metadata.yaml` alternate extension works
- [x] Test: invalid YAML syntax fails with descriptive error
- [x] Test: document at project root returns empty vec
- [x] Test: single-file project returns empty vec
- [ ] Test: `!path` tagged values become `ConfigValueKind::Path` (deferred with path resolution)
- [ ] Test: path conversion adjusts `Path` variants to document-relative (deferred)
- [ ] Test: plain strings (not `!path` tagged) are NOT converted (deferred)

### Phase 6: Integration Tests (smoke-all) ✅ COMPLETE

**Location**: `crates/quarto/tests/smoke-all/metadata/`

- [x] Test: directory metadata inherited by document (`dir-metadata/`)
- [x] Test: deeper directory overrides shallower (`dir-metadata-hierarchy/`)
- [x] Test: document overrides directory metadata (`dir-metadata-override/`)
- [ ] Test: format-specific settings in `_metadata.yml` work (TODO)
- [ ] Test: relative paths in `_metadata.yml` resolve correctly (deferred with path resolution)

### Phase 7: Documentation

- [ ] Update plan status to Complete
- [ ] Document new behavior in relevant module docs

## Running Tests

**Unit tests** (fast, run frequently):
```bash
cargo nextest run --workspace
# Or for specific crate:
cargo nextest run -p quarto-core
```

**Smoke-all tests** (slower, integration tests):
```bash
cargo nextest run -p quarto smoke_all
# Run specific test:
cargo nextest run -p quarto smoke_all::metadata
```

**Full verification** (before committing):
```bash
cargo xtask verify
```

**IMPORTANT**: Do NOT pipe `cargo nextest run` through `tail` or other commands - it causes hangs. Run it directly.

## Test Specifications

### Unit Tests

```rust
#[test]
fn test_directory_metadata_empty() {
    // Project with no _metadata.yml files
    // Returns: empty vec
}

#[test]
fn test_directory_metadata_single_file() {
    // project/
    //   chapters/
    //     _metadata.yml  { toc: true }
    //     doc.qmd
    // Returns: [{ toc: true }]
}

#[test]
fn test_directory_metadata_hierarchy() {
    // project/
    //   _metadata.yml     { theme: "cosmo" }
    //   chapters/
    //     _metadata.yml   { toc: true }
    //     intro/
    //       _metadata.yml { toc-depth: 2 }
    //       doc.qmd
    // Returns: [{ theme }, { toc }, { toc-depth }] in order
}

#[test]
fn test_directory_metadata_skips_missing() {
    // project/
    //   _metadata.yml     { theme: "cosmo" }
    //   chapters/
    //     intro/          # No _metadata.yml here
    //       _metadata.yml { toc: true }
    //       doc.qmd
    // Returns: [{ theme }, { toc }] - skips chapters/
}

#[test]
fn test_directory_metadata_invalid_yaml_fails() {
    // _metadata.yml with YAML syntax error
    // Returns: Err with "Directory metadata validation failed for..."
    // Note: Only validates syntax, not schema (schemas not yet ported)
}

#[test]
fn test_path_conversion() {
    // _metadata.yml in chapters/: { template: !path "templates/custom.tex" }
    // Document in chapters/intro/doc.qmd
    // Result: template path adjusted to "../templates/custom.tex"
    // Note: Only ConfigValueKind::Path values are converted, not plain strings
}
```

### Smoke-all Tests

**How smoke-all tests work**: The test runner in `crates/quarto/tests/smoke_all.rs` finds all `.qmd` files in `smoke-all/`, renders them, and checks assertions in the `_quarto.tests` frontmatter block.

**Test directory structure**:
```
crates/quarto/tests/smoke-all/metadata/dir-metadata/
├── _quarto.yml                    # Project config (required)
├── _metadata.yml                  # Root directory metadata
├── chapters/
│   ├── _metadata.yml              # Chapter-level metadata
│   └── chapter1.qmd               # Document with test assertions
```

**Example test files**:

```yaml
# tests/smoke-all/metadata/dir-metadata/_quarto.yml
project:
  type: default
title: "Dir Metadata Project"

# tests/smoke-all/metadata/dir-metadata/_metadata.yml
# This should be inherited by all docs in this project
author: "Project Author"
toc: false

# tests/smoke-all/metadata/dir-metadata/chapters/_metadata.yml
# This overrides the root _metadata.yml for docs in chapters/
toc: true
toc-depth: 2

# tests/smoke-all/metadata/dir-metadata/chapters/chapter1.qmd
---
title: Chapter 1
_quarto:
  tests:
    html:
      ensureHtmlElements:
        - ["nav#TOC", "TOC should be present (from chapters/_metadata.yml toc: true)"]
      ensureFileRegexMatches:
        - ["Project Author", "Author should be inherited from root _metadata.yml"]
---

## Introduction

Content here with a second heading.

## Another Section

More content.
```

**Test for path resolution** (if implementing):
```yaml
# tests/smoke-all/metadata/dir-metadata-paths/_quarto.yml
project:
  type: default

# tests/smoke-all/metadata/dir-metadata-paths/_metadata.yml
template: !path templates/custom.html

# tests/smoke-all/metadata/dir-metadata-paths/templates/custom.html
# (actual template file)

# tests/smoke-all/metadata/dir-metadata-paths/chapters/doc.qmd
# The template path should be resolved to ../templates/custom.html
```

## Open Questions

### Q1: Schema Availability ✅ RESOLVED (DEFERRED)

The front-matter schema is NOT yet ported to q2. TS Quarto builds it programmatically from 60+ YAML files in `src/resources/schema/`. The `quarto-yaml-validation` crate has the validation infrastructure, but no schemas.

**Decision**: Skip schema validation for now. Validate YAML syntax only. Add schema validation when schemas are ported (separate future work).

### Q2: Path Key List ✅ RESOLVED

**Finding**: q2 uses explicit `ConfigValueKind::Path` variant instead of key-based detection.

**TS Quarto approach** (implicit):
- Walks entire tree, probes filesystem for every string
- Expensive and implicit

**q2 approach** (explicit):
- Users mark paths with `!path` tag: `template: !path custom.tex`
- Creates `ConfigValueKind::Path` variant
- Walk tree and adjust only `Path` variants

**Decision**: Use q2's explicit approach. Only `ConfigValueKind::Path` values are converted.
No list of path keys needed. Users must use `!path` tags for paths requiring resolution.

**Future**: Schema-driven interpretation could auto-detect `schema: path` fields.

### Q3: Single-File Mode

When rendering a standalone document (no project), should we:
- Look for `_metadata.yml` in parent directories up to some limit?
- Only look in the document's own directory?
- Skip directory metadata entirely?

TS Quarto requires a project context for `directoryMetadataForInputFile()`.

**Proposed**: Skip directory metadata for single-file mode (no project). This matches TS behavior.

## Dependencies

```
quarto-yaml ──────────────────────┐
quarto-yaml-validation ───────────┤
                                  ▼
quarto-core (project.rs) ◄─── directory_metadata_for_document()
      │
      ▼
quarto-core (ast_transforms.rs) ◄─── merge all layers
```

## Key Code Locations

### Existing Implementation to Study

**Project metadata merging** (just completed, use as template):
- `crates/quarto-core/src/stage/stages/ast_transforms.rs` - Look at lines ~107-200 for the existing project → document merge
- `crates/quarto-config/src/format.rs` - `resolve_format_config()` flattens `format.{target}.*` to top-level

**YAML parsing to ConfigValue**:
- `crates/pampa/src/pandoc/meta.rs` - `yaml_to_config_value()` function
- Uses `InterpretationContext::ProjectConfig` for `_quarto.yml` and `_metadata.yml` (strings stay literal)
- Uses `InterpretationContext::DocumentMetadata` for frontmatter (strings parsed as markdown)

**Project context**:
- `crates/quarto-core/src/project.rs` - `ProjectContext` struct has `dir: PathBuf` (project root)
- `ProjectConfig` struct has `metadata: Option<ConfigValue>` (the parsed `_quarto.yml`)
- **Look at `find_project_config()`** (~line 263) - shows pattern for checking `.yml` and `.yaml` extensions
- **Look at `parse_config()`** (~line 301) - shows how to parse YAML to ConfigValue with `yaml_to_config_value`

**SystemRuntime abstraction**:
- `quarto_system_runtime::SystemRuntime` trait abstracts file I/O
- Used for `runtime.path_exists()`, `runtime.read_to_string()`, etc.
- This allows WASM and native to share code with different I/O implementations
- For `directory_metadata_for_document()`, you can either:
  - Take `runtime: &dyn SystemRuntime` parameter (consistent with existing code)
  - Use `std::fs` directly if this function is only called in native context

**ConfigValue types**:
- `crates/quarto-pandoc-types/src/config_value.rs` - `ConfigValue`, `ConfigValueKind`, `ConfigMapEntry`
- `ConfigValueKind::Path(String)` - created by `!path` YAML tag
- `ConfigValueKind::Glob(String)` - created by `!glob` YAML tag
- `ConfigValueKind::Map(Vec<ConfigMapEntry>)` - for nested structures

**Merging**:
- `crates/quarto-config/src/merged.rs` - `MergedConfig::new(layers)` and `merged.materialize()`
- Layers are ordered lowest-to-highest priority (first = lowest)

### Existing Smoke Tests (use as examples)

Recently added metadata tests in `crates/quarto/tests/smoke-all/metadata/`:
- `project-inherits/` - document inherits from `_quarto.yml`
- `doc-overrides/` - document overrides project settings
- `format-specific/` - tests `format.html.toc` layering

These use the `_quarto.tests` frontmatter pattern for assertions:
```yaml
_quarto:
  tests:
    html:
      ensureHtmlElements:
        - ["nav#TOC", "TOC should be present"]
      ensureFileRegexMatches:
        - ["some regex pattern"]
```

### Imports You'll Need

```rust
// In project.rs
use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{anyhow, Result};
use quarto_yaml;
use pampa::yaml_to_config_value;
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_config::{ConfigValue, ConfigValueKind, InterpretationContext};

// For path manipulation (choose one approach):
// Option A: Add pathdiff to Cargo.toml
use pathdiff::diff_paths;

// Option B: Manual implementation (no new dependency)
// See "Path Relativity Calculation" section below
```

**Note**: `yaml_to_config_value` takes a `DiagnosticCollector` for collecting warnings. Create with `DiagnosticCollector::new()`. Check `diagnostics.has_errors()` after parsing.

**Note**: `pathdiff` is NOT currently in the workspace. Either add it to `Cargo.toml` or implement the relative path calculation manually (see example below).

### ConfigValue Tree Walking Pattern

To walk a ConfigValue tree and transform Path values:

```rust
fn transform_paths(value: ConfigValue, transform: impl Fn(&str) -> String) -> ConfigValue {
    match value.value {
        ConfigValueKind::Path(p) => ConfigValue {
            value: ConfigValueKind::Path(transform(&p)),
            ..value
        },
        ConfigValueKind::Map(entries) => {
            let new_entries = entries.into_iter().map(|entry| ConfigMapEntry {
                value: transform_paths(entry.value, &transform),
                ..entry
            }).collect();
            ConfigValue {
                value: ConfigValueKind::Map(new_entries),
                ..value
            }
        },
        ConfigValueKind::Array(items) => {
            let new_items = items.into_iter()
                .map(|item| transform_paths(item, &transform))
                .collect();
            ConfigValue {
                value: ConfigValueKind::Array(new_items),
                ..value
            }
        },
        // Other variants pass through unchanged
        _ => value,
    }
}
```

### Path Relativity Calculation

To convert a path relative to `metadata_dir` to be relative to `document_dir`:

```rust
use std::path::Path;

fn make_document_relative(
    path: &str,
    metadata_dir: &Path,
    document_dir: &Path,
) -> String {
    // 1. Make the path absolute (relative to metadata_dir)
    let absolute = metadata_dir.join(path);

    // 2. Make it relative to document_dir
    // Use pathdiff crate or manual calculation
    if let Some(relative) = pathdiff::diff_paths(&absolute, document_dir) {
        relative.to_string_lossy().to_string()
    } else {
        // Fallback: return original if can't compute relative
        path.to_string()
    }
}
```

## References

- TS Quarto implementation: `~/src/quarto-cli/src/project/project-shared.ts` (`directoryMetadataForInputFile`)
- TS Quarto merge: `~/src/quarto-cli/src/command/render/render-contexts.ts` (`renderContexts`)
- Previous plan: `claude-notes/plans/2026-02-16-project-metadata-merging.md`
- Schema validation: `crates/quarto-yaml-validation/src/validator.rs`
- Schema gap analysis: `claude-notes/investigations/2026-02-17-schema-validation-gap-analysis.md`

## TS Quarto Code Analysis (2026-02-17)

### directoryMetadataForInputFile (project-shared.ts:299-355)

```typescript
export async function directoryMetadataForInputFile(
  project: ProjectContext,
  inputDir: string,  // <-- NOTE: this is the INPUT DIRECTORY, not the file itself
) {
  const projectDir = project.dir;

  // Finds _metadata.yml or _metadata.yaml
  const metadataFile = (dir: string) => {
    return ["_metadata.yml", "_metadata.yaml"]
      .map((file) => join(dir, file))
      .find(existsSync1);
  };

  // Walk from project root to input directory
  const relativePath = relative(projectDir, inputDir);
  const dirs = relativePath.split(SEP_PATTERN);

  let config = {};
  let currentDir = projectDir;

  for (let i = 0; i < dirs.length; i++) {
    const dir = dirs[i];
    currentDir = join(currentDir, dir);
    const file = metadataFile(currentDir);
    if (file) {
      // Validates against frontMatterSchema
      const yaml = await readAndValidateYamlFromFile(file, frontMatterSchema, errMsg);

      // Normalize format key
      if (yaml.format) {
        yaml.format = normalizeFormatYaml(yaml.format);
      }

      // Convert paths and merge (deeper overrides shallower)
      config = mergeConfigs(
        config,
        toInputRelativePaths(projectType, currentDir, inputDir, yaml),
      );
    }
  }
  return config;
}
```

**Key observations:**
1. Takes `inputDir` (document's parent directory), NOT the document path itself
2. Walks each directory component from project root to inputDir
3. Does NOT include project root's _metadata.yml (starts walking from first subdir)
4. Uses `mergeConfigs` which does deep merge - later values override earlier for scalars, arrays concatenate

### toInputRelativePaths (project-shared.ts:137-206)

This is the **implicit path resolution** - walks every string and probes filesystem:

```typescript
export function toInputRelativePaths(type, baseDir, inputDir, collection) {
  const existsCache = new Map<string, string>();
  const offset = relative(inputDir, baseDir);

  const fixup = (value: string) => {
    if (!existsCache.has(value)) {
      const projectPath = join(baseDir, value);
      try {
        if (existsSync(projectPath)) {
          existsCache.set(value, pathWithForwardSlashes(join(offset!, value)));
        } else {
          existsCache.set(value, value);
        }
      } catch {
        existsCache.set(value, value);
      }
    }
    return existsCache.get(value);
  };

  // Recursively walks arrays and objects, calling fixup on every string
  const inner = (collection, parentKey?) => {
    // ... walks structure and calls fixup on strings
  };

  inner(collection);
  return collection;
}
```

**q2 difference**: We use explicit `!path` tags instead of filesystem probing. This is cleaner but requires users to mark paths explicitly.

### Merge order in render-contexts.ts:397-671

```typescript
// resolveFormats function shows the merge order:
const userFormat = mergeFormatMetadata(
  projFormat || {},      // 1. Project config
  directoryFormat || {}, // 2. Directory metadata (our new layer)
  inputFormat || {},     // 3. Document frontmatter
);
```

So the layering is: project → directory → document (document wins)
