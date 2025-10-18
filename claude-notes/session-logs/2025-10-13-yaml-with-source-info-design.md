# Session Log: YamlWithSourceInfo Design (2025-10-13)

## Session Goals

Design the final implementation of `YamlWithSourceInfo` (renamed from `AnnotatedParse`) that:
1. Uses yaml-rust2's `Yaml` enum directly (avoid repeated conversions)
2. Handles lifetime management correctly
3. Supports config merging from different sources
4. Provides efficient access patterns (both raw Yaml and source-tracked)

## Key Challenge: Lifetime Tension

The user identified a fundamental tension:
- **Intuition**: Use lifetimes to avoid reconstruction (e.g., `yaml: &'a Yaml`)
- **Reality**: Config merging requires combining YAML from different sources with different lifetimes
- **Problem**: Can't merge `YamlWithSourceInfo<'proj>` and `YamlWithSourceInfo<'doc>` into a single struct

### The Question

> "It feels to me like AnnotatedParse should provide access to subparts of a Yaml enum (perhaps recursively) by returning references with lifetimes bound by the lifetime of the outermost Yaml object. This, however, seems to potentially become complicated when we take into account the requirements described in config-merging-analysis.md (different objects merged with different lifetimes seems potentially hard to design for)."

## Research Conducted

1. **Read config-merging-analysis.md**
   - Confirmed Strategy 4 (eager merging with AnnotatedParse) is recommended
   - Requires owned data to merge configs from different sources
   - Lifetimes would prevent merging

2. **Studied yaml-rust2::Yaml enum**
   - Structure: `Hash(LinkedHashMap<Yaml, Yaml>)`, `Array(Vec<Yaml>)`, scalars
   - Keys can be any Yaml type, not just strings
   - It's `Clone`, so we can copy it
   - Recursive structure (Hash contains Yaml, Array contains Yaml)

3. **Analyzed alternatives**:
   - Lifetimes: Clean but prevents config merging ❌
   - Custom YamlValue enum: Avoids yaml-rust2 ❌
   - Lazy children construction: Loses source tracking ❌
   - Arc/Rc everywhere: Complex, still needs cloning ❌

## Design Decisions

### 1. Use Owned Data Everywhere

**Decision**: Store `yaml: Yaml` (owned), not `yaml: &'a Yaml` (borrowed)

**Rationale**:
- Only way to support merging configs from different sources
- Simplifies API (no lifetime parameters everywhere)
- Yaml is Clone, configs are typically small (<10KB)
- Trade memory for simplicity and functionality

**Rejected Alternative**: Lifetime-based design
```rust
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,  // Can't merge with different lifetime
    ...
}
```

### 2. Parallel Children Structure

**Decision**: Store both the complete Yaml tree AND source-tracked children

```rust
pub struct YamlWithSourceInfo {
    pub yaml: Yaml,          // Complete tree (direct access)
    pub source_info: SourceInfo,
    children: Children,       // Parallel structure with source tracking
}

enum Children {
    None,                              // Scalars
    Array(Vec<YamlWithSourceInfo>),    // Array elements
    Hash(Vec<YamlHashEntry>),          // Key-value pairs
}
```

**Rationale**:
- The `yaml` field provides fast, direct access to yaml-rust2's types
- The `children` field enables source-tracked traversal
- Downstream code can use yaml-rust2 methods directly
- Yes, this duplicates data (~3x overhead), but it's the price of having both access patterns

**Rejected Alternative**: Store only children, build Yaml on demand
- Would require repeated Yaml construction (defeats "don't reconstruct" goal)

### 3. Support Non-String Hash Keys

**Decision**: Store hash entries as `Vec<YamlHashEntry>` with tracked keys and values

```rust
pub struct YamlHashEntry {
    pub key: YamlWithSourceInfo,    // Can be any Yaml type
    pub value: YamlWithSourceInfo,
}
```

**Rationale**:
- YAML spec allows any type as hash key
- Need source tracking for keys too (keys can have errors!)
- Maintains insertion order (important for YAML semantics)

### 4. Rename from AnnotatedParse

**Decision**: Use `YamlWithSourceInfo` instead of `AnnotatedParse`

**Rationale**:
- More descriptive (it's YAML with source info, not a generic parse structure)
- User's suggestion of specific naming (YAMLWithSourceInfo/JSONWithSourceInfo)
- Avoids confusion with other "annotated" structures

## Memory Overhead Analysis

### Data Duplication

The design stores child Yaml values twice:
1. In parent's `yaml` field (e.g., `Yaml::Hash` contains all key-value pairs)
2. In `children` field (each `YamlHashEntry` has `key.yaml` and `value.yaml`)

**Example**:
```rust
YamlWithSourceInfo {
    yaml: Yaml::Hash({
        Yaml::String("theme") => Yaml::String("cosmo"),  // <-- Here
    }),
    children: Children::Hash(vec![
        YamlHashEntry {
            key: YamlWithSourceInfo {
                yaml: Yaml::String("theme"),  // <-- And here
                ...
            },
            value: YamlWithSourceInfo {
                yaml: Yaml::String("cosmo"),  // <-- And here
                ...
            },
        },
    ]),
}
```

### Overhead Estimate

- Small config (~50 keys): ~5KB raw → ~15KB in memory (**~3x**)
- Large config (~500 keys): ~50KB raw → ~150KB in memory (**~3x**)

### Is This Acceptable?

✅ **Yes** for Quarto:
- Configs are typically <10KB
- Overhead is ~20-30KB per document
- Modern machines have GB of RAM
- Alternative (no duplication) requires complex lifetimes or repeated reconstruction
- Trade-off enables: direct Yaml access + source tracking + config merging

## API Design Highlights

### Construction

```rust
impl YamlWithSourceInfo {
    pub fn scalar(yaml: Yaml, source_info: SourceInfo) -> Self;
    pub fn array(elements: Vec<YamlWithSourceInfo>, source_info: SourceInfo) -> Self;
    pub fn hash(entries: Vec<YamlHashEntry>, source_info: SourceInfo) -> Self;
}
```

### Direct Yaml Access

```rust
impl YamlWithSourceInfo {
    pub fn as_yaml(&self) -> &Yaml;           // Direct access
    pub fn as_str(&self) -> Option<&str>;     // Forward to yaml
    pub fn as_i64(&self) -> Option<i64>;      // Forward to yaml
    pub fn as_bool(&self) -> Option<bool>;    // Forward to yaml
}
```

### Source-Tracked Traversal

```rust
impl YamlWithSourceInfo {
    // Array access
    pub fn get_array_element(&self, index: usize) -> Option<&YamlWithSourceInfo>;
    pub fn iter_array(&self) -> Option<impl Iterator<Item = &YamlWithSourceInfo>>;

    // Hash access (string keys - common case)
    pub fn get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo>;

    // Hash access (any key - rare but valid)
    pub fn get_hash_entry(&self, key: &Yaml) -> Option<&YamlHashEntry>;

    // Iterate hash
    pub fn iter_hash(&self) -> Option<impl Iterator<Item = &YamlHashEntry>>;

    // Path access
    pub fn get_path(&self, path: &[&str]) -> Option<&YamlWithSourceInfo>;
}
```

### Parsing from yaml-rust2

```rust
pub struct YamlWithSourceInfoParser {
    stack: Vec<PartialNode>,
    result: Option<YamlWithSourceInfo>,
    input_source_info: SourceInfo,
    input_text: String,
}

impl MarkedEventReceiver for YamlWithSourceInfoParser {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        // Build YamlWithSourceInfo from events
    }
}
```

### Config Merging

```rust
pub fn merge_yaml_with_source_info(
    base: &YamlWithSourceInfo,
    override_layer: &YamlWithSourceInfo,
) -> YamlWithSourceInfo {
    // Merge recursively, preserving source info
}
```

## Comparison with Alternatives

| Approach | Source Tracking | Config Merging | Memory | API Complexity | yaml-rust2 Direct |
|----------|----------------|----------------|---------|---------------|-------------------|
| **Owned (recommended)** | ✅ Perfect | ✅ Yes | ⚠️ 3x | ✅ Simple | ✅ Yes |
| Lifetimes | ✅ Perfect | ❌ No | ✅ 1x | ❌ Complex | ✅ Yes |
| Custom YamlValue | ✅ Perfect | ✅ Yes | ✅ 1x | ✅ Simple | ❌ No |
| Lazy children | ❌ Lost | ⚠️ Hard | ✅ 1x | ⚠️ Moderate | ✅ Yes |
| Arc everywhere | ✅ Perfect | ⚠️ Complex | ⚠️ 2x + runtime | ⚠️ Moderate | ✅ Yes |

**Verdict**: Owned data with parallel children is the best balance

## Implementation Plan

### Phase 1: Core Types (Week 1)
- Define `YamlWithSourceInfo`, `Children`, `YamlHashEntry`
- Implement construction methods
- Implement access methods
- Unit tests

### Phase 2: Parsing (Week 1-2)
- Implement `YamlWithSourceInfoParser` with `MarkedEventReceiver`
- Handle all yaml-rust2 Event types
- Calculate end positions for scalars
- Integration tests

### Phase 3: Config Merging (Week 2)
- Implement `merge_yaml_with_source_info`
- Implement `MergeCustomizer` trait
- Port TypeScript merge behaviors
- Multi-layer merge tests

### Phase 4: Validation (Week 2-3)
- Implement `validate_yaml` with schema
- Error reporting with source locations
- Integration with ariadne
- End-to-end tests

### Phase 5: Optimization (Week 3)
- Profile memory usage
- Profile merge performance
- Optimize hot paths
- Consider Arc for large subtrees if needed

### Phase 6: Integration (Week 3-4)
- Replace AnnotatedParse with YamlWithSourceInfo
- Update all consumers
- Documentation
- Backward compatibility

**Total estimate**: 3-4 weeks

## Artifacts Created

### Main Document

**[yaml-with-source-info-design.md](../yaml-with-source-info-design.md)** - Comprehensive 700+ line design document including:

- Executive summary with key decision (owned data)
- Background on the lifetime tension
- Detailed type definitions with complete API
- Construction, access (dual patterns), parsing, validation, merging
- Memory overhead analysis (~3x duplication)
- Comparison table with all alternatives
- 6-phase implementation plan (3-4 weeks)
- Open questions and success criteria

### Key Code Examples

1. **Core types** - YamlWithSourceInfo, Children, YamlHashEntry
2. **Dual access patterns** - Direct yaml access vs source-tracked traversal
3. **MarkedEventReceiver implementation** - Building from yaml-rust2 events
4. **Config merging** - Recursive merge preserving source info
5. **Validation** - Schema validation with source-tracked errors

## Key Technical Insights

### 1. Lifetimes vs Merging Trade-off

**The fundamental tension**:
- Lifetimes are perfect for single-file YAML parsing
- Config merging requires combining different lifetimes
- No way to reconcile both with Rust's type system

**Resolution**: Accept owned data, get mergeability

### 2. Parallel Structure is Necessary

Can't avoid duplication if you want:
1. Direct access to complete Yaml tree
2. Source tracking for each node
3. Mergeable across different sources

**Alternative architectures all have worse trade-offs**

### 3. Memory Overhead is Acceptable

- ~3x overhead for configs is negligible (KB not MB)
- Trade-off enables critical functionality
- Can optimize later if profiling shows issues

### 4. Use yaml-rust2 Types Directly

Avoids conversion layer between our types and yaml-rust2's types
- Downstream code can use yaml-rust2 methods
- One less enum to maintain
- Accept yaml-rust2's design choices (Alias, BadValue, etc.)

## Open Questions Addressed

### Q: Should we use lifetimes?

**Answer**: No, prevents config merging

**Reasoning**: Merging requires combining data from different sources with different lifetimes. Only owned data works.

### Q: How to avoid data duplication?

**Answer**: We can't, and that's okay

**Reasoning**: Need both complete Yaml tree AND source tracking. The alternatives (lazy construction, lifetimes, Arc) have worse trade-offs.

### Q: Should we use yaml-rust2's types directly?

**Answer**: Yes, use `yaml_rust2::Yaml`

**Reasoning**: Avoids conversion, lets downstream code use yaml-rust2 methods directly, one less enum to maintain.

### Q: What about non-string hash keys?

**Answer**: Support them fully

**Reasoning**: YAML spec allows it, keys can have errors too (need source tracking), not much more complex.

### Q: Memory overhead acceptable?

**Answer**: Yes, ~3x for configs is fine

**Reasoning**: Configs are small (<10KB), overhead is ~20-30KB per document, enables critical functionality.

## Next Steps

1. **Implement Phase 1** - Core types and construction
2. **Test with real configs** - Verify design works with actual Quarto YAML
3. **Profile early** - Measure memory usage and performance
4. **Iterate if needed** - Adjust based on real-world data

## Success Criteria

- ✅ Config merging works across different sources
- ✅ Direct Yaml access for performance-critical code
- ✅ Source-tracked traversal for validation
- ✅ Memory overhead <50KB per document
- ✅ Merge performance <5ms for 5-layer merge
- ✅ Clean API without lifetime parameters
- ✅ Unit test coverage >95%

## Key Takeaways

1. **Owned data is the only practical solution** - Lifetimes don't work for config merging

2. **Parallel structures are necessary** - Need both complete Yaml tree and source tracking

3. **Memory overhead is acceptable** - ~3x for small configs is negligible

4. **yaml-rust2 types directly** - Avoid conversion, enable downstream usage

5. **Trade-offs are explicit** - Memory for functionality, simplicity, and mergeability

6. **Design for the common case** - Optimize API for Quarto's actual usage patterns

## Documentation Updates

- Updated 00-INDEX.md with new design document
- Updated Technical Decisions section with YamlWithSourceInfo details
- Updated Configuration Merging decision
- Updated YAML Tags decision to reference new type name
- Created this session log

## Conclusion

The `YamlWithSourceInfo` design with owned data and parallel children provides the best balance for Quarto's requirements. While it incurs ~3x memory overhead, this trade-off enables:

1. ✅ Config merging from different sources
2. ✅ Direct yaml-rust2::Yaml access
3. ✅ Source-tracked traversal
4. ✅ Simple API without lifetime complexity

**Recommendation**: Proceed with implementation following the 3-4 week plan.
