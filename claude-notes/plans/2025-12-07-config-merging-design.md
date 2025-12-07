# Configuration Merging System Design

**Date**: 2025-12-07
**Issue**: k-zvzm
**Status**: Design proposal

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
    /// Treated like scalars: last wins, no concatenation
    PandocInlines(Vec<Inline>),

    /// Pandoc block content (for already-interpreted values)
    /// Treated like arrays: concatenate by default, !prefer to replace
    PandocBlocks(Vec<Block>),
}
```

### Construction: `MergedConfig`

For lazy evaluation, we don't immediately merge into a new tree. Instead, we keep references:

```rust
/// A lazily-evaluated merged configuration
///
/// This maintains references to original config layers without copying.
/// Values are resolved on demand at the "navigation site".
pub struct MergedConfig<'a> {
    /// Ordered list of config layers (first = lowest priority)
    layers: Vec<&'a ConfigValue>,
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
}
```

### Interpretation: Lazy Resolution

The key innovation is resolving values lazily at navigation sites:

```rust
impl<'a> MergedConfig<'a> {
    /// Get a scalar value at a path
    /// Returns the value and its source info for error reporting
    pub fn get_scalar(&self, path: &[&str]) -> Option<(&Yaml, &SourceInfo)> {
        // Walk path in reverse (highest priority first)
        for layer in self.layers.iter().rev() {
            if let Some((value, source)) = self.resolve_path(layer, path) {
                return Some((value, source));
            }
        }
        None
    }

    /// Get an array value at a path, applying merge semantics
    pub fn get_array(&self, path: &[&str]) -> Option<MergedArray<'a>> {
        let mut items: Vec<(&ConfigValue, &SourceInfo)> = Vec::new();

        // Walk layers in order (lowest priority first)
        for layer in &self.layers {
            if let Some(value) = self.resolve_path_to_value(layer, path) {
                match value.merge_op {
                    MergeOp::Prefer => {
                        // Reset: discard all previous items
                        items.clear();
                    }
                    MergeOp::Concat => {
                        // Concatenate: add to existing items
                    }
                }

                if let ConfigValueKind::Array(arr) = &value.value {
                    for item in arr {
                        items.push((item, &value.source_info));
                    }
                }
            }
        }

        if items.is_empty() {
            None
        } else {
            Some(MergedArray { items })
        }
    }

    /// Get a map value at a path, applying merge semantics
    pub fn get_map(&self, path: &[&str]) -> Option<MergedMap<'a>> {
        // Similar logic: Prefer resets, Concat merges field-wise
        // Field-wise merge uses recursive resolution
    }
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

### Eager Evaluation Option

For cases where eager evaluation is needed (e.g., serialization, caching):

```rust
impl<'a> MergedConfig<'a> {
    /// Materialize the merged config into a new owned ConfigValue
    /// This performs the full merge eagerly and returns an owned result.
    pub fn materialize(&self) -> ConfigValue {
        // Walk through all paths and resolve each one
        // Build a new ConfigValue tree with resolved values
        // Each leaf node's source_info points to its origin layer
    }
}
```

## YAML Tag Convention

### Problem: YAML Only Allows One Tag Per Node

YAML 1.2 spec (section 3.2.1.2) only allows one tag per value. But we may need both:
- **Merge semantics**: `!prefer` or `!concat`
- **Interpretation hints**: `!md`, `!str`, `!path`, `!glob`, `!expr`

### Solution: Comma-Separated Tag Components

Use a single YAML tag with comma-separated components:

```yaml
# Single component (most common)
title: !md "This is **strong**"
keywords: !prefer [one, two]

# Multiple components when needed
abstract: !prefer,md "This **overrides** completely"
paths: !concat,path ["./a", "./b"]
```

### Parsing Rules

1. Split the tag suffix by comma: `"prefer,md"` → `["prefer", "md"]`
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
title: !prefer,md "**New** Title"

# Concatenate paths (relative to source)
resources: !concat,path ["./images", "./data"]

# Override with plain string (no markdown parsing)
raw_html: !prefer,str "<div>literal</div>"

# Single tags (most common usage)
description: !md "Some **bold** text"
files: !prefer ["only", "these"]
```

### Why Comma?

- Visually clear and readable
- Not used in standard YAML tag names
- Easy to parse: `tag.split(',')`
- Common convention (CSS classes, CLI flags)

Alternatives considered:
- Colon (`:`) - might conflict with YAML syntax
- Dash (`-`) - ambiguous: is `!no-md` two tags or one?
- Underscore (`_`) - less readable: `!prefer_md`

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
| Want to override AND markdown | `!prefer,md` | Reset + parse as markdown |

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

Extend parser to recognize comma-separated tag components:

```rust
// In quarto-yaml/src/parser.rs

/// Parsed tag information
#[derive(Debug, Clone, Default)]
pub struct ParsedTag {
    pub merge_op: Option<MergeOp>,
    pub interpretation: Option<Interpretation>,
}

/// Parse a YAML tag suffix with comma-separated components
/// Examples: "prefer", "md", "prefer,md", "concat,path"
fn parse_tag(
    tag: &yaml_rust2::Tag,
    tag_source: &SourceInfo,
    diagnostics: &mut DiagnosticCollector,
) -> ParsedTag {
    let mut result = ParsedTag::default();

    for component in tag.suffix.split(',') {
        match component.trim() {
            // Merge semantics
            "prefer" => result.merge_op = Some(MergeOp::Prefer),
            "concat" => result.merge_op = Some(MergeOp::Concat),

            // Interpretation hints
            "md" => result.interpretation = Some(Interpretation::Markdown),
            "str" => result.interpretation = Some(Interpretation::PlainString),
            "path" => result.interpretation = Some(Interpretation::Path),
            "glob" => result.interpretation = Some(Interpretation::Glob),
            "expr" => result.interpretation = Some(Interpretation::Expr),

            // Unknown components emit warnings (Q-1-21)
            unknown => {
                diagnostics.warn(
                    DiagnosticMessageBuilder::warning("Unknown tag component")
                        .with_code("Q-1-21")
                        .problem(&format!(
                            "Unknown component '{}' in tag '!{}'",
                            unknown, tag.suffix
                        ))
                        .with_location(tag_source.clone())
                        .build()
                );
            }
        }
    }

    result
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

- [ ] Create crate skeleton with Cargo.toml (deps: quarto-yaml, quarto-source-map, quarto-pandoc-types)
- [ ] Define `MergeOp` enum (Prefer, Concat)
- [ ] Define `Interpretation` enum (Markdown, PlainString, Path, Glob, Expr)
- [ ] Define `ConfigValueKind` enum (Scalar, Array, Map, PandocInlines, PandocBlocks)
- [ ] Define `ConfigValue` struct
- [ ] Implement comma-separated tag parsing (`parse_tag()` function)
- [ ] Implement `From<YamlWithSourceInfo> for ConfigValue`
- [ ] Implement `From<MetaValueWithSourceInfo> for ConfigValue` (for already-interpreted values)
- [ ] Unit tests for basic construction and tag extraction

### Phase 2: Lazy Merge Structure

- [ ] Define `MergedConfig<'a>` with layer storage
- [ ] Define `MergedMap<'a>` and `MergedArray<'a>` for nested navigation
- [ ] Implement scalar resolution (last wins)
- [ ] Implement array resolution with `!prefer`/`!concat` semantics
- [ ] Implement map resolution with `!prefer`/`!concat` semantics
- [ ] Handle nested resolution (recursive field-wise merge)
- [ ] Unit tests for all merge scenarios from composable-validation
- [ ] Property-based tests for associativity

### Phase 3: quarto-yaml Integration

- [ ] Extend tag parsing to recognize `!prefer` and `!concat`
- [ ] Ensure tags flow through to `YamlWithSourceInfo.tag` field
- [ ] Emit warnings for unknown tags (Q-1-21, Q-1-22)
- [ ] Tests for tag parsing in various contexts

### Phase 4: pampa Integration

- [ ] Create `pampa/src/config_meta.rs` for conversion layer
- [ ] Implement `config_to_meta()` function
- [ ] Handle `Interpretation::Markdown` with existing parser
- [ ] Handle `Interpretation::PlainString` bypass
- [ ] Update document rendering to use `MergedConfig`
- [ ] Integration tests with real .qmd files and _quarto.yml

### Phase 5: Performance & Polish

- [ ] Implement `materialize()` for eager evaluation when needed
- [ ] Add caching for frequently-accessed subtrees (see Open Questions)
- [ ] Benchmark against TypeScript mergeConfigs
- [ ] Profile and optimize hot paths
- [ ] Documentation with examples
- [ ] Error messages for invalid tag combinations

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

### D3.5: Comma-separated tag components

**Decision**: Since YAML only allows one tag per node, use comma-separated components within a single tag to express multiple dimensions.

**Format**: `!component1,component2` (e.g., `!prefer,md`)

**Parsing**:
1. Split by comma
2. Merge tags: `prefer`, `concat` → sets `MergeOp`
3. Interpretation tags: `md`, `str`, `path`, `glob`, `expr` → sets `Interpretation`
4. **Unrecognized components emit warnings** (see D5 below)

**Examples**:
```yaml
value: !prefer,md "**override** with markdown"
paths: !concat,path ["./a", "./b"]
```

**Rationale**: Future-proofs the design. If we later need new tag dimensions, the convention is already established.

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
- `PandocInlines`: Treated like scalars (last wins)
- `PandocBlocks`: Treated like arrays (concat by default, `!prefer` to reset)

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

## Open Questions

The following areas need further design work and have dedicated beads subissues:

### OQ1: MergedConfig lifetime design (subissue: k-vpgx)

The `MergedConfig<'a>` lifetime design needs more detail, particularly:

- What does `MergedMap<'a>` look like? How does it support lazy resolution while being navigable?
- How does nested navigation work? E.g., `merged.get("format")?.get("html")?.get("theme")`
- Consider a `MergedValue<'a>` enum that can represent any navigable result
- How do we handle the case where inner navigation returns borrowed vs owned data?

### OQ2: Error handling strategy (subissue: k-os6h)

Need to define behavior for:

- YAML parsing failures in one layer (fail fast? skip layer with warning?)
- Syntactically invalid tags (covered by D5, but what about malformed comma syntax?)
- Circular includes (likely out of scope per Non-Goals, but should document boundary)
- Memory/stack overflow from deeply nested configs

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
