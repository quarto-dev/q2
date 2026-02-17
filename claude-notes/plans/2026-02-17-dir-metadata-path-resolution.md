# Directory Metadata Path Resolution

**Date**: 2026-02-17
**Status**: Complete
**Depends on**: `2026-02-17-metadata-yml-support.md` (Complete)

## Completion Summary

All phases implemented successfully:

- [x] **Phase 1: Unit Tests** - 7 tests in `directory_metadata_tests` module
- [x] **Phase 2: Core Implementation** - `adjust_paths_to_document_dir()` function
- [x] **Phase 3: Integration** - Called from `directory_metadata_for_document()`
- [x] **Phase 4: Integration Tests** - `smoke-all/metadata/dir-metadata-paths/`

### Key Implementation Details

1. Added `pathdiff = "0.2"` to `quarto-core/Cargo.toml`
2. Implemented recursive path adjustment in `crates/quarto-core/src/project.rs`
3. Path adjustment handles:
   - Relative `!path` values: adjusted via `pathdiff::diff_paths`
   - Absolute paths: unchanged
   - URLs: unchanged
   - Globs and other types: unchanged
   - Nested values in arrays and maps: recursively adjusted

### Verification

- All 15 directory metadata tests pass
- All 6488 workspace tests pass (4 pre-existing Pandoc-version failures excluded)
- smoke_all test suite passes including new `dir-metadata-paths` test

## Context

### What is q2?

q2 (Quarto Rust) is a Rust rewrite of Quarto, a scientific publishing system. It renders `.qmd` (Quarto Markdown) documents to HTML and other formats.

### What is `_metadata.yml`?

Quarto projects support directory-level metadata files named `_metadata.yml`. These files provide default metadata that applies to all documents in that directory and its subdirectories.

Example project structure:
```
project/
  _quarto.yml           # Project config
  chapters/
    _metadata.yml       # Applies to all docs in chapters/
    intro/
      _metadata.yml     # Applies to docs in chapters/intro/
      chapter1.qmd      # Document being rendered
```

When rendering `chapter1.qmd`, metadata is merged in this order (later wins):
1. `_quarto.yml` (project)
2. `chapters/_metadata.yml` (directory)
3. `chapters/intro/_metadata.yml` (directory)
4. `chapter1.qmd` frontmatter (document)

### What's Already Implemented?

The core directory metadata discovery is complete (see `2026-02-17-metadata-yml-support.md`):

**Function**: `crates/quarto-core/src/project.rs::directory_metadata_for_document()`

```rust
/// Find and parse all `_metadata.yml` files between project root and document directory.
pub fn directory_metadata_for_document(
    project: &ProjectContext,
    document_path: &Path,
) -> Result<Vec<ConfigValue>> {
    // Walks from project root to document's parent directory
    // Returns Vec of ConfigValue layers (root → leaf order)
}
```

This function currently parses `_metadata.yml` files but does NOT adjust paths. This plan implements that missing piece.

### What is ConfigValue?

`ConfigValue` (defined in `crates/quarto-pandoc-types/src/config_value.rs`) is q2's representation of YAML configuration:

```rust
pub struct ConfigValue {
    pub value: ConfigValueKind,     // The actual value
    pub source_info: SourceInfo,    // Where it came from
    pub merge_op: MergeOp,          // How to merge with other layers
}

pub enum ConfigValueKind {
    Scalar(Yaml),              // Plain strings, ints, bools, etc. - NOT paths
    PandocInlines(Inlines),    // Markdown inline content
    PandocBlocks(Blocks),      // Markdown block content
    Path(String),              // Explicitly tagged as a path with !path
    Glob(String),              // Glob pattern with !glob
    Expr(String),              // Runtime expression with !expr
    Array(Vec<ConfigValue>),   // Array of values
    Map(Vec<ConfigMapEntry>),  // Map with ConfigMapEntry { key, key_source, value }
}
```

**Key insight**: Only `ConfigValueKind::Path(String)` values need adjustment. Plain strings in `Scalar(Yaml::String(...))` are left alone.

### What are `!path` tags?

In YAML, `!path` is a custom tag that marks a value as a file path:

```yaml
# In _metadata.yml
css: !path ./styles.css          # Parsed as ConfigValueKind::Path
bibliography: !path ../refs.bib  # Parsed as ConfigValueKind::Path
theme: cosmo                     # Parsed as ConfigValueKind::String (NOT adjusted)
```

The `quarto_yaml` crate handles parsing these tags. When `InterpretationContext::ProjectConfig` is used, `!path` values become `ConfigValueKind::Path`.

### Why is Path Adjustment Needed?

Consider:
```
project/
  refs.bib
  chapters/
    _metadata.yml       # bibliography: !path ../refs.bib
    intro/
      chapter1.qmd      # Being rendered
```

The `bibliography: !path ../refs.bib` is relative to `chapters/` (where `_metadata.yml` lives). But when `chapter1.qmd` is rendered, it's in `chapters/intro/`, so the path needs to become `../../refs.bib` to correctly reference the same file.

**This is what this plan implements.**

## Design

### Path Resolution Algorithm

Given:
- `metadata_dir`: Directory containing the `_metadata.yml` (e.g., `/project/chapters/`)
- `document_dir`: Directory containing the document (e.g., `/project/chapters/intro/`)
- `path`: The path value from ConfigValueKind::Path (e.g., `../refs.bib`)

Algorithm:
1. Compute "absolute" path by joining: `abs_path = metadata_dir.join(path)`
   - `/project/chapters/` + `../refs.bib` → `/project/chapters/../refs.bib`
   - Note: We do NOT canonicalize (which would require file to exist)
2. Compute relative path from document_dir: `pathdiff::diff_paths(abs_path, document_dir)`
   - `pathdiff` handles the `..` components correctly
   - From `/project/chapters/intro/` to `/project/chapters/../refs.bib` → `../../refs.bib`

**Edge cases:**
- Absolute paths (`/usr/share/file.css`): Pass through unchanged
- URLs (`https://example.com/style.css`): Pass through unchanged
- Already correct (metadata_dir == document_dir): `pathdiff` returns the original path
- Non-existent files: Works fine - we don't check if files exist

### Where to Integrate

In `directory_metadata_for_document()`, after parsing each `_metadata.yml` but before adding to the layers vec:

```rust
// Current code (simplified):
for component in components {
    current_dir = current_dir.join(component);
    if let Some(path) = find_metadata_file(&current_dir) {
        let metadata = parse_metadata_file(&path)?;
        // TODO: Add path adjustment HERE
        //   adjust_paths_to_document_dir(&mut metadata, &current_dir, document_dir);
        layers.push(metadata);
    }
}
```

## Implementation Plan

**IMPORTANT**: Follow TDD workflow per CLAUDE.md - write tests first, verify they fail, then implement.

### Phase 1: Unit Tests (Write First)

**File**: `crates/quarto-core/src/project.rs` (in `directory_metadata_tests` module)

Add tests that create `_metadata.yml` files with `!path` values and verify they're adjusted:

```rust
#[test]
fn test_path_adjusted_for_subdirectory() {
    // project/
    //   shared/
    //     styles.css        # The actual file
    //   chapters/
    //     _metadata.yml     # css: !path ../shared/styles.css
    //     intro/
    //       doc.qmd
    //
    // When rendering doc.qmd, css should become "../../shared/styles.css"
}

#[test]
fn test_path_same_directory_unchanged() {
    // project/
    //   chapters/
    //     _metadata.yml     # css: !path ./local.css
    //     doc.qmd           # Same directory
    //
    // Path stays "./local.css" (or normalized equivalent)
}

#[test]
fn test_plain_string_not_adjusted() {
    // project/
    //   chapters/
    //     _metadata.yml     # theme: cosmo (plain string, not !path)
    //     intro/
    //       doc.qmd
    //
    // "cosmo" must NOT be changed to "../cosmo" or anything else
}

#[test]
fn test_absolute_path_unchanged() {
    // css: !path /usr/share/styles/base.css
    // Should pass through unchanged
}

#[test]
fn test_array_of_paths_all_adjusted() {
    // css:
    //   - !path ../shared/a.css
    //   - !path ../shared/b.css
    // Both should be adjusted
}

#[test]
fn test_glob_not_adjusted() {
    // resources: !glob ../images/*.png
    // Globs are patterns, not paths - should NOT be adjusted
}
```

### Phase 2: Core Implementation

**File**: `crates/quarto-core/src/project.rs`

Add helper function:

```rust
use std::path::Path;

/// Adjust `!path` values in metadata to be relative to document directory.
///
/// Walks the ConfigValue tree and for each `ConfigValueKind::Path`:
/// - Computes absolute path relative to metadata_dir
/// - Recomputes relative path from document_dir
///
/// Leaves other values (strings, globs, etc.) unchanged.
fn adjust_paths_to_document_dir(
    metadata: &mut ConfigValue,
    metadata_dir: &Path,
    document_dir: &Path,
) {
    adjust_paths_recursive(metadata, metadata_dir, document_dir);
}

/// Recursively walk ConfigValue, adjusting Path variants.
fn adjust_paths_recursive(
    value: &mut ConfigValue,
    metadata_dir: &Path,
    document_dir: &Path,
) {
    use quarto_pandoc_types::config_value::ConfigValueKind;
    use std::path::PathBuf;

    match &mut value.value {
        ConfigValueKind::Path(path_str) => {
            let path = PathBuf::from(&*path_str);
            // Only adjust relative paths (not absolute, not URLs)
            if path.is_relative() && !path_str.starts_with("http://") && !path_str.starts_with("https://") {
                let abs_path = metadata_dir.join(&path);
                if let Some(adjusted) = pathdiff::diff_paths(&abs_path, document_dir) {
                    *path_str = adjusted.to_string_lossy().into_owned();
                }
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                adjust_paths_recursive(item, metadata_dir, document_dir);
            }
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                adjust_paths_recursive(&mut entry.value, metadata_dir, document_dir);
            }
        }
        // All other kinds (Scalar, PandocInlines, Glob, Expr, etc.) - no adjustment
        _ => {}
    }
}
```

**Note**: The `pathdiff` crate provides `diff_paths(target, base)` which computes a relative path from `base` to `target`.

### Phase 3: Integration

**File**: `crates/quarto-core/src/project.rs`

Update `directory_metadata_for_document()` at line ~133 (after `yaml_to_config_value` call, before `layers.push`):

```rust
// Current code around line 132-137:
let mut metadata =
    yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

// ADD THIS: Adjust paths to be relative to document directory
adjust_paths_to_document_dir(&mut metadata, &current_dir, document_dir);

layers.push(metadata);
```

Note: `document_dir` is already computed earlier in the function (line 87-89).

### Phase 4: Integration Tests

**Location**: `crates/quarto/tests/smoke-all/metadata/dir-metadata-paths/`

Create test structure:
```
dir-metadata-paths/
  _quarto.yml
  shared/
    styles.css              # Actual CSS file
  chapters/
    _metadata.yml           # css: !path ../shared/styles.css
    intro/
      doc.qmd               # Verify CSS link resolves correctly
```

Test assertions in `doc.qmd`:
```yaml
_quarto:
  tests:
    html:
      ensureFileRegexMatches:
        - ["../../shared/styles.css"]  # Adjusted path appears in HTML
```

## Dependencies

Add `pathdiff` crate to `crates/quarto-core/Cargo.toml` if not present:
```toml
pathdiff = "0.2"
```

## Verification

After implementation:
1. Run unit tests: `cargo nextest run -p quarto-core directory_metadata`
2. Run smoke tests: `cargo nextest run -p quarto smoke_all`
3. Run full verification: `cargo xtask verify`

## References

- Existing implementation: `crates/quarto-core/src/project.rs::directory_metadata_for_document()`
- ConfigValue types: `crates/quarto-pandoc-types/src/config_value.rs`
- YAML parsing: `crates/quarto-yaml/src/lib.rs`
- TS Quarto reference: `~/src/quarto-cli/src/project/project-shared.ts:137-206` (`toInputRelativePaths`)
- Parent plan: `claude-notes/plans/2026-02-17-metadata-yml-support.md`
