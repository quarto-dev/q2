# MergedConfig Lifetime and Navigation API Design

**Date**: 2025-12-07
**Issue**: k-vpgx (child of k-zvzm)
**Status**: Design proposal
**Parent Plan**: `claude-notes/plans/2025-12-07-config-merging-design.md`

## Problem Statement

The parent design document proposes a `MergedConfig<'a>` type for lazy configuration merging. However, several design details need resolution:

1. How should `MergedConfig<'a>` manage lifetimes for its layers?
2. What types represent intermediate navigation results (`MergedMap<'a>`, `MergedArray<'a>`)?
3. How does chained navigation work? (`merged.get("format")?.get("html")?.get("theme")`)
4. When does resolution return borrowed vs owned data?

This document explores these questions and proposes a concrete design.

## Context: Existing Patterns in This Codebase

### YamlWithSourceInfo: Owned Data, No Lifetimes

From `quarto-yaml/src/yaml_with_source_info.rs`:

```rust
/// Uses the **owned data approach**: stores an owned `Yaml` value with a
/// parallel `Children` structure for source tracking.
///
/// ## Design Trade-offs
///
/// - **Memory**: ~3x overhead (owned Yaml + source-tracked children)
/// - **Simplicity**: No lifetime parameters, clean API
/// - **Config merging**: Can merge configs from different lifetimes
/// - **LSP caching**: Can serialize/deserialize for caching
///
/// Follows rust-analyzer's precedent of using owned data for tree structures.
pub struct YamlWithSourceInfo {
    pub yaml: Yaml,
    pub source_info: SourceInfo,
    pub tag: Option<(String, SourceInfo)>,
    children: Children,
}
```

Key insight: **This codebase explicitly chose owned data over borrowed data** for tree structures, citing rust-analyzer's precedent.

### MetaValueWithSourceInfo: Similar Pattern

From `quarto-pandoc-types/src/meta.rs`:

```rust
impl MetaValueWithSourceInfo {
    /// Get a value by key if this is a MetaMap
    pub fn get(&self, key: &str) -> Option<&MetaValueWithSourceInfo> {
        match self {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                entries.iter().find(|e| e.key == key).map(|e| &e.value)
            }
            _ => None,
        }
    }
}
```

Navigation returns borrowed references into the owned tree.

### TemplateValue: Path-Based Navigation

From `quarto-doctemplate/src/context.rs`:

```rust
impl TemplateValue {
    /// Get a nested field by path.
    pub fn get_path(&self, path: &[&str]) -> Option<&TemplateValue> {
        if path.is_empty() {
            return Some(self);
        }
        match self {
            TemplateValue::Map(m) => {
                let first = path[0];
                m.get(first).and_then(|v| v.get_path(&path[1..]))
            }
            _ => None,
        }
    }
}
```

This provides a path-based alternative to chained `.get()` calls.

## Design Options

### Option A: Borrowed Layers with Lifetime Parameter (Original Sketch)

```rust
pub struct MergedConfig<'a> {
    layers: Vec<&'a ConfigValue>,
}

pub struct MergedMap<'a> {
    fields: IndexMap<String, Vec<(&'a ConfigValue, MergeOp)>>,
}

impl<'a> MergedConfig<'a> {
    pub fn get_map(&self, path: &[&str]) -> Option<MergedMap<'a>> { ... }
}
```

**Pros:**
- Zero-copy construction: just store references
- Memory efficient for temporary views
- Clear ownership semantics
- Honest representation: `MergedConfig` borrows from existing data

**Cons:**
- Lifetime parameter on `MergedConfig<'a>`
- Can't directly serialize (but see "Serialization Strategy" section below)

### Option B: Owned Layers with Rc/Arc

```rust
use std::rc::Rc;

pub struct MergedConfig {
    layers: Vec<Rc<ConfigValue>>,
}
```

**Pros:**
- No lifetime parameter on `MergedConfig` itself

**Cons:**
- **Rc doesn't help with serialization**: Serializing an `Rc` serializes the data inside it. On deserialization, you get a *new* `Rc` pointing to *new* data—the sharing/reference semantics are lost across the serialization boundary.
- **Requires cloning or Rc-wrapping at construction**: You can't create an `Rc<ConfigValue>` from a `&ConfigValue` without cloning. The `ConfigValue` objects already exist in other structures that aren't `Rc`-wrapped, so this would force unnecessary cloning or require restructuring upstream code.
- Some runtime overhead for reference counting

### Option C: Path-Based Resolution (No Intermediate Types)

Instead of returning intermediate `MergedMap`/`MergedArray` types, resolve everything via paths:

```rust
pub struct MergedConfig<'a> {
    layers: Vec<&'a ConfigValue>,
}

impl<'a> MergedConfig<'a> {
    /// Get a scalar at a path (last-wins semantics)
    pub fn get_scalar(&self, path: &[&str]) -> Option<MergedScalar<'a>> { ... }

    /// Get an array at a path (with merge semantics applied)
    pub fn get_array(&self, path: &[&str]) -> Option<MergedArray<'a>> { ... }

    /// Get a map at a path (with merge semantics applied)
    pub fn get_map(&self, path: &[&str]) -> Option<MergedMap<'a>> { ... }

    /// Check if a path exists in any layer
    pub fn contains(&self, path: &[&str]) -> bool { ... }

    /// Get all keys at a path (union across layers)
    pub fn keys_at(&self, path: &[&str]) -> Vec<&str> { ... }
}
```

**Pros:**
- Simple API: all resolution happens through `MergedConfig`
- Clear mental model: path in, resolved value out
- No complex intermediate types needed for most use cases

**Cons:**
- Less flexible: can't easily "browse" a subtree
- Repeated path traversal if accessing many siblings

### Option D: Cursor-Based Navigation (Hybrid) ← RECOMMENDED

Combine path-based resolution with a cursor type for ergonomic chaining:

```rust
pub struct MergedConfig<'a> {
    layers: Vec<&'a ConfigValue>,
}

/// A cursor pointing to a location in the merged config
pub struct MergedCursor<'a> {
    config: &'a MergedConfig<'a>,
    path: Vec<String>,
}

impl<'a> MergedConfig<'a> {
    /// Get a cursor at the root
    pub fn cursor(&self) -> MergedCursor<'_> {
        MergedCursor { config: self, path: Vec::new() }
    }

    /// Direct path-based access (convenience)
    pub fn get_scalar(&self, path: &[&str]) -> Option<MergedScalar<'a>> {
        self.cursor().at_path(path).as_scalar()
    }
}

impl<'a> MergedCursor<'a> {
    /// Navigate to a child key
    pub fn at(&self, key: &str) -> MergedCursor<'a> {
        let mut path = self.path.clone();
        path.push(key.to_string());
        MergedCursor { config: self.config, path }
    }

    /// Navigate to a path
    pub fn at_path(&self, path: &[&str]) -> MergedCursor<'a> {
        let mut new_path = self.path.clone();
        new_path.extend(path.iter().map(|s| s.to_string()));
        MergedCursor { config: self.config, path: new_path }
    }

    /// Resolve as any value type (when type is not known ahead of time)
    pub fn as_value(&self) -> Option<MergedValue<'a>> { ... }

    /// Resolve as scalar (last-wins)
    pub fn as_scalar(&self) -> Option<MergedScalar<'a>> { ... }

    /// Resolve as array (with merge semantics)
    pub fn as_array(&self) -> Option<MergedArray<'a>> { ... }

    /// Resolve as map (with merge semantics)
    pub fn as_map(&self) -> Option<MergedMap<'a>> { ... }

    /// Check if this path exists
    pub fn exists(&self) -> bool { ... }

    /// Get child keys (union across layers)
    pub fn keys(&self) -> Vec<&str> { ... }
}
```

**Usage examples:**

```rust
// Path-based (direct)
let theme = merged.get_scalar(&["format", "html", "theme"]);

// Cursor-based (chainable)
let cursor = merged.cursor();
let theme = cursor.at("format").at("html").at("theme").as_scalar();

// Hybrid
let format = merged.cursor().at("format");
let html_theme = format.at("html").at("theme").as_scalar();
let pdf_class = format.at("pdf").at("documentclass").as_scalar();
```

**Pros:**
- Flexible: both direct and chainable access
- Cursor is lightweight (just path + reference)
- No complex lifetime propagation through intermediate types
- Ergonomic for both single lookups and tree exploration

**Cons:**
- Path cloning on each `.at()` call (but paths are typically short)
- Resolution happens at the leaf, not incrementally

## Recommended Design: Option D (Cursor-Based) with Borrowed References

Option D with `&'a ConfigValue` provides the best balance of ergonomics, simplicity, and correctness:

1. **Borrowed references** - zero-copy construction, no cloning required
2. **Lifetime parameter `'a`** - honest representation that `MergedConfig` borrows from existing data
3. **Lightweight cursor type** - just stores path + reference
4. **Lazy resolution** - work only happens when you call `as_*()` methods
5. **Flexible navigation** - both path-based and chainable APIs
6. **Serialization via materialization** - see "Serialization Strategy" section below

## Detailed Type Definitions

### Core Types

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
    /// Each item includes its source ConfigValue reference
    pub items: Vec<MergedArrayItem<'a>>,
}

pub struct MergedArrayItem<'a> {
    /// The item value
    pub value: &'a ConfigValue,
    /// Which layer this item came from
    pub layer_index: usize,
}

/// A resolved map with merge semantics applied
pub struct MergedMap<'a> {
    /// Reference back to the config for further navigation
    config: &'a MergedConfig,
    /// The path to this map
    path: Vec<String>,
    /// Keys present in this map (union across layers, respecting prefer/concat)
    keys: Vec<String>,
}

/// A resolved value of any type (for when type is not known ahead of time)
///
/// This allows pattern matching on the resolved value type:
/// ```rust
/// match cursor.as_value() {
///     Some(MergedValue::Scalar(s)) => println!("scalar: {:?}", s.value),
///     Some(MergedValue::Array(a)) => println!("array with {} items", a.items.len()),
///     Some(MergedValue::Map(m)) => println!("map with keys: {:?}", m.keys()),
///     None => println!("path does not exist"),
/// }
/// ```
pub enum MergedValue<'a> {
    Scalar(MergedScalar<'a>),
    Array(MergedArray<'a>),
    Map(MergedMap<'a>),
}
```

### MergedConfig Implementation

```rust
impl<'a> MergedConfig<'a> {
    /// Create a merged config from multiple layers
    pub fn new(layers: Vec<&'a ConfigValue>) -> Self {
        MergedConfig { layers }
    }

    /// Add a new layer (returns new MergedConfig with same lifetime)
    pub fn with_layer(&self, layer: &'a ConfigValue) -> MergedConfig<'a> {
        let mut layers = self.layers.clone();
        layers.push(layer);
        MergedConfig { layers }
    }

    /// Get a cursor at the root
    pub fn cursor(&self) -> MergedCursor<'_> {
        MergedCursor {
            config: self,
            path: Vec::new(),
        }
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

### MergedCursor Implementation

```rust
impl<'a> MergedCursor<'a> {
    /// Navigate to a child key
    pub fn at(&self, key: &str) -> MergedCursor<'a> {
        let mut path = self.path.clone();
        path.push(key.to_string());
        MergedCursor {
            config: self.config,
            path,
        }
    }

    /// Navigate to a path (multiple keys at once)
    pub fn at_path(&self, path: &[&str]) -> MergedCursor<'a> {
        let mut new_path = self.path.clone();
        new_path.extend(path.iter().map(|s| s.to_string()));
        MergedCursor {
            config: self.config,
            path: new_path,
        }
    }

    /// Check if this path exists in any layer
    pub fn exists(&self) -> bool {
        self.config.layers.iter().any(|layer| {
            self.navigate_to(layer).is_some()
        })
    }

    /// Get child keys at this path (union across layers, respecting merge semantics)
    pub fn keys(&self) -> Vec<String> {
        self.compute_keys()
    }

    /// Resolve as any value type (for when type is not known ahead of time)
    ///
    /// This determines the effective type at this path and returns the
    /// appropriate resolved value. Useful for generic traversal or when
    /// the schema is not known statically.
    pub fn as_value(&self) -> Option<MergedValue<'a>> {
        // Determine the effective type by looking at the highest-priority
        // layer that defines this path
        for layer in self.config.layers.iter().rev() {
            if let Some(value) = self.navigate_to(layer) {
                return match &value.value {
                    ConfigValueKind::Scalar(_) | ConfigValueKind::PandocInlines(_) => {
                        self.as_scalar().map(MergedValue::Scalar)
                    }
                    ConfigValueKind::Array(_) | ConfigValueKind::PandocBlocks(_) => {
                        self.as_array().map(MergedValue::Array)
                    }
                    ConfigValueKind::Map(_) => {
                        self.as_map().map(MergedValue::Map)
                    }
                };
            }
        }
        None
    }

    /// Resolve as scalar (last-wins semantics)
    pub fn as_scalar(&self) -> Option<MergedScalar<'a>> {
        // Walk layers in reverse (highest priority first)
        for (i, layer) in self.config.layers.iter().enumerate().rev() {
            if let Some(value) = self.navigate_to(layer) {
                // For scalars, we just want the last defined value
                if matches!(value.value, ConfigValueKind::Scalar(_)
                                       | ConfigValueKind::PandocInlines(_)) {
                    return Some(MergedScalar {
                        value,
                        layer_index: i,
                    });
                }
            }
        }
        None
    }

    /// Resolve as array (applying prefer/concat semantics)
    pub fn as_array(&self) -> Option<MergedArray<'a>> {
        let mut items: Vec<MergedArrayItem<'a>> = Vec::new();

        // Walk layers in order (lowest priority first)
        for (i, layer) in self.config.layers.iter().enumerate() {
            if let Some(value) = self.navigate_to(layer) {
                match value.merge_op {
                    MergeOp::Prefer => {
                        // Reset: discard all previous items
                        items.clear();
                    }
                    MergeOp::Concat => {
                        // Keep existing items
                    }
                }

                if let ConfigValueKind::Array(arr) = &value.value {
                    for item in arr {
                        items.push(MergedArrayItem {
                            value: item,
                            layer_index: i,
                        });
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

    /// Resolve as map (applying prefer/concat semantics)
    pub fn as_map(&self) -> Option<MergedMap<'a>> {
        let keys = self.compute_keys();
        if keys.is_empty() {
            None
        } else {
            Some(MergedMap {
                config: self.config,
                path: self.path.clone(),
                keys,
            })
        }
    }

    // Internal helper: navigate to path within a single layer
    fn navigate_to(&self, root: &'a ConfigValue) -> Option<&'a ConfigValue> {
        let mut current = root;

        for key in &self.path {
            match &current.value {
                ConfigValueKind::Map(map) => {
                    current = map.get(key)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    // Internal helper: compute merged keys at this path
    fn compute_keys(&self) -> Vec<String> {
        let mut result_keys: IndexMap<String, ()> = IndexMap::new();

        for layer in &self.config.layers {
            if let Some(value) = self.navigate_to(layer) {
                match value.merge_op {
                    MergeOp::Prefer => {
                        // Reset ALL keys
                        result_keys.clear();
                    }
                    MergeOp::Concat => {
                        // Keep existing keys
                    }
                }

                if let ConfigValueKind::Map(map) = &value.value {
                    for key in map.keys() {
                        result_keys.insert(key.clone(), ());
                    }
                }
            }
        }

        result_keys.into_keys().collect()
    }
}
```

### MergedMap Implementation

```rust
impl<'a> MergedMap<'a> {
    /// Get the keys in this map
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Get a cursor to a child key
    pub fn get(&self, key: &str) -> MergedCursor<'a> {
        let mut path = self.path.clone();
        path.push(key.to_string());
        MergedCursor {
            config: self.config,
            path,
        }
    }

    /// Iterate over key-cursor pairs
    pub fn iter(&self) -> impl Iterator<Item = (&str, MergedCursor<'a>)> {
        self.keys.iter().map(move |key| {
            (key.as_str(), self.get(key))
        })
    }
}
```

## Usage Examples

### Basic Usage

```rust
// ConfigValues exist somewhere (e.g., parsed from YAML files)
let project_config: ConfigValue = parse_config("_quarto.yml");
let doc_config: ConfigValue = parse_config("document.qmd");

// Create merged config by borrowing - zero copy!
let merged = MergedConfig::new(vec![&project_config, &doc_config]);

// Direct path access
if let Some(theme) = merged.get_scalar(&["format", "html", "theme"]) {
    println!("Theme: {:?}", theme.value);
    println!("From layer: {}", theme.layer_index);
}

// Cursor-based navigation
let cursor = merged.cursor();
let format = cursor.at("format");

// Check multiple formats
for fmt in ["html", "pdf", "docx"] {
    if format.at(fmt).exists() {
        println!("Format {} is configured", fmt);
    }
}
```

### Generic Value Access with `as_value()`

```rust
// When you don't know the type ahead of time, use as_value()
fn print_config_value(cursor: MergedCursor<'_>, indent: usize) {
    let prefix = "  ".repeat(indent);
    match cursor.as_value() {
        Some(MergedValue::Scalar(s)) => {
            println!("{}scalar: {:?}", prefix, s.value);
        }
        Some(MergedValue::Array(a)) => {
            println!("{}array ({} items):", prefix, a.items.len());
            for (i, item) in a.items.iter().enumerate() {
                println!("{}  [{}]: {:?}", prefix, i, item.value);
            }
        }
        Some(MergedValue::Map(m)) => {
            println!("{}map:", prefix);
            for (key, child_cursor) in m.iter() {
                println!("{}  {}:", prefix, key);
                print_config_value(child_cursor, indent + 2);
            }
        }
        None => {
            println!("{}(not defined)", prefix);
        }
    }
}

// Use it to explore any config path
let cursor = merged.cursor().at("format").at("html");
print_config_value(cursor, 0);
```

### Working with Arrays

```rust
// Get merged filters array
if let Some(filters) = merged.get_array(&["filters"]) {
    for item in &filters.items {
        println!("Filter from layer {}: {:?}", item.layer_index, item.value);
    }
}
```

### Working with Maps

```rust
// Get format map and iterate keys
if let Some(format_map) = merged.get_map(&["format"]) {
    for (key, cursor) in format_map.iter() {
        println!("Format: {}", key);
        if let Some(theme) = cursor.at("theme").as_scalar() {
            println!("  Theme: {:?}", theme.value);
        }
    }
}
```

### Validation with Source Tracking

```rust
// When validation fails, we have source info
if let Some(value) = merged.get_scalar(&["format", "html", "theme"]) {
    if !is_valid_theme(&value.value) {
        // value.value.source_info points to the exact file and location
        emit_error(
            "Invalid theme",
            &value.value.source_info,
        );
    }
}
```

## Thread Safety Considerations

The current design uses `&'a ConfigValue` references. Thread safety depends on whether `ConfigValue` is `Sync`:

- If `ConfigValue` is `Sync`, then `&ConfigValue` is `Send`, and `MergedConfig` can be shared across threads
- This is typically the case for immutable data structures

If you need to *own* config values across threads, you would materialize first:

```rust
let materialized: ConfigValue = merged.materialize();
// materialized is owned and can be sent to another thread
```

## Serialization Strategy

**Key insight**: `MergedConfig<'a>` contains borrowed references and cannot be directly serialized. However, this is not a problem because:

1. **We don't mutate merged configs** - instead of modifying an existing merged config, we create new merges (`old <> new`) or rebuild from scratch
2. **Materialization preserves what matters** - the materialized `ConfigValue` tree contains all merged values with their `SourceInfo` intact
3. **Associativity guarantees correctness** - `materialize(a <> b) <> c == a <> b <> c`

### The Pattern

```
MergedConfig<'a> (layered, borrowed)
       ↓ materialize()
ConfigValue (owned tree with SourceInfo preserved)
       ↓ serialize
bytes/JSON/etc
       ↓ deserialize
ConfigValue (owned tree, can be used as a layer in new merges)
```

### What's Preserved

- **Source locations**: Each value's `SourceInfo` survives materialization. Validation errors will still point to the correct file and line.
- **Merge semantics**: The materialized tree reflects all `!prefer`/`!concat` semantics that were applied.

### What's Lost

- **Layer index**: You can't tell which layer a value came from after materialization. But `SourceInfo` provides the *real* provenance (file + location), so this is rarely needed.
- **Layered structure**: The separate layers are flattened into one tree. But due to associativity, re-merging works correctly.

### Edge Case: Serializing Layered Structure

If you ever need to serialize the layers themselves (not the merged result):

```rust
// Serialize layers separately
let layers: Vec<ConfigValue> = original_layers.iter().map(|l| (*l).clone()).collect();
let serialized = serde_json::to_string(&layers)?;

// Deserialize and reconstruct
let layers: Vec<ConfigValue> = serde_json::from_str(&serialized)?;
let merged = MergedConfig::new(layers.iter().collect());
```

This is a niche use case; the typical pattern is materialize-then-serialize.

## Materialization

For cases where an owned, materialized tree is needed (serialization, caching):

```rust
impl<'a> MergedConfig<'a> {
    /// Materialize the entire merged config into an owned ConfigValue tree
    pub fn materialize(&self) -> ConfigValue {
        self.materialize_at(&[])
    }

    /// Materialize a subtree at a path
    pub fn materialize_at(&self, path: &[&str]) -> ConfigValue {
        let cursor = self.cursor().at_path(path);
        cursor.materialize()
    }
}

impl<'a> MergedCursor<'a> {
    /// Materialize this cursor's value into an owned ConfigValue
    pub fn materialize(&self) -> ConfigValue {
        // Check what type of value this is
        if let Some(scalar) = self.as_scalar() {
            return scalar.value.clone();
        }

        if let Some(array) = self.as_array() {
            let items: Vec<ConfigValue> = array.items
                .iter()
                .map(|item| item.value.clone())
                .collect();
            return ConfigValue {
                value: ConfigValueKind::Array(items),
                source_info: SourceInfo::default(), // Or compute merged source
                merge_op: MergeOp::Concat,
                interpretation: None,
            };
        }

        if let Some(map) = self.as_map() {
            let entries: IndexMap<String, ConfigValue> = map.keys()
                .iter()
                .map(|key| {
                    let child = map.get(key);
                    (key.clone(), child.materialize())
                })
                .collect();
            return ConfigValue {
                value: ConfigValueKind::Map(entries),
                source_info: SourceInfo::default(), // Or compute merged source
                merge_op: MergeOp::Concat,
                interpretation: None,
            };
        }

        // Path doesn't exist - return null/empty
        ConfigValue::null()
    }
}
```

## Open Questions

### Q1: Should MergedCursor store path as `Vec<String>` or `Rc<[String]>`?

Current design uses `Vec<String>`, which clones on each `.at()` call. For deeply nested access patterns, this could be optimized:

```rust
// Option: Use Rc for path sharing
pub struct MergedCursor<'a> {
    config: &'a MergedConfig,
    path: Rc<Vec<String>>,
    depth: usize,  // How much of path is "ours"
}
```

**Recommendation**: Start with `Vec<String>`. Paths are typically short (3-5 segments), so the overhead is minimal. Optimize later if profiling shows it matters.

### Q2: Should we cache navigation results?

The current design recomputes navigation on each access. For hot paths, caching could help:

```rust
pub struct MergedConfig {
    layers: Vec<Rc<ConfigValue>>,
    // Cache of resolved values by path
    cache: RefCell<HashMap<Vec<String>, CachedValue>>,
}
```

**Recommendation**: Defer caching until we have benchmarks. The lazy evaluation already avoids most unnecessary work.

### Q3: How should we handle type mismatches during navigation?

If path `["format", "html"]` exists in layer 1 as a map but layer 2 has it as a scalar:

```yaml
# Layer 1
format:
  html:
    theme: cosmo

# Layer 2
format:
  html: "simple"  # Scalar, not a map!
```

Current design: The scalar wins (last layer), so `cursor.at("format").at("html").at("theme")` returns None because you can't navigate into a scalar.

**Recommendation**: This is correct behavior. The type mismatch is handled by "last wins" semantics, and validation will catch any schema violations.

## Implementation Plan

1. **Define core types** in `quarto-config/src/merge.rs`:
   - `MergedConfig<'a>`
   - `MergedCursor<'a>`
   - `MergedValue<'a>`
   - `MergedScalar<'a>`
   - `MergedArray<'a>`, `MergedArrayItem<'a>`
   - `MergedMap<'a>`

2. **Implement navigation** for `MergedCursor`:
   - `at()`, `at_path()`
   - `exists()`
   - `keys()`
   - Internal `navigate_to()` helper

3. **Implement resolution** for `MergedCursor`:
   - `as_value()` for type-agnostic access
   - `as_scalar()` with last-wins semantics
   - `as_array()` with prefer/concat semantics
   - `as_map()` with prefer/concat semantics

4. **Implement MergedMap** iteration

5. **Add convenience methods** to `MergedConfig`

6. **Add materialization** for when owned data is needed

7. **Write tests** covering:
   - Basic navigation
   - Merge semantics (prefer/concat for arrays and maps)
   - Type mismatches
   - Empty paths
   - Non-existent paths

## Relationship to Parent Design

This document refines the `MergedConfig<'a>` concept from the parent design:

- **Confirmed**: `MergedConfig<'a>` uses borrowed references (`&'a ConfigValue`)
- **Added**: `MergedCursor<'a>` type for ergonomic navigation
- **Added**: `MergedValue<'a>` enum for type-agnostic access
- **Added**: Detailed resolution algorithms for scalar/array/map
- **Added**: Serialization strategy via materialization
- **Preserved**: Lazy evaluation principle (resolution at access time)
- **Preserved**: `MergeOp::Prefer`/`Concat` semantics
