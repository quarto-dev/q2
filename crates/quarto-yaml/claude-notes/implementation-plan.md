# quarto-yaml Implementation Plan

## Overview

This crate implements `YamlWithSourceInfo`, a data structure that wraps `yaml-rust2::Yaml` with source location tracking.

## Architecture Decision: Owned Data

Following rust-analyzer's precedent, we use owned `Yaml` values with a parallel children structure for source tracking. Trade-off: ~3x memory overhead for simplicity and compatibility with config merging across different lifetimes.

## Core Data Structures

### 1. YamlWithSourceInfo

```rust
pub struct YamlWithSourceInfo {
    /// The complete yaml-rust2::Yaml value (owned)
    pub yaml: Yaml,

    /// Source location for this node
    pub source_info: SourceInfo,

    /// Source-tracked children (parallel structure)
    children: Children,
}
```

### 2. Children Enum

```rust
enum Children {
    None,
    Array(Vec<YamlWithSourceInfo>),
    Hash(Vec<YamlHashEntry>),
}
```

### 3. YamlHashEntry

```rust
pub struct YamlHashEntry {
    pub key: YamlWithSourceInfo,
    pub value: YamlWithSourceInfo,
    pub key_span: SourceInfo,    // Span of just the key
    pub value_span: SourceInfo,  // Span of just the value
    pub entry_span: SourceInfo,  // Span of key + value
}
```

## SourceInfo Type

For Phase 1, we'll use a simple SourceInfo type:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    /// Optional filename
    pub file: Option<String>,

    /// Byte offset in source
    pub offset: usize,

    /// Line number (1-based)
    pub line: usize,

    /// Column number (1-based)
    pub col: usize,

    /// Length in bytes
    pub len: usize,
}
```

Later this will be replaced by the unified SourceInfo from the main project.

## Implementation Phases

### Phase 1: Core Data Structures (Current)
- [x] Create crate structure
- [ ] Define SourceInfo type
- [ ] Define YamlWithSourceInfo, Children, YamlHashEntry
- [ ] Implement basic constructors

### Phase 2: Parser Implementation
- [ ] Implement MarkedEventReceiver trait
- [ ] Build tree from events
- [ ] Track source positions
- [ ] Handle errors

### Phase 3: Public API
- [ ] `parse(content: &str) -> Result<YamlWithSourceInfo>`
- [ ] `parse_file(content: &str, filename: &str) -> Result<YamlWithSourceInfo>`
- [ ] Access methods: `get_hash_value()`, `get_array_item()`, etc.
- [ ] Error type with source positions

### Phase 4: Testing
- [ ] Unit tests for data structures
- [ ] Parser tests with various YAML structures
- [ ] Source position tracking tests
- [ ] Error handling tests

### Phase 5: Documentation
- [ ] API documentation
- [ ] Usage examples
- [ ] Integration guide

## Parser Design

The parser will use yaml-rust2's `MarkedEventReceiver` API:

```rust
struct YamlBuilder {
    stack: Vec<YamlNode>,
    source: String,
    filename: Option<String>,
}

impl MarkedEventReceiver for YamlBuilder {
    fn on_event(&mut self, event: Event, marker: Marker) {
        // Build tree with source tracking
    }
}
```

## Testing Strategy

### Test Categories

1. **Basic YAML structures**
   - Scalars (string, int, float, bool)
   - Arrays
   - Hashes
   - Nested structures

2. **Source position tracking**
   - Verify line/column accuracy
   - Test multi-line values
   - Test nested structures

3. **Error handling**
   - Invalid YAML
   - Parse errors with positions

4. **Edge cases**
   - Empty documents
   - Documents with only comments
   - Multi-document streams (initially unsupported)

## Dependencies

- `yaml-rust2 = "0.9"` - YAML parsing with position tracking
- `serde = "1.0"` - For future SourceInfo serialization
- `thiserror = "1.0"` - Error types

## Future Enhancements

1. **Config merging** - Merge multiple YamlWithSourceInfo objects
2. **Validation** - Schema validation with source positions
3. **Unified SourceInfo** - Replace with project-wide SourceInfo type
4. **Multi-document** - Support YAML streams
