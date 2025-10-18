# YamlWithSourceInfo: Design for Source-Tracked YAML Parsing

## Executive Summary

This document proposes the design for `YamlWithSourceInfo` (renamed from `AnnotatedParse`), a data structure that wraps yaml-rust2's `Yaml` enum with source location tracking. The design must balance several competing concerns:

1. **Use yaml-rust2's Yaml enum directly** - avoid repeated conversions
2. **Support config merging** - combine YAML from different sources with different lifetimes
3. **Provide efficient access** - both raw Yaml access and source-tracked traversal
4. **Maintain source fidelity** - track origin of every value for error reporting

**Key Decision**: Use **owned data** with parallel structures:
- The `yaml` field contains the complete yaml-rust2 `Yaml` tree
- The `children` field contains parallel source-tracked children
- This trades memory for API simplicity and mergeability

**Rationale**: Config merging requires combining YAML from different sources (project config, document config, etc.) with different lifetimes. Owned data is the only practical solution. The alternative (lifetimes + CoW) adds significant complexity for minimal benefit.

## Background: The Lifetime Tension

### Initial Intuition: Use Lifetimes

The user's initial idea was to use lifetimes to avoid reconstruction:

```rust
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,  // Borrow from original
    source_info: SourceInfo,
    children: Vec<YamlWithSourceInfo<'a>>,  // Share lifetime
}
```

This works beautifully for a **single YAML file**:
- Parse once, get owned `Yaml` tree
- Wrap with source info using references
- No duplication!

### The Problem: Config Merging

But Quarto needs to merge configs from **different sources**:

```rust
// These have different lifetimes!
let project_config: YamlWithSourceInfo<'proj> = parse_yaml("_quarto.yml")?;
let document_config: YamlWithSourceInfo<'doc> = parse_yaml("document.qmd")?;

// How to merge? What's the lifetime of the result?
let merged: YamlWithSourceInfo<'???> = merge(project_config, document_config);
```

You can't merge references from different lifetimes into a single structure. The options are:
1. **Owned data** - clone the Yaml trees (simple, works)
2. **Complex lifetime juggling** - use enums to track which lifetime each node came from (nightmare)
3. **Arc/Rc everywhere** - shared ownership (runtime overhead, still need cloning for merged nodes)

See [config-merging-analysis.md](config-merging-analysis.md) for detailed analysis of merging strategies.

## Recommended Design: Owned Data with Parallel Children

### Core Types

```rust
use yaml_rust2::Yaml;
use indexmap::IndexMap;

/// YAML value with complete source location tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YamlWithSourceInfo {
    /// The complete YAML value at this node (from yaml-rust2)
    /// Can be accessed directly for performance-critical code that doesn't need source tracking
    pub yaml: Yaml,

    /// Source location for this node
    pub source_info: SourceInfo,

    /// Source-tracked children (structure mirrors `yaml`)
    children: Children,
}

/// Child nodes with source tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Children {
    /// No children (for scalars: String, Integer, Boolean, Real, Null)
    None,

    /// Array elements with source info
    /// Invariant: elements.len() == yaml.as_vec().unwrap().len()
    Array(Vec<YamlWithSourceInfo>),

    /// Hash entries with source info
    /// Invariant: entries correspond to yaml.as_hash().unwrap() in insertion order
    Hash(Vec<YamlHashEntry>),
}

/// A key-value pair in a YAML hash with source tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YamlHashEntry {
    /// The key (can be any Yaml type, not just String)
    pub key: YamlWithSourceInfo,

    /// The value
    pub value: YamlWithSourceInfo,
}
```

### Key Design Decisions

#### 1. Owned `Yaml` Field

**Decision**: Store `yaml: Yaml` (owned), not `yaml: &'a Yaml` (borrowed)

**Rationale**:
- Enables merging configs from different sources
- Simplifies API (no lifetime parameters)
- Yaml is `Clone`, so we can copy it
- Config YAML is typically small (KB not MB)

**Trade-off**: Memory usage increases, but mergability is essential

#### 2. Parallel `children` Structure

**Decision**: Store source-tracked children separately from `yaml` field

**Rationale**:
- The `yaml` field gives fast access to the complete tree structure
- The `children` field provides source tracking for traversal
- Yes, this duplicates data, but it's the cost of having both access patterns

**Alternative considered**: Store *only* children with source info, reconstruct Yaml on demand
- **Rejected**: Would require repeated Yaml construction, defeating the "don't reconstruct" goal

#### 3. Hash Entry Structure

**Decision**: Store hash entries as `Vec<YamlHashEntry>` with both key and value tracked

**Rationale**:
- YAML allows any type as hash key (not just strings)
- Need source tracking for both keys and values (keys can have errors too!)
- Maintains insertion order (important for YAML semantics)

#### 4. Use yaml-rust2's Types Directly

**Decision**: Use `yaml_rust2::Yaml` instead of defining our own `YamlValue` enum

**Rationale**:
- Avoids conversion between our type and yaml-rust2's type
- Downstream code can use yaml-rust2's methods directly
- One less enum to maintain

**Trade-off**: We inherit yaml-rust2's design choices (Alias, BadValue, etc.)

## API Design

### Construction

```rust
impl YamlWithSourceInfo {
    /// Create a scalar YAML node
    pub fn scalar(yaml: Yaml, source_info: SourceInfo) -> Self {
        YamlWithSourceInfo {
            yaml,
            source_info,
            children: Children::None,
        }
    }

    /// Create an array YAML node
    pub fn array(
        elements: Vec<YamlWithSourceInfo>,
        source_info: SourceInfo,
    ) -> Self {
        let yaml = Yaml::Array(
            elements.iter().map(|e| e.yaml.clone()).collect()
        );
        YamlWithSourceInfo {
            yaml,
            source_info,
            children: Children::Array(elements),
        }
    }

    /// Create a hash YAML node
    pub fn hash(
        entries: Vec<YamlHashEntry>,
        source_info: SourceInfo,
    ) -> Self {
        let mut hash = yaml_rust2::Hash::new();
        for entry in &entries {
            hash.insert(entry.key.yaml.clone(), entry.value.yaml.clone());
        }
        let yaml = Yaml::Hash(hash);
        YamlWithSourceInfo {
            yaml,
            source_info,
            children: Children::Hash(entries),
        }
    }
}
```

### Access: Direct Yaml

```rust
impl YamlWithSourceInfo {
    /// Get reference to underlying Yaml
    /// Use this for performance-critical code that doesn't need source tracking
    pub fn as_yaml(&self) -> &Yaml {
        &self.yaml
    }

    /// Consume self and return the underlying Yaml
    pub fn into_yaml(self) -> Yaml {
        self.yaml
    }

    // Convenience forwarding methods
    pub fn as_str(&self) -> Option<&str> {
        self.yaml.as_str()
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.yaml.as_i64()
    }

    pub fn as_bool(&self) -> Option<bool> {
        self.yaml.as_bool()
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.yaml.as_f64()
    }

    pub fn is_null(&self) -> bool {
        self.yaml.is_null()
    }
}
```

### Access: Source-Tracked Traversal

```rust
impl YamlWithSourceInfo {
    /// Get array element at index with source tracking
    pub fn get_array_element(&self, index: usize) -> Option<&YamlWithSourceInfo> {
        match &self.children {
            Children::Array(elements) => elements.get(index),
            _ => None,
        }
    }

    /// Iterate over array elements with source tracking
    pub fn iter_array(&self) -> Option<impl Iterator<Item = &YamlWithSourceInfo>> {
        match &self.children {
            Children::Array(elements) => Some(elements.iter()),
            _ => None,
        }
    }

    /// Get hash value by string key with source tracking
    /// This is the common case for Quarto configs
    pub fn get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo> {
        match &self.children {
            Children::Hash(entries) => {
                for entry in entries {
                    if let Yaml::String(k) = &entry.key.yaml {
                        if k == key {
                            return Some(&entry.value);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Get hash entry by any Yaml key with source tracking
    /// Supports non-string keys (rare but valid YAML)
    pub fn get_hash_entry(&self, key: &Yaml) -> Option<&YamlHashEntry> {
        match &self.children {
            Children::Hash(entries) => {
                entries.iter().find(|e| &e.key.yaml == key)
            }
            _ => None,
        }
    }

    /// Iterate over hash entries with source tracking
    pub fn iter_hash(&self) -> Option<impl Iterator<Item = &YamlHashEntry>> {
        match &self.children {
            Children::Hash(entries) => Some(entries.iter()),
            _ => None,
        }
    }

    /// Get nested value by path with source tracking
    /// Example: get_path(&["format", "html", "theme"])
    pub fn get_path(&self, path: &[&str]) -> Option<&YamlWithSourceInfo> {
        let mut current = self;
        for &key in path {
            current = current.get_hash_value(key)?;
        }
        Some(current)
    }
}
```

### Parsing from yaml-rust2

```rust
/// Parser that builds YamlWithSourceInfo from yaml-rust2 events
pub struct YamlWithSourceInfoParser {
    /// Stack of partially-built nodes
    stack: Vec<PartialNode>,

    /// Completed root node
    result: Option<YamlWithSourceInfo>,

    /// Source info for the input YAML string
    input_source_info: SourceInfo,

    /// The input YAML string (needed for end position calculation)
    input_text: String,
}

enum PartialNode {
    Scalar {
        start: usize,
        yaml: Yaml,
    },
    Array {
        start: usize,
        elements: Vec<YamlWithSourceInfo>,
    },
    Hash {
        start: usize,
        entries: Vec<YamlHashEntry>,
        pending_key: Option<YamlWithSourceInfo>,
    },
}

impl MarkedEventReceiver for YamlWithSourceInfoParser {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        match ev {
            Event::Scalar(text, style, _anchor, _tag) => {
                let start = mark.index();
                let end = self.find_scalar_end(start, &text, style);
                let source_info = self.input_source_info.substring(start, end);

                // Convert to Yaml (using yaml-rust2's logic)
                let yaml = Yaml::from_str(&text);

                let node = YamlWithSourceInfo::scalar(yaml, source_info);
                self.insert_node(node);
            }

            Event::SequenceStart(_anchor, _tag) => {
                let start = mark.index();
                self.stack.push(PartialNode::Array {
                    start,
                    elements: Vec::new(),
                });
            }

            Event::SequenceEnd => {
                let node = self.stack.pop().unwrap();
                if let PartialNode::Array { start, elements } = node {
                    let end = mark.index();
                    let source_info = self.input_source_info.substring(start, end);
                    let yaml_node = YamlWithSourceInfo::array(elements, source_info);
                    self.insert_node(yaml_node);
                }
            }

            Event::MappingStart(_anchor, _tag) => {
                let start = mark.index();
                self.stack.push(PartialNode::Hash {
                    start,
                    entries: Vec::new(),
                    pending_key: None,
                });
            }

            Event::MappingEnd => {
                let node = self.stack.pop().unwrap();
                if let PartialNode::Hash { start, entries, .. } = node {
                    let end = mark.index();
                    let source_info = self.input_source_info.substring(start, end);
                    let yaml_node = YamlWithSourceInfo::hash(entries, source_info);
                    self.insert_node(yaml_node);
                }
            }

            _ => { /* Handle other events */ }
        }
    }
}

impl YamlWithSourceInfoParser {
    fn insert_node(&mut self, node: YamlWithSourceInfo) {
        if self.stack.is_empty() {
            // Root node
            self.result = Some(node);
        } else {
            let parent = self.stack.last_mut().unwrap();
            match parent {
                PartialNode::Array { elements, .. } => {
                    elements.push(node);
                }
                PartialNode::Hash { entries, pending_key, .. } => {
                    if let Some(key) = pending_key.take() {
                        // This node is a value
                        entries.push(YamlHashEntry { key, value: node });
                    } else {
                        // This node is a key
                        *pending_key = Some(node);
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

/// Parse YAML string into YamlWithSourceInfo
pub fn parse_yaml_with_source_info(
    yaml_str: &str,
    source_info: SourceInfo,
) -> Result<YamlWithSourceInfo, ScanError> {
    let mut parser = Parser::new(yaml_str.chars());
    let mut receiver = YamlWithSourceInfoParser {
        stack: Vec::new(),
        result: None,
        input_source_info: source_info,
        input_text: yaml_str.to_string(),
    };

    parser.load(&mut receiver, true)?;

    receiver.result.ok_or_else(|| {
        ScanError::new(Marker::new(0, 0, 0), "No YAML document found")
    })
}
```

## Config Merging

```rust
/// Merge two YAML trees, preserving source locations
pub fn merge_yaml_with_source_info(
    base: &YamlWithSourceInfo,
    override_layer: &YamlWithSourceInfo,
) -> YamlWithSourceInfo {
    merge_yaml_with_customizer(base, override_layer, &DefaultMergeCustomizer)
}

pub fn merge_yaml_with_customizer(
    base: &YamlWithSourceInfo,
    override_layer: &YamlWithSourceInfo,
    customizer: &dyn MergeCustomizer,
) -> YamlWithSourceInfo {
    use Yaml::*;

    match (&base.yaml, &override_layer.yaml) {
        // Both are hashes: merge recursively
        (Hash(_), Hash(_)) => {
            merge_hashes(base, override_layer, customizer)
        }

        // Both are arrays: concatenate and deduplicate
        (Array(_), Array(_)) => {
            merge_arrays(base, override_layer)
        }

        // Scalar override: later wins
        _ => override_layer.clone(),
    }
}

fn merge_hashes(
    base: &YamlWithSourceInfo,
    override_layer: &YamlWithSourceInfo,
    customizer: &dyn MergeCustomizer,
) -> YamlWithSourceInfo {
    let base_entries = match &base.children {
        Children::Hash(e) => e,
        _ => unreachable!(),
    };
    let override_entries = match &override_layer.children {
        Children::Hash(e) => e,
        _ => unreachable!(),
    };

    let mut merged_entries = Vec::new();
    let mut processed_keys = HashSet::new();

    // Process override entries first (they win)
    for override_entry in override_entries {
        let key_str = override_entry.key.as_str();

        // Try to find matching key in base
        if let Some(base_entry) = base_entries.iter().find(|e| {
            if let (Some(k1), Some(k2)) = (e.key.as_str(), key_str) {
                k1 == k2
            } else {
                e.key.yaml == override_entry.key.yaml
            }
        }) {
            // Key exists in both: check customizer, then recursively merge
            let merged_value = if let Some(key_str) = key_str {
                if let Some(custom) = customizer.customize(
                    key_str,
                    &base_entry.value,
                    &override_entry.value,
                ) {
                    custom
                } else {
                    merge_yaml_with_customizer(
                        &base_entry.value,
                        &override_entry.value,
                        customizer,
                    )
                }
            } else {
                // Non-string key: no customization
                merge_yaml_with_customizer(
                    &base_entry.value,
                    &override_entry.value,
                    customizer,
                )
            };

            merged_entries.push(YamlHashEntry {
                key: override_entry.key.clone(),  // Use override's key source
                value: merged_value,
            });
        } else {
            // Key only in override: use as-is
            merged_entries.push(override_entry.clone());
        }

        if let Some(k) = key_str {
            processed_keys.insert(k.to_string());
        }
    }

    // Add base entries that weren't overridden
    for base_entry in base_entries {
        if let Some(key_str) = base_entry.key.as_str() {
            if !processed_keys.contains(key_str) {
                merged_entries.push(base_entry.clone());
            }
        }
    }

    // Create merged source info
    let merged_source = SourceInfo::concat(vec![
        (base.source_info.clone(), 1),
        (override_layer.source_info.clone(), 1),
    ]);

    YamlWithSourceInfo::hash(merged_entries, merged_source)
}

fn merge_arrays(
    base: &YamlWithSourceInfo,
    override_layer: &YamlWithSourceInfo,
) -> YamlWithSourceInfo {
    let base_elements = match &base.children {
        Children::Array(e) => e,
        _ => unreachable!(),
    };
    let override_elements = match &override_layer.children {
        Children::Array(e) => e,
        _ => unreachable!(),
    };

    // Concatenate
    let mut combined = base_elements.clone();
    combined.extend(override_elements.iter().cloned());

    // Deduplicate by JSON representation (matching TypeScript behavior)
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for element in combined {
        let key = serde_json::to_string(&element.yaml).unwrap_or_else(|_| {
            format!("{:?}", element.yaml)
        });
        if seen.insert(key) {
            deduped.push(element);
        }
    }

    let merged_source = SourceInfo::concat(vec![
        (base.source_info.clone(), base_elements.len()),
        (override_layer.source_info.clone(), override_elements.len()),
    ]);

    YamlWithSourceInfo::array(deduped, merged_source)
}
```

## Validation Integration

```rust
/// Validate a YamlWithSourceInfo tree against a schema
pub fn validate_yaml(
    yaml: &YamlWithSourceInfo,
    schema: &Schema,
    ctx: &SourceContext,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    validate_recursive(yaml, schema, &[], &mut errors, ctx);
    errors
}

fn validate_recursive(
    yaml: &YamlWithSourceInfo,
    schema: &Schema,
    path: &[&str],
    errors: &mut Vec<ValidationError>,
    ctx: &SourceContext,
) {
    match schema {
        Schema::String => {
            if yaml.as_str().is_none() {
                errors.push(ValidationError {
                    path: path.to_vec(),
                    message: "Expected string".to_string(),
                    source_info: yaml.source_info.clone(),
                });
            }
        }

        Schema::Object(properties) => {
            if let Some(entries) = yaml.iter_hash() {
                for entry in entries {
                    if let Some(key_str) = entry.key.as_str() {
                        if let Some(prop_schema) = properties.get(key_str) {
                            let mut new_path = path.to_vec();
                            new_path.push(key_str);
                            validate_recursive(
                                &entry.value,
                                prop_schema,
                                &new_path,
                                errors,
                                ctx,
                            );
                        } else {
                            errors.push(ValidationError {
                                path: path.to_vec(),
                                message: format!("Unknown property '{}'", key_str),
                                source_info: entry.key.source_info.clone(),
                            });
                        }
                    }
                }
            } else {
                errors.push(ValidationError {
                    path: path.to_vec(),
                    message: "Expected object".to_string(),
                    source_info: yaml.source_info.clone(),
                });
            }
        }

        Schema::Array(item_schema) => {
            if let Some(elements) = yaml.iter_array() {
                for (i, element) in elements.enumerate() {
                    validate_recursive(element, item_schema, path, errors, ctx);
                }
            } else {
                errors.push(ValidationError {
                    path: path.to_vec(),
                    message: "Expected array".to_string(),
                    source_info: yaml.source_info.clone(),
                });
            }
        }

        // ... other schema types
    }
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: Vec<String>,
    pub message: String,
    pub source_info: SourceInfo,
}

impl ValidationError {
    pub fn format_error(&self, ctx: &SourceContext) -> String {
        if let Some(mapped) = self.source_info.map_offset(0, ctx) {
            if let Some(file) = ctx.get_file(mapped.file_id) {
                return format!(
                    "{}:{}:{}: {}",
                    file.path.display(),
                    mapped.location.row,
                    mapped.location.column,
                    self.message
                );
            }
        }
        format!("Error: {}", self.message)
    }
}
```

## Memory Overhead Analysis

### Data Duplication

**The elephant in the room**: We store child Yaml values twice:
1. In the parent's `yaml` field (e.g., `Yaml::Hash` contains all key-value pairs)
2. In the `children` field (each `YamlHashEntry` has `key.yaml` and `value.yaml`)

**Example**:
```rust
let yaml = YamlWithSourceInfo {
    yaml: Yaml::Hash({
        Yaml::String("theme") => Yaml::String("cosmo"),  // <-- Stored here
        Yaml::String("toc") => Yaml::Boolean(true),
    }),
    children: Children::Hash(vec![
        YamlHashEntry {
            key: YamlWithSourceInfo {
                yaml: Yaml::String("theme"),  // <-- AND here
                ...
            },
            value: YamlWithSourceInfo {
                yaml: Yaml::String("cosmo"),  // <-- AND here
                ...
            },
        },
        // ...
    ]),
}
```

**Rough overhead estimate**:
- Small config (~50 keys): ~5KB raw YAML → ~15KB in memory (3x overhead)
- Large config (~500 keys): ~50KB raw YAML → ~150KB in memory (3x overhead)

**Is this acceptable?**
- ✅ Yes for Quarto: Configs are typically <10KB, overhead is ~20-30KB per document
- ✅ Modern machines have GB of RAM, KB duplication is negligible
- ✅ Trade-off enables: direct Yaml access + source tracking + config merging
- ✅ Alternative (no duplication) would require complex lifetime management or repeated reconstruction

### Potential Optimization (Future)

If memory becomes a concern, we could use `Arc` for large Yaml subtrees:

```rust
pub struct YamlWithSourceInfo {
    yaml: Arc<Yaml>,  // Shared ownership
    source_info: SourceInfo,
    children: Children,
}
```

But this:
- Adds runtime overhead (Arc refcount operations)
- Complicates mutation (need Arc::make_mut)
- Doesn't eliminate all duplication (children still need their own Arc)

**Recommendation**: Start with owned data, profile, optimize if needed

## Comparison with Alternatives

### Alternative 1: Lifetimes

```rust
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,
    source_info: SourceInfo,
    children: Vec<YamlWithSourceInfo<'a>>,
}
```

**Pros**: No duplication, zero-cost abstraction
**Cons**: Cannot merge configs from different sources (different lifetimes)
**Verdict**: ❌ Not viable for Quarto's use case

### Alternative 2: Custom Enum (No yaml-rust2)

```rust
pub enum YamlValue {
    String(String),
    Integer(i64),
    // ... define our own
}

pub struct YamlWithSourceInfo {
    value: YamlValue,
    source_info: SourceInfo,
    children: Vec<YamlWithSourceInfo>,
}
```

**Pros**: No duplication, full control
**Cons**: Requires conversion from yaml-rust2, downstream code can't use yaml-rust2 methods
**Verdict**: ❌ Defeats goal of using yaml-rust2 directly

### Alternative 3: Lazy Children (Build on Demand)

```rust
pub struct YamlWithSourceInfo {
    yaml: Yaml,
    source_info: SourceInfo,
    // No children field! Build them on demand
}

impl YamlWithSourceInfo {
    pub fn get_hash_value(&self, key: &str) -> Option<YamlWithSourceInfo> {
        // Problem: Where do we get the source_info for the child?
        // We'd need to store it somewhere... back to square one
    }
}
```

**Pros**: No duplication of Yaml data
**Cons**: Loses source tracking for children (the whole point!)
**Verdict**: ❌ Not viable

### Recommended Design (This Document)

**Pros**:
- ✅ Uses yaml-rust2 directly
- ✅ Supports config merging
- ✅ Provides both raw Yaml access and source-tracked traversal
- ✅ Simple API, no lifetime complexity

**Cons**:
- ⚠️ Data duplication (~3x memory overhead)

**Verdict**: ⭐ **Best balance** for Quarto's requirements

## Implementation Plan

### Phase 1: Core Types (Week 1)
- [ ] Define `YamlWithSourceInfo`, `Children`, `YamlHashEntry`
- [ ] Implement construction methods
- [ ] Implement access methods (as_yaml, iter_array, etc.)
- [ ] Unit tests

### Phase 2: Parsing from yaml-rust2 (Week 1-2)
- [ ] Implement `YamlWithSourceInfoParser` with `MarkedEventReceiver`
- [ ] Handle all Event types
- [ ] Implement end position calculation for scalars
- [ ] Integration tests with real YAML

### Phase 3: Config Merging (Week 2)
- [ ] Implement `merge_yaml_with_source_info`
- [ ] Implement `MergeCustomizer` trait
- [ ] Port TypeScript merge behaviors (variants, disableable arrays, etc.)
- [ ] Tests for multi-layer merging

### Phase 4: Validation Integration (Week 2-3)
- [ ] Implement `validate_yaml` with schema
- [ ] Error reporting with source locations
- [ ] Integration with ariadne for pretty errors
- [ ] Tests with merged configs

### Phase 5: Optimization (Week 3)
- [ ] Profile memory usage
- [ ] Profile merge performance
- [ ] Optimize hot paths if needed
- [ ] Consider Arc for large subtrees if beneficial

### Phase 6: Replace Existing Code (Week 3-4)
- [ ] Replace AnnotatedParse with YamlWithSourceInfo
- [ ] Update all consumers
- [ ] Ensure backward compatibility
- [ ] Documentation

**Total estimate**: 3-4 weeks

## Open Questions

### Q1: Should we implement Hash lookups as HashMap for O(1) access?

Currently `get_hash_value` is O(n) in number of hash entries.

**Options**:
1. Keep Vec, O(n) lookup (current design)
2. Add HashMap<String, usize> index for string keys
3. Add HashMap<Yaml, usize> index for all keys

**Recommendation**: Start with O(n), optimize if profiling shows it's a bottleneck. Most configs have <50 keys.

### Q2: How to handle Yaml::Alias?

yaml-rust2 supports YAML aliases (`&anchor` and `*alias`). Should we:
1. Resolve aliases during parsing (easier)
2. Preserve alias structure (more faithful to source)

**Recommendation**: Resolve during parsing. Aliases are rare in Quarto configs.

### Q3: Should we add convenience methods for common access patterns?

e.g.:
```rust
impl YamlWithSourceInfo {
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.get_hash_value(key)?.as_str()
    }
}
```

**Recommendation**: Yes, add as needed based on usage patterns. Start with basics.

## Success Criteria

- ✅ All three YAML scenarios work (standalone, metadata, cell options)
- ✅ Config merging preserves source locations correctly
- ✅ Validation errors point to exact source locations
- ✅ Can use yaml-rust2's Yaml directly for non-tracked operations
- ✅ Memory overhead acceptable (<50KB per document)
- ✅ Merge performance acceptable (<5ms for 5-layer merge)
- ✅ Unit test coverage >95%

## Conclusion

The `YamlWithSourceInfo` design with owned data and parallel children provides the best balance for Quarto's requirements:

1. **Uses yaml-rust2 directly** - no repeated conversions
2. **Supports config merging** - owned data enables merging different lifetimes
3. **Dual access patterns** - raw Yaml for performance, source-tracked for errors
4. **Simple API** - no lifetime parameters, straightforward to use

The memory overhead (~3x) is acceptable given the size of configs and the benefits gained. The alternative (lifetimes) would prevent config merging, which is essential.

**Recommendation**: Proceed with implementation following the 3-4 week plan.
