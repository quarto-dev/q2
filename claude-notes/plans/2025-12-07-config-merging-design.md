# Configuration Merging System Design

**Date**: 2025-12-07
**Issue**: k-zvzm
**Status**: Design proposal

**Incorporated subissue designs**:
- `k-vpgx`: MergedConfig lifetime design → cursor-based navigation (see RQ1)
- `k-os6h`: Error handling strategy → comprehensive error codes (see RQ2)

## Executive Summary

This document proposes a design for Quarto's Rust configuration merging system. The key insight from the Haskell `composable-validation` work is that we can achieve both proper associativity AND preference semantics by **separating construction from interpretation**.

**Recommendation**: Implement a tag-based merging system where `!prefer` and `!concat` YAML tags control merge behavior, with lazy evaluation for performance and source tracking for validation.

## Background

### Problem Statement

Quarto merges configuration from multiple sources:

```
default format configs
  ↓ (merge)
project-level configs (_quarto.yml)
  ↓ (merge)
directory-level configs (_metadata.yml)
  ↓ (merge)
document-level configs (YAML frontmatter)
  ↓ (merge)
command-line flags
  ↓
final configuration
```

The current TypeScript implementation uses `mergeConfigs()` with lodash's `mergeWith`, which:
1. Loses source location information after merge
2. Uses hardcoded logic (arrays concatenate, scalars override)
3. Accounts for ~15% of total runtime

### Goals for Rust Port

1. **Source location preservation**: Validation errors must point to the correct file/line
2. **Explicit merge semantics**: Users can control behavior via `!prefer` and `!concat` tags
3. **Performance**: Address the 15% runtime cost via lazy evaluation
4. **Associativity**: `(a <> b) <> c == a <> (b <> c)` for any configs a, b, c
5. **Integration**: Work naturally with existing `YamlWithSourceInfo` and `SourceInfo` types

### Non-Goals

The following are explicitly out of scope for this design:

1. **Schema validation**: Validation of merged configs against JSON schemas is handled by `quarto-yaml-validation`, not this system. This design focuses purely on merge semantics.

2. **Runtime expression evaluation**: The `!expr` tag marks values for runtime evaluation, but actual evaluation (in R/Python/Julia) happens elsewhere.

3. **File resolution**: The `!path` tag marks values as paths, but actual path resolution (relative to source file) happens during interpretation in pampa, not during merge.

4. **Circular include detection**: Detecting and handling circular `_metadata.yml` includes is a project-level concern, not a merge-level concern.

5. **Default format configs**: This design handles merging of user-provided configs. Default format configurations are generated elsewhere and passed to the merge system as regular layers.

## Existing Infrastructure

### 1. quarto-yaml: `YamlWithSourceInfo`

Already provides source-tracked YAML with tag support:

```rust
pub struct YamlWithSourceInfo {
    pub yaml: Yaml,                        // The raw value
    pub source_info: SourceInfo,           // Location tracking
    pub tag: Option<(String, SourceInfo)>, // YAML tags (!str, !path, etc.)
    children: Children,                    // Parallel source tracking
}
```

**Key observation**: The `tag` field can be extended to support `!prefer` and `!concat`.

### 2. quarto-source-map: `SourceInfo`

Tracks source locations through transformations:

```rust
pub enum SourceInfo {
    Original { file_id, start_offset, end_offset },
    Substring { parent, start_offset, end_offset },
    Concat { pieces: Vec<SourcePiece> },
    FilterProvenance { filter_path, line },
}
```

**Key observation**: `SourceInfo::Concat` already supports combining sources from different files.

### 3. pampa: `MetaValueWithSourceInfo`

Converts YAML to Pandoc metadata with source tracking. Currently handles tags like `!str`, `!path`, `!md`.

## Composable-Validation Insights

The Haskell work revealed a crucial insight: **tags must be preserved through composition** to maintain associativity.

### Why Tag Preservation Matters

Consider three config layers:

```yaml
# obj1: _quarto.yml
format:
  html: default

# obj2: _metadata.yml
format:
  !prefer pdf: default   # User wants PDF only, not HTML+PDF

# obj3: document.qmd
toc: true
```

**Without tag preservation** (TypeScript behavior):
```
(obj1 <> obj2) <> obj3 gives: {format: {pdf: default}, toc: true}
obj1 <> (obj2 <> obj3) gives: {format: {html: default, pdf: default}, toc: true}
```
These are different! The `!prefer` tag is "consumed" after the first merge.

**With tag preservation** (proposed design):
Both associativity orders give `{format: {pdf: default}, toc: true}` because the `!prefer` tag is preserved until final interpretation.

### The Construction/Interpretation Split

The key design pattern from `composable-validation`:

1. **Construction Phase**: Build a structure that records all operations (proper associative monoid)
2. **Interpretation Phase**: Apply merge semantics when extracting final values

This is how `TaggedList` works in Haskell:
```haskell
-- Construction: Just list concatenation (associative!)
TaggedList xs <> TaggedList ys = TaggedList (xs <> ys)

-- Interpretation: Apply prefer/concat semantics
lower :: TaggedList a -> [a]
lower (TaggedList tags) = -- apply preference logic here
```

## Design Proposal

### Core Type: `ConfigValue`

A new type that wraps `YamlWithSourceInfo` with explicit merge semantics:

```rust
/// Merge operation for a value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOp {
    /// This value overrides/resets previous values (from !prefer tag)
    Prefer,
    /// This value concatenates with previous values (from !concat tag or default)
    Concat,
}

/// Interpretation hint for string values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpretation {
    Markdown,    // !md - parse as markdown
    PlainString, // !str - keep as literal string
    Path,        // !path - resolve relative to source file
    Glob,        // !glob - treat as glob pattern
    Expr,        // !expr - runtime expression
}

/// A configuration value with explicit merge semantics
#[derive(Debug, Clone)]
pub struct ConfigValue {
    /// The underlying value
    pub value: ConfigValueKind,
    /// Source location for this value
    pub source_info: SourceInfo,
    /// Merge operation (derived from tag or inferred)
    pub merge_op: MergeOp,
    /// Interpretation hint for string values (derived from tag)
    pub interpretation: Option<Interpretation>,
}

#[derive(Debug, Clone)]
pub enum ConfigValueKind {
    /// Atomic values (String, Int, Float, Bool, Null)
    /// Always use "last wins" semantics regardless of MergeOp
    Scalar(Yaml),

    /// Arrays: merge_op controls concatenate vs replace
    Array(Vec<ConfigValue>),

    /// Objects: merge_op controls field-wise merge vs replace
    Map(IndexMap<String, ConfigValue>),

    /// Pandoc inline content (for already-interpreted values)
    /// Default: !prefer (last wins, no concatenation)
    /// Use !concat explicitly if concatenation is desired
    PandocInlines(Vec<Inline>),

    /// Pandoc block content (for already-interpreted values)
    /// Default: !prefer (last wins, no concatenation)
    /// Use !concat explicitly if concatenation is desired
    PandocBlocks(Vec<Block>),
}
```

### Construction: `MergedConfig` with Cursor-Based Navigation

For lazy evaluation, we don't immediately merge into a new tree. Instead, we keep references and use a cursor-based API for ergonomic navigation:

```rust
use indexmap::IndexMap;

/// A lazily-evaluated merged configuration
///
/// The lifetime parameter `'a` indicates that MergedConfig borrows from
/// existing ConfigValue data. This is zero-copy construction.
pub struct MergedConfig<'a> {
    /// Ordered list of config layers (first = lowest priority)
    layers: Vec<&'a ConfigValue>,
}

/// A cursor for navigating merged configuration
///
/// The cursor is lightweight: it stores a reference to the config
/// and a path. Resolution happens lazily when you call `as_*()` methods.
pub struct MergedCursor<'a> {
    config: &'a MergedConfig<'a>,
    path: Vec<String>,
}

/// A resolved scalar value with its source
pub struct MergedScalar<'a> {
    /// The resolved value
    pub value: &'a ConfigValue,
    /// Which layer this value came from (for debugging)
    pub layer_index: usize,
}

/// A resolved array with merge semantics applied
pub struct MergedArray<'a> {
    /// Items after applying prefer/concat semantics
    pub items: Vec<MergedArrayItem<'a>>,
}

pub struct MergedArrayItem<'a> {
    pub value: &'a ConfigValue,
    pub layer_index: usize,
}

/// A resolved map with merge semantics applied
pub struct MergedMap<'a> {
    config: &'a MergedConfig<'a>,
    path: Vec<String>,
    keys: Vec<String>,
}

/// A resolved value of any type (for when type is not known ahead of time)
pub enum MergedValue<'a> {
    Scalar(MergedScalar<'a>),
    Array(MergedArray<'a>),
    Map(MergedMap<'a>),
}

impl<'a> MergedConfig<'a> {
    /// Create a merged config from multiple layers
    pub fn new(layers: Vec<&'a ConfigValue>) -> Self {
        MergedConfig { layers }
    }

    /// Add a new layer (returns new MergedConfig, doesn't mutate)
    pub fn with_layer(&self, layer: &'a ConfigValue) -> MergedConfig<'a> {
        let mut new_layers = self.layers.clone();
        new_layers.push(layer);
        MergedConfig { layers: new_layers }
    }

    /// Get a cursor at the root
    pub fn cursor(&self) -> MergedCursor<'_> {
        MergedCursor { config: self, path: Vec::new() }
    }

    // Convenience methods that delegate to cursor
    pub fn get_scalar(&self, path: &[&str]) -> Option<MergedScalar<'a>> {
        self.cursor().at_path(path).as_scalar()
    }

    pub fn get_array(&self, path: &[&str]) -> Option<MergedArray<'a>> {
        self.cursor().at_path(path).as_array()
    }

    pub fn get_map(&self, path: &[&str]) -> Option<MergedMap<'a>> {
        self.cursor().at_path(path).as_map()
    }

    pub fn contains(&self, path: &[&str]) -> bool {
        self.cursor().at_path(path).exists()
    }
}
```

### Cursor Navigation and Resolution

The cursor provides both path-based and chainable navigation:

```rust
impl<'a> MergedCursor<'a> {
    /// Navigate to a child key
    pub fn at(&self, key: &str) -> MergedCursor<'a> {
        let mut path = self.path.clone();
        path.push(key.to_string());
        MergedCursor { config: self.config, path }
    }

    /// Navigate to a path (multiple keys at once)
    pub fn at_path(&self, path: &[&str]) -> MergedCursor<'a> {
        let mut new_path = self.path.clone();
        new_path.extend(path.iter().map(|s| s.to_string()));
        MergedCursor { config: self.config, path: new_path }
    }

    /// Check if this path exists in any layer
    pub fn exists(&self) -> bool {
        self.config.layers.iter().any(|layer| self.navigate_to(layer).is_some())
    }

    /// Get child keys at this path (union across layers, respecting merge semantics)
    pub fn keys(&self) -> Vec<String> { ... }

    /// Resolve as any value type (for when type is not known ahead of time)
    pub fn as_value(&self) -> Option<MergedValue<'a>> { ... }

    /// Resolve as scalar (last-wins semantics)
    /// Scalars, PandocInlines, and PandocBlocks all default to !prefer (last wins)
    pub fn as_scalar(&self) -> Option<MergedScalar<'a>> {
        // Walk layers in reverse (highest priority first)
        for (i, layer) in self.config.layers.iter().enumerate().rev() {
            if let Some(value) = self.navigate_to(layer) {
                if matches!(value.value, ConfigValueKind::Scalar(_)
                                       | ConfigValueKind::PandocInlines(_)
                                       | ConfigValueKind::PandocBlocks(_)) {
                    return Some(MergedScalar { value, layer_index: i });
                }
            }
        }
        None
    }

    /// Resolve as array (applying prefer/concat semantics)
    pub fn as_array(&self) -> Option<MergedArray<'a>> {
        let mut items: Vec<MergedArrayItem<'a>> = Vec::new();

        for (i, layer) in self.config.layers.iter().enumerate() {
            if let Some(value) = self.navigate_to(layer) {
                match value.merge_op {
                    MergeOp::Prefer => items.clear(),
                    MergeOp::Concat => { /* keep existing */ }
                }
                if let ConfigValueKind::Array(arr) = &value.value {
                    for item in arr {
                        items.push(MergedArrayItem { value: item, layer_index: i });
                    }
                }
            }
        }

        if items.is_empty() { None } else { Some(MergedArray { items }) }
    }

    /// Resolve as map (applying prefer/concat semantics)
    pub fn as_map(&self) -> Option<MergedMap<'a>> { ... }

    // Internal helper: navigate to path within a single layer
    fn navigate_to(&self, root: &'a ConfigValue) -> Option<&'a ConfigValue> {
        let mut current = root;
        for key in &self.path {
            match &current.value {
                ConfigValueKind::Map(map) => current = map.get(key)?,
                _ => return None,
            }
        }
        Some(current)
    }
}
```

### Usage Examples

```rust
// ConfigValues exist somewhere (e.g., parsed from YAML files)
let project_config: ConfigValue = parse_config("_quarto.yml");
let doc_config: ConfigValue = parse_config("document.qmd");

// Create merged config by borrowing - zero copy!
let merged = MergedConfig::new(vec![&project_config, &doc_config]);

// Direct path access
if let Some(theme) = merged.get_scalar(&["format", "html", "theme"]) {
    println!("Theme: {:?}", theme.value);
}

// Cursor-based navigation (chainable)
let cursor = merged.cursor();
let theme = cursor.at("format").at("html").at("theme").as_scalar();

// Hybrid: reuse common prefix
let format = merged.cursor().at("format");
let html_theme = format.at("html").at("theme").as_scalar();
let pdf_class = format.at("pdf").at("documentclass").as_scalar();

// Generic traversal with as_value()
match cursor.at("metadata").as_value() {
    Some(MergedValue::Scalar(s)) => println!("scalar: {:?}", s.value),
    Some(MergedValue::Array(a)) => println!("array: {} items", a.items.len()),
    Some(MergedValue::Map(m)) => println!("map: {:?}", m.keys()),
    None => println!("not defined"),
}
```

### Handling Nested Objects

For objects, merge_op applies to the entire object:

```rust
// With Prefer: entire object replaces
obj1: {foo: 1, bar: 2}
obj2: !prefer {baz: 3}
// Result: {baz: 3}  -- foo and bar are GONE

// With Concat (default): field-wise merge
obj1: {foo: 1, bar: 2}
obj2: {baz: 3, foo: 10}
// Result: {foo: 10, bar: 2, baz: 3}
```

This matches the behavior from `composable-validation`:

```rust
impl MergedConfig<'_> {
    fn resolve_map_at_path(&self, path: &[&str]) -> MergedMap<'_> {
        let mut result_fields: IndexMap<String, Vec<(&ConfigValue, MergeOp)>> = IndexMap::new();

        for layer in &self.layers {
            if let Some(value) = self.navigate_to(layer, path) {
                match value.merge_op {
                    MergeOp::Prefer => {
                        // Reset ALL fields
                        result_fields.clear();
                    }
                    MergeOp::Concat => {
                        // Keep existing fields
                    }
                }

                if let ConfigValueKind::Map(map) = &value.value {
                    for (key, child_value) in map {
                        result_fields
                            .entry(key.clone())
                            .or_default()
                            .push((child_value, child_value.merge_op));
                    }
                }
            }
        }

        MergedMap { fields: result_fields }
    }
}
```

### Conversion from YamlWithSourceInfo

```rust
impl From<YamlWithSourceInfo> for ConfigValue {
    fn from(yaml: YamlWithSourceInfo) -> Self {
        // Extract merge_op from tag
        let merge_op = match &yaml.tag {
            Some((tag, _)) if tag == "prefer" => MergeOp::Prefer,
            Some((tag, _)) if tag == "concat" => MergeOp::Concat,
            _ => MergeOp::Concat, // Default behavior
        };

        let kind = match yaml.yaml {
            Yaml::Array(_) => {
                let children = yaml.into_array().unwrap().0;
                ConfigValueKind::Array(
                    children.into_iter().map(ConfigValue::from).collect()
                )
            }
            Yaml::Hash(_) => {
                let entries = yaml.into_hash().unwrap().0;
                let map = entries.into_iter()
                    .filter_map(|entry| {
                        entry.key.yaml.as_str().map(|key| {
                            (key.to_string(), ConfigValue::from(entry.value))
                        })
                    })
                    .collect();
                ConfigValueKind::Map(map)
            }
            scalar => ConfigValueKind::Scalar(scalar),
        };

        ConfigValue {
            value: kind,
            source_info: yaml.source_info,
            merge_op,
        }
    }
}
```

### Materialization and Serialization

For cases where owned data is needed (serialization, caching, cross-thread use):

```rust
/// Options for materialization
pub struct MaterializeOptions {
    /// Maximum nesting depth (default: 256)
    pub max_depth: usize,
}

impl Default for MaterializeOptions {
    fn default() -> Self {
        Self { max_depth: 256 }
    }
}

impl<'a> MergedConfig<'a> {
    /// Materialize with default options
    pub fn materialize(&self) -> Result<ConfigValue, ConfigError> {
        self.materialize_with_options(&MaterializeOptions::default())
    }

    /// Materialize with custom options
    pub fn materialize_with_options(
        &self,
        options: &MaterializeOptions,
    ) -> Result<ConfigValue, ConfigError> {
        self.cursor().materialize_with_depth(0, options)
    }
}

impl<'a> MergedCursor<'a> {
    /// Materialize this cursor's value into an owned ConfigValue
    fn materialize_with_depth(
        &self,
        depth: usize,
        options: &MaterializeOptions,
    ) -> Result<ConfigValue, ConfigError> {
        if depth > options.max_depth {
            return Err(ConfigError::NestingTooDeep {
                max_depth: options.max_depth,
                path: self.path.clone(),
            });
        }

        // Resolution logic: scalars, arrays, maps...
        // Recursive calls pass depth + 1
    }
}
```

**Serialization Strategy**: `MergedConfig<'a>` contains borrowed references and cannot be directly serialized. Instead:

1. **Materialize first**: Call `merged.materialize()` to get an owned `ConfigValue`
2. **Serialize the owned tree**: `ConfigValue` can implement `Serialize`
3. **Deserialize and re-merge**: The deserialized `ConfigValue` can be used as a layer in new merges

**What's preserved**: Each value's `SourceInfo` survives materialization—validation errors will still point to the correct file and line.

**What's lost**: Layer indices (which layer a value came from), but `SourceInfo` provides the real provenance.

## YAML Tag Convention

### Problem: YAML Only Allows One Tag Per Node

YAML 1.2 spec (section 3.2.1.2) only allows one tag per value. But we may need both:
- **Merge semantics**: `!prefer` or `!concat`
- **Interpretation hints**: `!md`, `!str`, `!path`, `!glob`, `!expr`

### Solution: Underscore-Separated Tag Components

Use a single YAML tag with underscore-separated components:

```yaml
# Single component (most common)
title: !md "This is **strong**"
keywords: !prefer [one, two]

# Multiple components when needed
abstract: !prefer_md "This **overrides** completely"
paths: !concat_path ["./a", "./b"]
```

### Parsing Rules

1. Split the tag suffix by underscore: `"prefer_md"` → `["prefer", "md"]`
2. Categorize each component:
   - **Merge tags**: `prefer`, `concat`
   - **Interpretation tags**: `md`, `str`, `path`, `glob`, `expr`
3. Apply defaults for missing categories:
   - No merge tag → `Concat` (default)
   - No interpretation tag → context-dependent (markdown in .qmd, plain in .yml)

### Tag Reference

| Tag Component | Category | Meaning |
|---------------|----------|---------|
| `prefer` | Merge | Reset/override previous values |
| `concat` | Merge | Append to previous values (explicit default) |
| `md` | Interpretation | Parse string as markdown |
| `str` | Interpretation | Keep as literal string |
| `path` | Interpretation | Resolve relative to source file |
| `glob` | Interpretation | Treat as glob pattern |
| `expr` | Interpretation | Runtime expression (R/Python/Julia) |

### Examples

```yaml
# Override with markdown-interpreted value
title: !prefer_md "**New** Title"

# Concatenate paths (relative to source)
resources: !concat_path ["./images", "./data"]

# Override with plain string (no markdown parsing)
raw_html: !prefer_str "<div>literal</div>"

# Single tags (most common usage)
description: !md "Some **bold** text"
files: !prefer ["only", "these"]
```

### Why Underscore?

- Fully supported by YAML parsers (unlike comma which fails)
- Common in programming (snake_case)
- Visually separates components
- Easy to parse: `tag.split('_')`

**Tested separators**:
| Separator | Example | YAML Parser Support |
|-----------|---------|---------------------|
| Underscore | `!prefer_md` | ✓ Works |
| Dash | `!prefer-md` | ✓ Works |
| Dot | `!prefer.md` | ✓ Works |
| Colon | `!prefer:md` | ✓ Works |
| Comma | `!prefer,md` | ✗ Fails |
| Bang | `!md!prefer` | ✗ Fails |

We chose underscore as it's the most readable and common in code conventions.

## Markdown Value Handling

### Context-Dependent Defaults

In `.qmd` files, untagged strings are interpreted as Markdown:
```yaml
title: This is **strong**
```

In `_quarto.yml`, untagged strings are plain:
```yaml
title: This is **not bold**
```

### Explicit Control

Use tags to override the default:

| Situation | Tag | Result |
|-----------|-----|--------|
| Want markdown in _quarto.yml | `!md` | Parsed as markdown |
| Want plain string in .qmd | `!str` | Kept as literal |
| Want to override AND markdown | `!prefer_md` | Reset + parse as markdown |

### Implementation

The `interpretation` field in `ConfigValue` stores the hint. Actual parsing happens in pampa when converting `MergedConfig` → `MetaValueWithSourceInfo`.

## Design Alternatives Considered

### Alternative 1: Pure Eager Merge (Previous Recommendation)

Eagerly create a new `YamlWithSourceInfo` tree for every merge.

**Pros:**
- Simple mental model
- Fast access after merge

**Cons:**
- ~15% runtime cost (cloning/copying)
- Memory pressure from duplicated trees
- Doesn't preserve preference tags through multi-layer merge

**Verdict**: Not recommended due to performance and associativity issues.

### Alternative 2: Pure Lazy Evaluation with Index Maps

Never merge trees; always navigate through all layers at access time.

**Pros:**
- Zero merge cost
- Perfect source tracking

**Cons:**
- Slower access (must check all layers)
- Complex lifetime management for nested navigation
- Memory layout unfriendly (pointer chasing)

**Verdict**: Too slow for frequent access patterns.

### Alternative 3: Hybrid (Recommended)

Lazy merge structure with on-demand materialization:
- `MergedConfig<'a>` for lazy composition
- `ConfigValue` for materialized results
- Cache materialized subtrees as needed

**Pros:**
- Fast composition (no copying)
- Fast access (materialize hot paths)
- Perfect source tracking
- Proper associativity

**Cons:**
- More complex implementation
- Need to decide when to materialize

**Verdict**: Recommended approach.

## Integration Points

### 1. YAML Parsing (quarto-yaml)

Extend parser to recognize underscore-separated tag components with comprehensive error handling:

```rust
// In quarto-yaml/src/parser.rs

/// Parsed tag information
#[derive(Debug, Clone, Default)]
pub struct ParsedTag {
    pub merge_op: Option<MergeOp>,
    pub interpretation: Option<Interpretation>,
    /// True if any errors occurred (not just warnings)
    pub had_errors: bool,
}

/// Parse a YAML tag suffix with underscore-separated components
/// Examples: "prefer", "md", "prefer_md", "concat_path"
fn parse_tag(
    tag_str: &str,
    tag_source: &SourceInfo,
    diagnostics: &mut DiagnosticCollector,
) -> ParsedTag {
    let mut result = ParsedTag::default();

    // Check for invalid characters (only alphanumeric and underscore allowed)
    if tag_str.contains(|c: char| !c.is_alphanumeric() && c != '_') {
        diagnostics.error_at(
            format!("Invalid character in tag '!{}' (Q-1-26)", tag_str),
            tag_source.clone(),
        );
        result.had_errors = true;
        return result;
    }

    for component in tag_str.split('_') {
        // Empty component check (Q-1-24)
        if component.is_empty() {
            diagnostics.error_at(
                format!("Empty component in tag '!{}' (Q-1-24)", tag_str),
                tag_source.clone(),
            );
            result.had_errors = true;
            return result;
        }

        // Whitespace check (Q-1-25)
        if component != component.trim() {
            diagnostics.error_at(
                format!("Whitespace in tag component '!{}' (Q-1-25)", tag_str),
                tag_source.clone(),
            );
            result.had_errors = true;
            return result;
        }

        match component {
            "prefer" => {
                if result.merge_op.is_some() {
                    diagnostics.error_at(
                        format!("Conflicting merge operations in tag '!{}' (Q-1-28)", tag_str),
                        tag_source.clone(),
                    );
                    result.had_errors = true;
                    return result;
                }
                result.merge_op = Some(MergeOp::Prefer);
            }
            "concat" => {
                if result.merge_op.is_some() {
                    diagnostics.error_at(
                        format!("Conflicting merge operations in tag '!{}' (Q-1-28)", tag_str),
                        tag_source.clone(),
                    );
                    result.had_errors = true;
                    return result;
                }
                result.merge_op = Some(MergeOp::Concat);
            }
            "md" => result.interpretation = Some(Interpretation::Markdown),
            "str" => result.interpretation = Some(Interpretation::PlainString),
            "path" => result.interpretation = Some(Interpretation::Path),
            "glob" => result.interpretation = Some(Interpretation::Glob),
            "expr" => result.interpretation = Some(Interpretation::Expr),

            // Unknown components emit warnings (Q-1-21), not errors
            unknown => {
                diagnostics.warn_at(
                    format!("Unknown tag component '{}' in '!{}' (Q-1-21)", unknown, tag_str),
                    tag_source.clone(),
                );
            }
        }
    }

    result
}
```

### Error Handling for Layer Parsing

When merging config layers, use "collect errors, then decide" pattern:

```rust
impl<'a> MergedConfig<'a> {
    /// Merge config layers, collecting diagnostics
    ///
    /// Returns Some if all layers parsed successfully, None if any failed.
    /// All errors/warnings are collected in diagnostics either way.
    pub fn merge_with_diagnostics(
        layers: Vec<(&'a ConfigValue, &SourceInfo)>,
        diagnostics: &mut DiagnosticCollector,
    ) -> Option<MergedConfig<'a>> {
        let mut valid_layers = Vec::new();
        let mut had_errors = false;

        for (layer, source) in layers {
            // Validate the layer (tag parsing, etc.)
            if let Err(e) = validate_layer(layer, diagnostics) {
                diagnostics.error_at(
                    format!("Config layer parse failure (Q-1-23): {}", e),
                    source.clone(),
                );
                had_errors = true;
            } else {
                valid_layers.push(layer);
            }
        }

        if had_errors {
            None  // Errors already in diagnostics
        } else {
            Some(MergedConfig::new(valid_layers))
        }
    }
}
```

### 2. Metadata Conversion (pampa)

Convert `ConfigValue` to `MetaValueWithSourceInfo`:

```rust
// In pampa/src/pandoc/meta.rs
pub fn config_to_meta(
    config: &MergedConfig,
    context: &ASTContext,
    diagnostics: &mut DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    // Navigate merged config and convert to Meta
    // Apply interpretation tags when converting strings
}
```

### 3. Validation (quarto-yaml-validation)

Validate against merged config while preserving source locations:

```rust
pub fn validate_config(
    merged: &MergedConfig,
    schema: &Schema,
) -> Vec<ValidationError> {
    // Each error includes SourceInfo pointing to original file
}
```

## Implementation Plan

### Phase 1: Create `quarto-config` crate

- [ ] Create crate skeleton with Cargo.toml (deps: quarto-yaml, quarto-source-map, quarto-pandoc-types, quarto-error-reporting)
- [ ] Define `MergeOp` enum (Prefer, Concat)
- [ ] Define `Interpretation` enum (Markdown, PlainString, Path, Glob, Expr)
- [ ] Define `ConfigValueKind` enum (Scalar, Array, Map, PandocInlines, PandocBlocks)
- [ ] Define `ConfigValue` struct
- [ ] Implement underscore-separated tag parsing with full error handling:
  - Q-1-21: Unknown tag component (warning)
  - Q-1-22: Unrecognized tag (warning)
  - Q-1-24: Empty tag component (error)
  - Q-1-25: Whitespace in tag (error)
  - Q-1-26: Invalid tag character (error)
  - Q-1-28: Conflicting merge operations (error)
- [ ] Implement `From<YamlWithSourceInfo> for ConfigValue`
- [ ] Implement `From<MetaValueWithSourceInfo> for ConfigValue` (for already-interpreted values)
- [ ] Add error codes Q-1-23 through Q-1-28 to `error_catalog.json`
- [ ] Unit tests for basic construction and tag extraction

### Phase 2: Cursor-Based Navigation

- [ ] Define `MergedConfig<'a>` with layer storage (borrowed references)
- [ ] Define `MergedCursor<'a>` with path-based navigation
- [ ] Define `MergedValue<'a>`, `MergedScalar<'a>`, `MergedArray<'a>`, `MergedMap<'a>`
- [ ] Implement cursor navigation: `at()`, `at_path()`, `exists()`, `keys()`
- [ ] Implement resolution: `as_value()`, `as_scalar()`, `as_array()`, `as_map()`
- [ ] Implement scalar resolution (last wins for Scalar, PandocInlines, PandocBlocks—all default to !prefer)
- [ ] Implement array resolution with `!prefer`/`!concat` semantics
- [ ] Implement map resolution with `!prefer`/`!concat` semantics
- [ ] Implement `MergedMap::iter()` for key-cursor iteration
- [ ] Unit tests for all merge scenarios from composable-validation
- [ ] Property-based tests for associativity

### Phase 3: Materialization and Error Handling

- [ ] Define `MaterializeOptions` with `max_depth` (default: 256)
- [ ] Implement `materialize()` and `materialize_with_options()`
- [ ] Implement depth-limited recursive materialization
- [ ] Add Q-1-27 error for nesting too deep
- [ ] Implement `merge_with_diagnostics()` for layer validation
- [ ] Add Q-1-23 error for layer parse failures
- [ ] Tests for depth limit enforcement
- [ ] Tests for error collection pattern

### Phase 4: quarto-yaml Integration

- [ ] Extend tag parsing to recognize `!prefer` and `!concat`
- [ ] Ensure tags flow through to `YamlWithSourceInfo.tag` field
- [ ] Integrate error handling with DiagnosticCollector
- [ ] Tests for tag parsing in various contexts

### Phase 5: pampa Integration

- [ ] Create `pampa/src/config_meta.rs` for conversion layer
- [ ] Implement `config_to_meta()` function
- [ ] Handle `Interpretation::Markdown` with existing parser
- [ ] Handle `Interpretation::PlainString` bypass
- [ ] Update document rendering to use `MergedConfig`
- [ ] Integration tests with real .qmd files and _quarto.yml

### Phase 6: Performance & Polish

- [ ] Benchmark against TypeScript mergeConfigs
- [ ] Profile and optimize hot paths
- [ ] Consider caching for frequently-accessed subtrees (if needed)
- [ ] Documentation with examples

## Design Decisions (Confirmed)

### D1: `!prefer` on objects does complete reset

**Decision**: When `!prefer` is applied to an object, it resets ALL nested structure.

**Syntax note**: YAML tags attach to *values*, not keys. The `!prefer` tag goes after the colon:

**Example**:
```yaml
# Layer 1
format:
  html:
    toc: true
    theme: cosmo
  pdf:
    documentclass: article

# Layer 2: !prefer on the format value resets ALL of format
format: !prefer
  html:
    theme: journal

# Result: format is ONLY {html: {theme: journal}}
# Both format.html.toc and format.pdf are GONE
```

**Another example** (prefer on nested value):
```yaml
# Layer 1
format:
  html:
    toc: true
    theme: cosmo

# Layer 2: !prefer only on html, not on format
format:
  html: !prefer
    theme: journal

# Result: format.html is ONLY {theme: journal}, toc is GONE
# But format.pdf (if it existed) would be preserved
```

**Rationale**: If you write `format: !prefer {...}`, you want ONLY what's in the braces, discarding everything that was there before.

### D2: Type mismatches are silent override

**Decision**: When merging layers with different types, silently use the later value.

**Example**:
```yaml
# Layer 1
foo: [1, 2, 3]

# Layer 2
foo: "string now"

# Result: foo is "string now" (no warning from merge layer)
```

**Rationale**: Type validation is handled separately in the YAML validation layer (`quarto-yaml-validation`). The merge layer should be permissive to allow flexible configuration evolution.

**Note on error reporting**: Because merged values preserve their original `SourceInfo`, if this type change causes a validation error, the error message will correctly point to `Layer 2` where `foo: "string now"` was defined. The merge layer doesn't need to warn because validation will catch any problems with full source location information.

### D3: No `!prefer` on individual array elements

`!prefer` only makes sense at the array level, not on individual elements.

```yaml
# NOT supported:
authors:
  - Alice
  - !prefer Bob  # What would this even mean?

# Supported:
authors: !prefer
  - Bob
```

### D3.5: Underscore-separated tag components

**Decision**: Since YAML only allows one tag per node, use underscore-separated components within a single tag to express multiple dimensions.

**Format**: `!component1_component2` (e.g., `!prefer_md`)

**Parsing**:
1. Split by underscore
2. Merge tags: `prefer`, `concat` → sets `MergeOp`
3. Interpretation tags: `md`, `str`, `path`, `glob`, `expr` → sets `Interpretation`
4. **Unrecognized components emit warnings** (see D5 below)

**Examples**:
```yaml
value: !prefer_md "**override** with markdown"
paths: !concat_path ["./a", "./b"]
```

**Rationale**: Underscore is fully supported by YAML parsers (unlike comma which fails). Future-proofs the design for new tag dimensions.

### D5: Unknown tag components emit warnings

**Decision**: When parsing tag components, unknown components should emit warnings with source location pointing to the tag.

**Error codes** (from `quarto-error-reporting`):

| Code | Subsystem | Description |
|------|-----------|-------------|
| Q-1-21 | yaml | Unknown YAML tag component |
| Q-1-22 | yaml | Completely unrecognized YAML tag |

**Example diagnostics**:

```
Warning [Q-1-21]: Unknown tag component
  at document.qmd:5:8

  5 | title: !prefre,md "Hello"
           ^^^^^^^

  Unknown component 'prefre' in tag '!prefre,md'
  ℹ Did you mean 'prefer'?
```

```
Warning [Q-1-22]: Unknown YAML tag
  at _quarto.yml:10:12

  10 | theme: !custom "dark"
              ^^^^^^^

  Unrecognized tag '!custom'
  ℹ Valid tags are: prefer, concat, md, str, path, glob, expr
```

**Rationale**:
- Typos like `!prefre` instead of `!prefer` should be caught early
- Source location tracking enables precise error messages
- Warnings (not errors) allow forward compatibility—future Quarto versions may add new tags

**Implementation**: These error codes should be added to `error_catalog.json` in `quarto-error-reporting`.

### D3.6: ConfigValueKind includes Pandoc AST types

**Decision**: `ConfigValueKind` includes `PandocInlines(Vec<Inline>)` and `PandocBlocks(Vec<Block>)` variants.

**Rationale**: Allows merging at any stage of the pipeline:
- Raw YAML from parsing
- Already-interpreted Pandoc AST from filters or prior processing

**Merge semantics**:
- `PandocInlines`: Default `!prefer` (last wins), use `!concat` explicitly if needed
- `PandocBlocks`: Default `!prefer` (last wins), use `!concat` explicitly if needed

**Implication**: `quarto-config` depends on `quarto-pandoc-types`.

### D6: Source tracking for merged containers

**Decision**: Merged arrays and objects need special `SourceInfo` handling since they don't exist in any single source file.

**For individual elements/fields**: Each element retains its original `SourceInfo` from the layer it came from. This is the primary mechanism for error reporting.

**For the container itself**: The merged array or object needs a `SourceInfo` that indicates it was synthesized from multiple sources. Two options:

1. **Use `SourceInfo::Concat`**: Combine the `SourceInfo` from all contributing layers:
   ```rust
   let merged_source = SourceInfo::concat(vec![
       (layer1.source_info.clone(), layer1.source_info.length()),
       (layer2.source_info.clone(), layer2.source_info.length()),
   ]);
   ```

2. **New `SourceInfo::MergeProvenance` variant** (similar to `FilterProvenance`):
   ```rust
   // Potential future addition to quarto-source-map
   MergeProvenance {
       layers: Vec<SourceInfo>,  // Sources that contributed
   }
   ```

**Initial recommendation**: Use `SourceInfo::Concat` for now. It already exists and supports combining multiple sources. A dedicated `MergeProvenance` variant can be added later if needed.

**Validation note**: When validating a merged container, errors typically point to specific *fields* or *elements* within it, not the container itself. Since fields/elements retain their original `SourceInfo`, validation error messages will be accurate. The container's merged `SourceInfo` is mainly useful for debugging and introspection.

### D4: Crate organization - new `quarto-config` crate

**Decision**: Create a new `quarto-config` crate for configuration merging.

**Key insight**: Markdown interpretation doesn't happen during merge - it happens *after* merge, when converting to Pandoc metadata. The `Interpretation` enum (`!md`, `!str`) is just metadata that travels with values; actual parsing happens in pampa.

**Architecture**:
```
quarto-source-map ─────────────────────────────────┐
        ↓                                          │
quarto-pandoc-types ←──────────────────────────────┤
        ↓                                          │
quarto-yaml ←──────────────────────────────────────┤
        ↓                                          │
quarto-config (NEW) ←──────────────────────────────┘
        ↓               (deps: source-map, yaml, pandoc-types)
      pampa
```

Key: `quarto-config` depends on:
- `quarto-source-map` for `SourceInfo`
- `quarto-yaml` for `YamlWithSourceInfo`
- `quarto-pandoc-types` for `Inline`, `Block` (in `ConfigValueKind::PandocInlines/PandocBlocks`)

**Rationale**:
1. **Pure merge semantics**: `quarto-config` is about "how values combine," not "what values mean"
2. **Testability**: Can test merge edge cases without markdown parsing complexity
3. **Parallel structure**: Matches existing pattern (`quarto-pandoc-types` for types, `quarto-yaml` for parsing)
4. **Reusability**: LSP can use config merging without importing all of pampa
5. **Precedent**: Pandoc AST types were lifted to their own crate for the same reasons
6. **Mixed-stage merging**: Can merge raw YAML with already-interpreted Pandoc AST values

**Crate structure**:
```
crates/quarto-config/
  Cargo.toml          # deps: quarto-yaml, quarto-source-map
  src/
    lib.rs
    types.rs          # ConfigValue, MergeOp, ConfigValueKind, Interpretation
    merge.rs          # MergedConfig<'a>, lazy resolution logic
    convert.rs        # From<YamlWithSourceInfo> for ConfigValue
```

**pampa integration**:
```rust
// pampa/src/config_meta.rs - the conversion layer
pub fn config_to_meta(
    merged: &MergedConfig,
    ctx: &ASTContext,
    diagnostics: &mut DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    // Apply Interpretation hints here, parsing markdown as needed
}
```

This keeps `quarto-config` pure and simple, while pampa handles the markdown complexity at the appropriate layer.

## Relationship to Previous Work

This design builds on:

1. **config-merging-analysis.md**: Strategy 4 (AnnotatedParse merge) is the eager version of this design
2. **composable-validation**: The `TaggedJson` and `ValueOp` concepts are directly adapted
3. **session-log-2025-10-11.md**: The MergeCustomizer trait concept is replaced by tags

**Terminology note**: The earlier analysis used the term "AnnotatedParse" for a source-tracked YAML tree. In the actual implementation, this is called `YamlWithSourceInfo` (in `quarto-yaml`). The proposed `ConfigValue` type is a new struct that builds on `YamlWithSourceInfo`, adding explicit merge semantics (`MergeOp`) and interpretation hints (`Interpretation`).

Key differences from previous analysis:
- Tags instead of customizer traits for merge behavior
- Lazy evaluation by default instead of eager
- Explicit handling of markdown interpretation
- `ConfigValue` as a new type (not just extending `YamlWithSourceInfo`) to support Pandoc AST values directly

## Resolved Questions (from subissue documents)

### RQ1: MergedConfig lifetime design (was OQ1, subissue: k-vpgx)

**Resolution**: Cursor-based navigation design with borrowed references.

- `MergedConfig<'a>` stores `Vec<&'a ConfigValue>` (zero-copy construction)
- `MergedCursor<'a>` provides chainable navigation via `.at(key)` and `.at_path()`
- `MergedValue<'a>` enum allows type-agnostic access via `cursor.as_value()`
- Resolution happens lazily at `as_scalar()`, `as_array()`, `as_map()` call sites
- Materialization via `materialize()` produces owned `ConfigValue` for serialization

### RQ2: Error handling strategy (was OQ2, subissue: k-os6h)

**Resolution**: Comprehensive error handling with specific error codes.

| Code | Description | Severity |
|------|-------------|----------|
| Q-1-21 | Unknown tag component | Warning |
| Q-1-22 | Unrecognized tag | Warning |
| Q-1-23 | Config layer parse failure | Error |
| Q-1-24 | Empty tag component | Error |
| Q-1-25 | Whitespace in tag | Error |
| Q-1-26 | Invalid tag character | Error |
| Q-1-27 | Config nesting too deep | Error |
| Q-1-28 | Conflicting merge operations | Error |

- **Layer parsing**: "Collect errors, then decide" pattern—all errors shown, None returned if any
- **Circular includes**: Out of scope (project layer concern)
- **Depth limit**: 256 levels default, enforced during materialization

## Open Questions

### OQ3: Caching strategy for MergedConfig

If the same config layers are merged repeatedly (e.g., during batch rendering), can we cache results?

**Concerns**:
- Implementation complexity with lifetimes
- Cache invalidation when source files change
- Memory pressure from cached trees

**Potential sketch**:
```rust
/// A cache key based on layer identities
#[derive(Hash, Eq, PartialEq)]
struct MergeCacheKey {
    layer_ids: Vec<FileId>,  // or some stable identifier
}

/// Cached merge results (owned, not borrowed)
struct MergeCache {
    results: HashMap<MergeCacheKey, ConfigValue>,
}
```

This would require `materialize()` to produce owned `ConfigValue` trees that can outlive the original `MergedConfig<'a>`. The cache would be optional and opt-in.

**Recommendation**: Defer this until we have performance benchmarks. Lazy evaluation may be fast enough without caching.

### OQ4: Testing strategy

Need to decide on the testing approach for the new `quarto-config` crate:

1. **Unit tests**: Test merge scenarios in isolation (already planned)
2. **Property-based tests**: Test associativity property with `proptest` (already planned)
3. **Integration tests**: How do we test the full pipeline (YAML → ConfigValue → MergedConfig → MetaValue)?
4. **Snapshot tests**: Should we use snapshot testing for merge results?
5. **Compatibility tests**: How do we verify behavior matches TypeScript `mergeConfigs()`?

## Conclusion

The proposed design addresses all requirements:

1. **Source location preservation**: Every `ConfigValue` carries `SourceInfo`
2. **Explicit merge semantics**: `!prefer` and `!concat` tags
3. **Performance**: Lazy evaluation avoids unnecessary copying
4. **Associativity**: Tags preserved through composition
5. **Integration**: Natural fit with existing infrastructure

The implementation can proceed incrementally, starting with the core types and gradually adding lazy evaluation and optimization.
