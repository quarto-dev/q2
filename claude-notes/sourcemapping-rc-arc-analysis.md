# SourceMapping Performance: Rc vs Arc Analysis

## Problem Statement

`SourceMapping` currently uses `Box<SourceInfo>` for parent references in `Substring` and `Transformed` variants. This causes expensive deep clones during YAML parsing.

### Current Implementation

```rust
pub enum SourceMapping {
    Original { file_id: FileId },
    Substring {
        parent: Box<SourceInfo>,  // <-- Deep clone on every clone()
        offset: usize
    },
    Concat { pieces: Vec<SourcePiece> },
    Transformed {
        parent: Box<SourceInfo>,  // <-- Deep clone on every clone()
        mapping: Vec<RangeMapping>
    },
}
```

### Performance Issue

In `quarto-yaml/src/parser.rs`, `make_source_info()` is called for every YAML node:

```rust
fn make_source_info(&self, marker: &Marker, len: usize) -> SourceInfo {
    if let Some(ref parent) = self.parent {
        SourceInfo::substring(parent.clone(), start_offset, end_offset)  // <-- EXPENSIVE
    }
    // ...
}
```

For a YAML document with N nodes:
- **Current**: O(N * D) where D = depth of parent chain
- **With Rc/Arc**: O(N) - just increment reference count

## Serialization Compatibility

Serde supports Rc/Arc with the `rc` feature flag:

```toml
[dependencies]
serde = { version = "1.0", features = ["rc"] }
```

**Important caveat**: Serialization doesn't preserve sharing - each Rc/Arc is serialized as a full copy.

**Why this is acceptable for our use case**:
- Cloning happens hundreds of times during parsing (performance critical)
- Serialization happens once per document (not performance critical)
- Deserialization creates new instances anyway (expected behavior for WASM/TypeScript integration)

## Rc vs Arc Trade-offs

### Rc (Reference Counted)

**Pros**:
- Slightly faster (no atomic operations)
- Lower memory overhead
- Simpler

**Cons**:
- Not thread-safe
- Cannot be sent between threads

### Arc (Atomic Reference Counted)

**Pros**:
- Thread-safe
- Can be sent between threads
- Future-proof for parallel processing

**Cons**:
- Slightly slower (atomic operations)
- Slightly higher memory overhead

## Current Threading Analysis

### quarto-source-map
- `SourceInfo` derives `Clone` (not `Send` or `Sync` explicitly)
- No current multi-threaded usage
- Library is single-threaded

### quarto-yaml
- YAML parsing is single-threaded
- No parallel processing

### quarto-markdown-pandoc
- Parsing is single-threaded
- No evidence of parallel processing in AST construction

## Recommendation: Use Rc

**Rationale**:
1. **No current threading requirements**: All parsing is single-threaded
2. **Performance**: Rc is slightly faster (no atomic operations)
3. **Simplicity**: Easier to reason about in single-threaded code
4. **YAGNI principle**: Don't add thread-safety until we need it

**If threading is needed in the future**:
- Easy migration path: `Rc<T>` → `Arc<T>` is a simple search-and-replace
- Compiler will catch any threading issues immediately
- Can be done as a separate PR when needed

## Implementation Plan

1. **Add serde rc feature**:
   ```toml
   # In quarto-source-map/Cargo.toml
   [dependencies]
   serde = { version = "1.0", features = ["derive", "rc"] }
   ```

2. **Update SourceMapping**:
   ```rust
   use std::rc::Rc;

   pub enum SourceMapping {
       Original { file_id: FileId },
       Substring {
           parent: Rc<SourceInfo>,  // Changed from Box
           offset: usize
       },
       Concat { pieces: Vec<SourcePiece> },
       Transformed {
           parent: Rc<SourceInfo>,  // Changed from Box
           mapping: Vec<RangeMapping>
       },
   }
   ```

3. **Update constructors**:
   ```rust
   pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self {
       // ...
       mapping: SourceMapping::Substring {
           parent: Rc::new(parent),  // Changed from Box::new
           offset: start,
       },
   }
   ```

4. **Update SourcePiece** (also clones SourceInfo):
   ```rust
   pub struct SourcePiece {
       pub source_info: Rc<SourceInfo>,  // Consider this too
       pub offset_in_concat: usize,
       pub length: usize,
   }
   ```

## Testing Strategy

1. Run existing serialization tests to verify JSON output unchanged
2. Add benchmark comparing Box vs Rc performance on large YAML documents
3. Verify all tests pass after migration

## Expected Performance Improvement

For a typical YAML document with 100 nodes and depth 3:
- **Before**: ~300 deep clones (100 nodes × depth 3)
- **After**: ~100 reference count increments
- **Estimated speedup**: 10-50x for documents with deep nesting

## Migration Checklist

- [ ] Add `serde = { features = ["rc"] }` to Cargo.toml
- [ ] Change `Box<SourceInfo>` to `Rc<SourceInfo>` in SourceMapping
- [ ] Update `substring()` constructor
- [ ] Update `transformed()` constructor
- [ ] Update `concat()` if SourcePiece uses SourceInfo
- [ ] Run `cargo test` to verify all tests pass
- [ ] Run serialization tests specifically
- [ ] Consider adding performance benchmark
- [ ] Update documentation if needed

## Open Questions

1. Should we also change `SourcePiece.source_info` to use `Rc<SourceInfo>`?
2. Do we need benchmarks before/after to quantify the improvement?
3. Should we document the serialization caveat in the API docs?

## References

- Serde Rc/Arc docs: https://serde.rs/feature-flags.html
- Stack Overflow discussion: https://stackoverflow.com/questions/49312600/
- Serde issue #194: https://github.com/serde-rs/serde/issues/194
