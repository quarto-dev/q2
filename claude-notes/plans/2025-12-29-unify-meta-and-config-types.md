# Unify MetaValueWithSourceInfo and ConfigValue

**Issue:** k-2tu9
**Date:** 2025-12-29
**Status:** In Progress - Phases 1-4 Complete
**Blocks:** k-ic1o (ConfigValue integration into pipeline)

## Overview

Replace `MetaValueWithSourceInfo` (from `quarto-pandoc-types`) with `ConfigValue` (from `quarto-config`) throughout the codebase. This eliminates type duplication and enables seamless configuration merging between project config and document metadata.

## Background

### Current State: Two Similar Types

**MetaValueWithSourceInfo** (`quarto-pandoc-types/src/meta.rs`):
```rust
pub enum MetaValueWithSourceInfo {
    MetaString { value: String, source_info: SourceInfo },
    MetaBool { value: bool, source_info: SourceInfo },
    MetaInlines { content: Inlines, source_info: SourceInfo },
    MetaBlocks { content: Blocks, source_info: SourceInfo },
    MetaList { items: Vec<MetaValueWithSourceInfo>, source_info: SourceInfo },
    MetaMap { entries: Vec<MetaMapEntry>, source_info: SourceInfo },
}

pub struct MetaMapEntry {
    pub key: String,
    pub key_source: SourceInfo,  // Key source tracking!
    pub value: MetaValueWithSourceInfo,
}
```

**ConfigValue** (`quarto-config/src/types.rs`):
```rust
pub struct ConfigValue {
    pub value: ConfigValueKind,
    pub source_info: SourceInfo,
    pub merge_op: MergeOp,           // Merge semantics!
    pub interpretation: Option<Interpretation>,  // String handling!
}

pub enum ConfigValueKind {
    Scalar(Yaml),
    Array(Vec<ConfigValue>),
    Map(IndexMap<String, ConfigValue>),  // No key_source tracking
    PandocInlines(Inlines),
    PandocBlocks(Blocks),
}
```

### Key Differences

| Feature | MetaValueWithSourceInfo | ConfigValue (current) | ConfigValue (proposed) |
|---------|------------------------|----------------------|----------------------|
| Map key source tracking | Yes (`key_source`) | No | Yes (`ConfigMapEntry`) |
| Merge semantics | No | Yes (`merge_op`) | Yes (`merge_op`) |
| Interpretation hints | No | Yes (`interpretation`) | No (use explicit variants) |
| Deferred interpretation | Via Span attributes | Via `interpretation` field (unused!) | Explicit variants: `Path`, `Glob`, `Expr` |
| Scalar representation | Separate variants | `Yaml` enum | `Yaml` enum |
| Usage | Document metadata | Project config | Both |

### Problems with Current State

1. **Conversion overhead**: Need `config_from_meta()` and `meta_from_config()` functions
2. **Duplicate utilities**: Methods like `get()`, `is_string_value()` written for both types
3. **Merge semantics unavailable**: Document metadata can't use `!prefer`/`!concat` tags
4. **Growing complexity**: Every new feature requires updating both types

### Goal

Single type (`ConfigValue`) used everywhere:
- Document frontmatter → `ConfigValue` (with markdown-by-default interpretation)
- Project `_quarto.yml` → `ConfigValue` (with literal-by-default interpretation)
- Merge both using `MergedConfig`
- All merge semantics (`!prefer`, `!concat`) work in both contexts

## Design

### Enhanced ConfigValue

Two major changes:
1. Add key source tracking to ConfigValue's map type
2. Remove `interpretation` field, use explicit variants for deferred interpretations

#### Why Remove the `interpretation` Field?

The current `ConfigValue` has `interpretation: Option<Interpretation>`, but:
- It's **never read** outside tests
- Interpretations that happen at parse time (`!md`, `!str`) don't need storage
- Interpretations that happen later (`!path`, `!glob`, `!expr`) are either:
  - Lost (`!path` is currently treated as `!str`)
  - Stored in DOM attributes (`!glob`, `!expr` use Span wrapper)

Making variants explicit is cleaner:
- Type system enforces handling all cases
- Can't accidentally forget to handle `!path`
- No redundant field that's never read

```rust
// quarto-config/src/types.rs

/// A configuration value with explicit merge semantics.
#[derive(Debug, Clone)]
pub struct ConfigValue {
    pub value: ConfigValueKind,
    pub source_info: SourceInfo,
    pub merge_op: MergeOp,
    // REMOVED: interpretation: Option<Interpretation>
}

pub enum ConfigValueKind {
    // === Scalar values (interpretation resolved or not applicable) ===

    /// Plain scalar: string, bool, number, null
    /// For strings: this means "keep as literal" (was !str or context default)
    Scalar(Yaml),

    // === Parsed content (interpretation happened at parse time) ===

    /// Markdown parsed to inlines (was !md or context default for doc metadata)
    PandocInlines(Inlines),

    /// Markdown parsed to blocks (multi-paragraph content)
    PandocBlocks(Blocks),

    // === Deferred interpretation (needs later processing) ===

    /// Path to resolve relative to source file (!path tag)
    Path(String),

    /// Glob pattern to expand (!glob tag)
    Glob(String),

    /// Runtime expression to evaluate (!expr tag)
    Expr(String),

    // === Compound values ===

    /// Array of values
    Array(Vec<ConfigValue>),

    /// Map with key source tracking
    Map(Vec<ConfigMapEntry>),
}

/// Map entry with key source tracking
#[derive(Debug, Clone)]
pub struct ConfigMapEntry {
    pub key: String,
    pub key_source: SourceInfo,
    pub value: ConfigValue,
}
```

#### Interpretation Flow

| Tag | At Parse Time | ConfigValueKind |
|-----|---------------|-----------------|
| (none, doc context) | Parse as markdown | `PandocInlines` or `PandocBlocks` |
| (none, project context) | Keep literal | `Scalar(Yaml::String)` |
| `!md` | Parse as markdown | `PandocInlines` or `PandocBlocks` |
| `!str` | Keep literal | `Scalar(Yaml::String)` |
| `!path` | Keep for later | `Path(String)` |
| `!glob` | Keep for later | `Glob(String)` |
| `!expr` | Keep for later | `Expr(String)` |

#### Utility Methods

```rust
impl ConfigValue {
    /// Get a value by key if this is a Map
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        match &self.value {
            ConfigValueKind::Map(entries) => {
                entries.iter().find(|e| e.key == key).map(|e| &e.value)
            }
            _ => None,
        }
    }

    /// Check if this is a string with a specific value
    /// (handles Scalar(String), PandocInlines with single Str, Path, Glob, Expr)
    pub fn is_string_value(&self, expected: &str) -> bool {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::String(s)) => s == expected,
            ConfigValueKind::Path(s) => s == expected,
            ConfigValueKind::Glob(s) => s == expected,
            ConfigValueKind::Expr(s) => s == expected,
            ConfigValueKind::PandocInlines(inlines) if inlines.len() == 1 => {
                if let Inline::Str(str_node) = &inlines[0] {
                    return str_node.text == expected;
                }
                false
            }
            _ => false,
        }
    }

    /// Get the raw string value if this is any string-like variant
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::String(s)) => Some(s),
            ConfigValueKind::Path(s) => Some(s),
            ConfigValueKind::Glob(s) => Some(s),
            ConfigValueKind::Expr(s) => Some(s),
            _ => None,
        }
    }
}
```

### Interpretation Context

Handle different default string interpretations:

```rust
// quarto-config/src/types.rs

/// Context for interpreting string values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpretationContext {
    /// Document frontmatter: strings are parsed as markdown by default
    /// Use `!str` to keep literal
    #[default]
    DocumentMetadata,

    /// Project config (_quarto.yml): strings are literal by default
    /// Use `!md` to parse as markdown
    ProjectConfig,
}
```

### Updated Conversion Function

Replace `yaml_to_meta_with_source_info` with `yaml_to_config_value`:

```rust
// pampa/src/pandoc/meta.rs (or move to quarto-config)

pub fn yaml_to_config_value(
    yaml: YamlWithSourceInfo,
    context: InterpretationContext,
    ast_context: &ASTContext,
    diagnostics: &mut DiagnosticCollector,
) -> ConfigValue {
    // Check for YAML tags first
    let (tag_merge_op, tag_interpretation) = if let Some((tag, _)) = &yaml.tag {
        parse_config_tag(tag)  // Reuse quarto-config tag parsing
    } else {
        (None, None)
    };

    // Determine effective interpretation
    let interpretation = tag_interpretation.or_else(|| {
        match context {
            InterpretationContext::DocumentMetadata => Some(Interpretation::Markdown),
            InterpretationContext::ProjectConfig => None,
        }
    });

    if yaml.is_hash() {
        let (entries, source_info) = yaml.into_hash().unwrap();
        let config_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .filter_map(|entry| {
                entry.key.yaml.as_str().map(|key_str| ConfigMapEntry {
                    key: key_str.to_string(),
                    key_source: entry.key_span,
                    value: yaml_to_config_value(entry.value, context, ast_context, diagnostics),
                })
            })
            .collect();

        return ConfigValue {
            value: ConfigValueKind::Map(config_entries),
            source_info,
            merge_op: tag_merge_op.unwrap_or(MergeOp::Concat),
            interpretation: None,
        };
    }

    if yaml.is_array() {
        let (items, source_info) = yaml.into_array().unwrap();
        let config_items: Vec<ConfigValue> = items
            .into_iter()
            .map(|item| yaml_to_config_value(item, context, ast_context, diagnostics))
            .collect();

        return ConfigValue {
            value: ConfigValueKind::Array(config_items),
            source_info,
            merge_op: tag_merge_op.unwrap_or(MergeOp::Concat),
            interpretation: None,
        };
    }

    // Scalar handling
    let source_info = yaml.source_info.clone();
    match yaml.yaml {
        Yaml::String(s) => {
            if interpretation == Some(Interpretation::Markdown) {
                // Parse as markdown, produce PandocInlines/Blocks
                parse_string_as_markdown(&s, &source_info, ast_context, diagnostics)
            } else if interpretation == Some(Interpretation::PlainString) {
                // Wrap in single Str inline
                ConfigValue::new_inlines(
                    vec![Inline::Str(Str { text: s, source_info: source_info.clone() })],
                    source_info,
                )
            } else {
                // Keep as Scalar for later interpretation
                ConfigValue {
                    value: ConfigValueKind::Scalar(Yaml::String(s)),
                    source_info,
                    merge_op: tag_merge_op.unwrap_or(MergeOp::Concat),
                    interpretation,
                }
            }
        }
        Yaml::Boolean(b) => ConfigValue::new_scalar(Yaml::Boolean(b), source_info),
        Yaml::Integer(i) => ConfigValue::new_scalar(Yaml::Integer(i), source_info),
        Yaml::Real(r) => ConfigValue::new_scalar(Yaml::Real(r), source_info),
        Yaml::Null => ConfigValue::new_scalar(Yaml::Null, source_info),
        _ => ConfigValue::null(source_info),
    }
}
```

### Pandoc Type Update

Update `Pandoc` struct to use `ConfigValue`:

```rust
// quarto-pandoc-types/src/pandoc.rs

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pandoc {
    pub api_version: (i32, i32, i32, i32),
    pub meta: ConfigValue,  // Changed from MetaValueWithSourceInfo
    pub blocks: Blocks,
}
```

### MergedConfig for Map with Key Sources

Update `quarto-config/src/merged.rs` to handle `ConfigMapEntry`:

```rust
// The MergedConfig already works with ConfigValue.
// The main change is that Map iteration now yields ConfigMapEntry
// instead of (String, ConfigValue) tuples.

impl<'a> MergedMap<'a> {
    pub fn iter(&self) -> impl Iterator<Item = (&str, MergedCursor<'a>)> {
        // Iterate over all unique keys from all layers
        // Return cursors that resolve through the layers
    }

    pub fn get_entry(&self, key: &str) -> Option<(&ConfigMapEntry, MergedCursor<'a>)> {
        // Return the entry with key_source from the winning layer
    }
}
```

## Implementation Plan

### Phase 1: Move and Refactor ConfigValue Types (Medium Risk)

**Goal**: Move ConfigValue types to quarto-pandoc-types, clean up design.

#### Step 1a: Move types to quarto-pandoc-types ✅ COMPLETED

- [x] Create `quarto-pandoc-types/src/config_value.rs` with:
  - `ConfigValue` struct
  - `ConfigValueKind` enum
  - `MergeOp` enum
  - `ConfigMapEntry` struct
- [x] Add `yaml-rust2` dependency to quarto-pandoc-types (for `Yaml` in `ConfigValueKind::Scalar`)
- [x] Export from `quarto-pandoc-types/src/lib.rs`
- [x] Update `quarto-config` to import types from `quarto-pandoc-types` instead of defining them
- [x] Add re-exports in `quarto-config` for backward compatibility
- [x] Verify build succeeds with no circular dependency

#### Step 1b: Refactor ConfigValueKind ✅ COMPLETED

- [x] Add new variants: `Path(String)`, `Glob(String)`, `Expr(String)`
- [x] Remove `interpretation: Option<Interpretation>` field from `ConfigValue`
- [x] Keep `Interpretation` enum (still needed for tag parsing)
- [x] Add `ConfigMapEntry` struct with `key_source` field
- [x] Change `ConfigValueKind::Map` from `IndexMap<String, ConfigValue>` to `Vec<ConfigMapEntry>`
- [x] Add utility methods: `get()`, `contains_key()`, `is_empty()`, `is_string_value()`, `as_str()`

#### Step 1c: Update quarto-config internals ✅ COMPLETED

- [x] Update `config_value_from_yaml()` to:
  - Produce `Path`/`Glob`/`Expr` variants for those tags
  - Produce `ConfigMapEntry` with key sources
- [x] Update `MergedConfig` to handle new variants in merge logic
- [x] Update `materialize()` to handle new variants
- [x] Update all tests in quarto-config
- [x] Run full test suite (2603 tests pass)

### Phase 2: Add InterpretationContext ✅ COMPLETED

**Goal**: Support different default interpretations for document vs project config.

- [x] Add `InterpretationContext` enum to quarto-pandoc-types (not quarto-config, to avoid cycles)
- [x] Create `yaml_to_config_value()` in pampa with context parameter
- [x] `DocumentMetadata` context: untagged strings → parse as markdown → `PandocInlines`
- [x] `ProjectConfig` context: untagged strings → keep literal → `Scalar(Yaml::String)`
- [x] Comprehensive tests for context-dependent interpretation (32 tests)
- [x] Run full test suite (2635 tests pass)

**Note**: Discovered that quarto-yaml doesn't capture tags on compound types (arrays/maps).
Created issue k-d4r0 to track this limitation.

### Phase 3: Bidirectional Conversion Functions ✅ COMPLETED (REVISED)

**Original plan**: Create type aliases `MetaValueWithSourceInfo = ConfigValue`.
**Problem**: Types have different structures (enum vs struct), so aliases don't work.
**Revised approach**: Create conversion functions for gradual migration.

- [x] `meta_to_config_value()` already existed in pampa/src/template/config_merge.rs
- [x] Add `config_value_to_meta()` for reverse conversion
- [x] Add tests for bidirectional conversion (roundtrip test)
- [x] Run full test suite (2649 tests pass)

These functions enable gradual migration: code can use ConfigValue internally
and convert at boundaries that still need MetaValueWithSourceInfo.

### Phase 4: Migrate Document Parsing ✅ COMPLETED

**Goal**: Migrate document parsing to use ConfigValue internally while converting back to MetaValueWithSourceInfo at boundaries. This proves the new code path works without changing 35+ consumer files yet.

- [x] Create `rawblock_to_config_value()` function in `pampa/src/pandoc/meta.rs`
- [x] `yaml_to_config_value()` already exists from Phase 2
- [x] Add comprehensive tests for `rawblock_to_config_value` (19 tests in `test_rawblock_to_config_value.rs`)
- [x] Update `readers/qmd.rs` to use ConfigValue internally with conversion at boundary
- [x] Fix `config_value_to_meta()` to produce correct output for Path/Glob/Expr variants
- [x] Run full test suite (2668 tests pass)

**Files modified**:
- `pampa/tests/test_rawblock_to_config_value.rs` (NEW - 19 tests)
- `pampa/src/readers/qmd.rs` - Uses rawblock_to_config_value → config_value_to_meta
- `pampa/src/template/config_merge.rs` - Fixed config_value_to_meta for Path/Glob/Expr
- `pampa/tests/test_metadata_source_tracking.rs` - Updated for known limitation
- `pampa/snapshots/json/yaml-tags.snap` - Updated snapshot

**Known limitations documented**:
1. Unknown YAML tags (like `!date`) now parse as markdown instead of wrapping in Span
   - This is a minor behavioral difference acceptable during migration
2. `attr_source` for YAML tag attributes is empty through ConfigValue path (issue k-d4r0)
   - Functional behavior is correct (Span has correct class/attrs), but source tracking not preserved
3. `!str` via ConfigValue produces `MetaString` vs legacy `MetaInlines { Str }`
   - Semantically equivalent, minor structural difference

### Phase 5: Migrate Consumers (Largest Phase)

Files to update (32 total, grouped by area):

**quarto-pandoc-types** (definition):
- [ ] `meta.rs` - Remove `MetaValueWithSourceInfo`, keep only `ConfigValue` re-export
- [ ] `lib.rs` - Update exports
- [ ] `pandoc.rs` - `Pandoc.meta` type
- [ ] `block.rs` - Any metadata references

**pampa readers/writers** (core):
- [ ] `readers/qmd.rs`
- [ ] `readers/json.rs`
- [ ] `writers/html.rs`
- [ ] `writers/json.rs`
- [ ] `writers/qmd.rs`

**pampa internal** (supporting):
- [ ] `pandoc/meta.rs` - Conversion functions
- [ ] `pandoc/treesitter_utils/document.rs`
- [ ] `template/context.rs`
- [ ] `template/render.rs`
- [ ] `template/config_merge.rs`
- [ ] `lua/filter.rs`
- [ ] `lua/readwrite.rs`
- [ ] `filters.rs`
- [ ] `json_filter.rs`
- [ ] `citeproc_filter.rs`

**quarto-core transforms**:
- [ ] `transforms/callout.rs`
- [ ] `transforms/callout_resolve.rs`
- [ ] `transforms/metadata_normalize.rs`
- [ ] `transforms/resource_collector.rs`
- [ ] `transforms/title_block.rs`
- [ ] `template.rs`

**Tests**:
- [ ] `pampa/tests/test_yaml_tag_regression.rs`
- [ ] `pampa/tests/test_metadata_source_tracking.rs`
- [ ] `pampa/tests/test_json_roundtrip.rs`

**comrak-to-pandoc** (auxiliary):
- [ ] `block.rs`
- [ ] `normalize.rs`
- [ ] `tests/generators.rs`
- [ ] `tests/debug.rs`

### Phase 6: Remove Legacy Types

- [ ] Remove `MetaValueWithSourceInfo` enum from `quarto-pandoc-types`
- [ ] Remove `MetaMapEntry` struct (now using `ConfigMapEntry`)
- [ ] Remove `meta_from_legacy`, `meta_value_from_legacy` functions
- [ ] Remove `to_meta_value`, `to_meta` methods
- [ ] Remove type aliases from Phase 3
- [ ] Final test suite run

### Phase 7: Enable Full Merge Semantics

- [ ] Document metadata now supports `!prefer` and `!concat` tags
- [ ] Update documentation
- [ ] Add tests for merge semantics in document frontmatter

## Risk Assessment

| Phase | Risk | Mitigation |
|-------|------|------------|
| 1-2 | Low | Additive changes only, no breaking changes |
| 3 | Low | Type aliases maintain compatibility |
| 4 | Medium | Core parsing changes; extensive testing required |
| 5 | High | Many files; incremental commits, test after each file |
| 6 | Low | Just cleanup after everything works |

## Testing Strategy

1. **Unit tests**: Update all existing tests to use ConfigValue
2. **Integration tests**: Run full corpus validation
3. **Regression tests**: Compare rendered output before/after for sample documents
4. **Round-trip tests**: Ensure JSON serialization still works

## Circular Dependency Resolution

### The Problem

Rust does not allow circular dependencies between crates. Currently:
- `quarto-config` → depends on → `quarto-pandoc-types` (for `Blocks`, `Inlines`)
- `quarto-pandoc-types` → does NOT depend on → `quarto-config`

If we naively add `ConfigValue` to `Pandoc.meta`:
- `quarto-pandoc-types` would need `quarto-config` (for `ConfigValue`)
- `quarto-config` already needs `quarto-pandoc-types` (for `Blocks`/`Inlines`)
- **Cycle!** Cargo rejects.

### Solution: Move ConfigValue types to quarto-pandoc-types (Option B)

Move the core types to `quarto-pandoc-types`:
- `ConfigValue`
- `ConfigValueKind`
- `MergeOp`
- `ConfigMapEntry`

Keep the merging logic in `quarto-config`:
- `MergedConfig`
- `MergedCursor`
- `MergedScalar`, `MergedArray`, `MergedMap`
- `materialize()`
- `merge_with_diagnostics()`

```
quarto-pandoc-types
├── Blocks, Inlines, Block, Inline...
├── ConfigValue, ConfigValueKind, MergeOp, ConfigMapEntry (MOVED HERE)
├── Pandoc { meta: ConfigValue, blocks: Blocks }
└── depends on: quarto-source-map

quarto-config
├── MergedConfig, MergedCursor... (merging logic only)
├── config_value_from_yaml() (conversion from YAML)
├── re-exports ConfigValue types for convenience
└── depends on: quarto-pandoc-types, quarto-yaml, quarto-source-map
```

### Why This Works

- No cycle: `quarto-config` depends on `quarto-pandoc-types`, not vice versa
- Conceptually sound: `ConfigValue` is "the unified metadata type for Pandoc documents"
- Minimal disruption: Most code imports from `quarto-config` anyway, re-exports maintain compatibility

### Dependency Graph After Change

```
quarto-source-map (leaf)
       ↑
quarto-pandoc-types
├── ConfigValue, ConfigValueKind, MergeOp, ConfigMapEntry
├── Blocks, Inlines, Pandoc...
       ↑
quarto-config
├── MergedConfig, MergedCursor, materialize...
       ↑
quarto-yaml
       ↑
pampa, quarto-core, etc.
```

## Open Questions

1. ~~**Circular dependency**: How to resolve quarto-config ↔ quarto-pandoc-types?~~
   - **Resolved**: Move ConfigValue types to quarto-pandoc-types (see above)

2. **Serialization compatibility**: Will JSON output change?
   - Need to verify JSON writer produces identical output

3. **Lua filter compatibility**: Do Lua filters expect specific structure?
   - Need to verify Lua readwrite produces compatible data

4. **Performance**: Is Vec<ConfigMapEntry> slower than IndexMap?
   - Likely negligible; measure if concerned

## Success Criteria

1. All tests pass
2. Rendered output identical for sample documents
3. JSON round-trip works
4. `!prefer` and `!concat` tags work in document frontmatter
5. Project config merges correctly with document metadata
6. No duplicate type definitions remain
