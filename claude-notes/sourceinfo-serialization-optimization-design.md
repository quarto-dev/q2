# SourceInfo JSON Serialization Optimization: Design Proposal

**Date**: 2025-10-19
**Related Issues**: k-44
**Measurements**: See sourceinfo-json-serialization-measurements.md

## Problem Statement

Current SourceInfo JSON serialization produces **25-55x blowup** compared to original QMD source. This is unacceptable for shipping ASTs across WASM/TypeScript boundaries, network transfer, or caching.

### Root Cause

Each `Substring` mapping serializes its complete parent chain:

```json
{
  "mapping": {
    "Substring": {
      "offset": N,
      "parent": {
        "mapping": { /* ENTIRE parent chain */ },
        "range": { /* ... */ }
      }
    }
  }
}
```

For 100 sibling YAML nodes sharing the same parent:
- **In memory (with Rc)**: 1 copy of parent chain
- **In JSON**: 100 copies of parent chain (**51-55x blowup**)

## Design Goals

1. **Minimize JSON size** - Target <5x blowup instead of 25-55x
2. **Maintain correctness** - All source info queries must still work
3. **Backward compatibility** - Don't break Pandoc JSON format
4. **Fast serialization/deserialization** - Avoid complex graph algorithms
5. **Simple implementation** - Easy to maintain and debug

## Proposed Solution: Interned Parent Chains with ID References

### Core Idea

Instead of duplicating parent chains, maintain a **pool of unique SourceInfo objects** and reference them by ID.

### JSON Structure

```json
{
  "pandoc-api-version": [1, 23, 1],
  "meta": { /* Pandoc standard format */ },
  "blocks": [ /* Pandoc standard format */ ],
  "astContext": {
    "filenames": [...],
    "sourceInfoPool": [
      {"id": 0, "range": {...}, "mapping": {"Original": {"file_id": 0}}},
      {"id": 1, "range": {...}, "mapping": {"Substring": {"parent_id": 0, "offset": 4}}},
      {"id": 2, "range": {...}, "mapping": {"Substring": {"parent_id": 1, "offset": 10}}}
    ]
  }
}
```

In the AST, instead of:
```json
{
  "key_source": {
    "range": {...},
    "mapping": {
      "Substring": {
        "offset": 10,
        "parent": { /* full parent chain */ }
      }
    }
  }
}
```

We write:
```json
{
  "key_source": {"$ref": 2}
}
```

### Advantages

✅ **Massive size reduction**: Each SourceInfo appears once
✅ **Exact deduplication**: Rc-shared objects get same ID
✅ **Fast lookup**: ID → SourceInfo is O(1) array access
✅ **Maintains Pandoc compatibility**: Changes only in astContext
✅ **Preserves all information**: Can reconstruct exact same structure

### Size Improvement Estimate

From our measurements (100 siblings test):
- **Current**: 103,677 bytes (51.05x blowup)
- **Expected with pooling**: ~5,000-8,000 bytes (2.5-4x blowup)
- **Improvement**: ~93% size reduction

The remaining blowup comes from:
- Range/Location structures (2 Locations per SourceInfo)
- JSON overhead (field names, braces, commas)
- Pool metadata (IDs)

## Implementation Plan

### Phase 1: Custom Serialization (Writer)

Add custom serialization that builds the pool:

```rust
// In writers/json.rs

struct SourceInfoSerializer {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
}

#[derive(Serialize)]
struct SerializableSourceInfo {
    id: usize,
    range: Range,
    mapping: SerializableSourceMapping,
}

#[derive(Serialize)]
enum SerializableSourceMapping {
    Original { file_id: FileId },
    Substring { parent_id: usize, offset: usize },
    Concat { pieces: Vec<SerializableSourcePiece> },
    Transformed { parent_id: usize, mapping: Vec<RangeMapping> },
}

impl SourceInfoSerializer {
    fn intern(&mut self, source_info: &SourceInfo) -> usize {
        let ptr = source_info as *const SourceInfo;

        if let Some(&id) = self.id_map.get(&ptr) {
            return id; // Already interned
        }

        let id = self.pool.len();

        // First, recursively intern parents
        let mapping = match &source_info.mapping {
            SourceMapping::Original { file_id } => {
                SerializableSourceMapping::Original { file_id: *file_id }
            }
            SourceMapping::Substring { parent, offset } => {
                let parent_id = self.intern(parent); // Recurse
                SerializableSourceMapping::Substring {
                    parent_id,
                    offset: *offset,
                }
            }
            SourceMapping::Transformed { parent, mapping } => {
                let parent_id = self.intern(parent); // Recurse
                SerializableSourceMapping::Transformed {
                    parent_id,
                    mapping: mapping.clone(),
                }
            }
            SourceMapping::Concat { pieces } => {
                let serializable_pieces = pieces
                    .iter()
                    .map(|piece| SerializableSourcePiece {
                        source_info_id: self.intern(&piece.source_info),
                        offset_in_concat: piece.offset_in_concat,
                        length: piece.length,
                    })
                    .collect();
                SerializableSourceMapping::Concat {
                    pieces: serializable_pieces,
                }
            }
        };

        // Add to pool
        self.pool.push(SerializableSourceInfo {
            id,
            range: source_info.range.clone(),
            mapping,
        });

        self.id_map.insert(ptr, id);
        id
    }

    fn serialize_source_info(&mut self, source_info: &SourceInfo) -> serde_json::Value {
        let id = self.intern(source_info);
        json!({"$ref": id})
    }
}
```

### Phase 2: Traverse AST and Collect SourceInfos

```rust
fn write_pandoc(pandoc: &Pandoc, context: &ASTContext) -> Value {
    let mut serializer = SourceInfoSerializer::new();

    // Walk the AST and intern all SourceInfo objects
    intern_from_meta(&pandoc.meta, &mut serializer);
    intern_from_blocks(&pandoc.blocks, &mut serializer);

    // Build the JSON with $ref instead of full SourceInfo
    let meta_json = write_meta_with_refs(&pandoc.meta, &mut serializer);
    let blocks_json = write_blocks_with_refs(&pandoc.blocks, &mut serializer);

    // Add pool to astContext
    let mut ast_context_obj = serde_json::Map::new();
    ast_context_obj.insert("filenames".to_string(), json!(context.filenames));
    if !serializer.pool.is_empty() {
        ast_context_obj.insert("sourceInfoPool".to_string(), json!(serializer.pool));
    }

    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": meta_json,
        "blocks": blocks_json,
        "astContext": ast_context_obj,
    })
}
```

### Phase 3: Custom Deserialization (Reader)

```rust
// In readers/json.rs

struct SourceInfoDeserializer {
    pool: Vec<SourceInfo>,
}

impl SourceInfoDeserializer {
    fn new(pool_json: &Value) -> Result<Self> {
        let pool_array = pool_json.as_array()
            .ok_or_else(|| JsonReadError::InvalidType("sourceInfoPool must be array"))?;

        let mut pool = Vec::with_capacity(pool_array.len());

        // Build pool, resolving parent_id references
        for item in pool_array {
            let id = item["id"].as_u64().unwrap() as usize;
            let range: Range = serde_json::from_value(item["range"].clone())?;

            let mapping = match &item["mapping"] {
                Value::Object(obj) if obj.contains_key("Original") => {
                    let file_id = serde_json::from_value(obj["Original"]["file_id"].clone())?;
                    SourceMapping::Original { file_id }
                }
                Value::Object(obj) if obj.contains_key("Substring") => {
                    let parent_id = obj["Substring"]["parent_id"].as_u64().unwrap() as usize;
                    let offset = obj["Substring"]["offset"].as_u64().unwrap() as usize;

                    // Parent must already be in pool (topological order)
                    let parent = pool[parent_id].clone();
                    SourceMapping::Substring {
                        parent: Rc::new(parent),
                        offset,
                    }
                }
                // ... similar for Transformed and Concat
            };

            pool.push(SourceInfo { range, mapping });
        }

        Ok(SourceInfoDeserializer { pool })
    }

    fn deserialize_source_info(&self, value: &Value) -> Result<SourceInfo> {
        if let Some(ref_id) = value.get("$ref").and_then(|v| v.as_u64()) {
            let id = ref_id as usize;
            self.pool.get(id)
                .cloned()
                .ok_or_else(|| JsonReadError::InvalidRef(id))
        } else {
            Err(JsonReadError::InvalidType("Expected $ref".to_string()))
        }
    }
}
```

## Alternative Approaches (Considered and Rejected)

### Alternative 1: Flatten to Absolute Positions

**Idea**: Store only `(file_id, offset)` for each SourceInfo, losing transformation history.

**Pros**: Smallest possible size
**Cons**:
- ❌ Loses transformation chain information
- ❌ Can't reconstruct parent relationships
- ❌ Makes debugging harder (can't see YAML → Meta → Markdown path)

**Verdict**: Too much information loss

### Alternative 2: Rely on Compression

**Idea**: Accept the duplication, use gzip/brotli compression

**Pros**: No code changes
**Cons**:
- ❌ Still large uncompressed (bad for debugging)
- ❌ Compression/decompression overhead
- ❌ Not always available (e.g., in-memory WASM)
- ❌ Wastes network bandwidth

**Verdict**: Doesn't solve the root problem

### Alternative 3: Different Serialization Format

**Idea**: Use MessagePack, CBOR, or custom binary format

**Pros**: More compact than JSON
**Cons**:
- ❌ Breaks Pandoc compatibility
- ❌ Less debuggable (binary instead of text)
- ❌ Still has duplication problem (just smaller bytes)

**Verdict**: Doesn't address duplication, adds complexity

## Migration Strategy

**Key Insight**: All source location information is internal to our crate. The Pandoc-standard parts of the JSON (meta, blocks) are non-negotiable, but `astContext` is entirely our own design. We can change the SourceInfo serialization format without backward compatibility concerns.

### Step 1: Implement New Serialization

Replace the current inline SourceInfo serialization with pool-based approach:

```rust
fn write_pandoc(pandoc: &Pandoc, context: &ASTContext) -> Value {
    let mut serializer = SourceInfoSerializer::new();

    // Walk AST and build pool
    let meta_json = write_meta_with_refs(&pandoc.meta, &mut serializer);
    let blocks_json = write_blocks_with_refs(&pandoc.blocks, &mut serializer);

    // Write pool to astContext
    let mut ast_context_obj = serde_json::Map::new();
    ast_context_obj.insert("filenames".to_string(), json!(context.filenames));
    if !serializer.pool.is_empty() {
        ast_context_obj.insert("sourceInfoPool".to_string(), json!(serializer.pool));
    }

    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": meta_json,
        "blocks": blocks_json,
        "astContext": ast_context_obj,
    })
}
```

### Step 2: Update Reader

```rust
fn read_pandoc(value: &Value) -> Result<(Pandoc, ASTContext)> {
    // Extract sourceInfoPool from astContext
    let deserializer = if let Some(pool_json) = value
        .get("astContext")
        .and_then(|ctx| ctx.get("sourceInfoPool"))
    {
        Some(SourceInfoDeserializer::new(pool_json)?)
    } else {
        None
    };

    let meta = read_meta(
        value.get("meta").ok_or(...)?,
        deserializer.as_ref(),
    )?;

    let blocks = read_blocks(
        value.get("blocks").ok_or(...)?,
        deserializer.as_ref(),
    )?;

    // ...
}
```

### Step 3: Test New Format

```rust
#[test]
fn test_roundtrip_with_pool() {
    let (pandoc, context) = readers::qmd::read(...);

    // Serialize with pool
    let mut json_output = Vec::new();
    writers::json::write(&pandoc, &context, &mut json_output).unwrap();

    // Deserialize
    let mut json_reader = std::io::Cursor::new(&json_output);
    let (pandoc2, _) = readers::json::read(&mut json_reader).unwrap();

    // Verify structure preserved
    assert_ast_equal(&pandoc, &pandoc2);

    // Verify pool was created and is compact
    let json_value: serde_json::Value = serde_json::from_slice(&json_output).unwrap();
    let pool = &json_value["astContext"]["sourceInfoPool"];
    assert!(pool.is_array());
    println!("Pool size: {} entries", pool.as_array().unwrap().len());
}
```

### Step 4: Update Snapshots

Regenerate all JSON snapshots with the new format:

```bash
cargo test -p quarto-markdown-pandoc -- --ignored
# Review changes, then accept
```

## Performance Considerations

### Serialization Cost

**Building the pool**: O(N) where N = unique SourceInfo objects
- Each SourceInfo visited once
- HashMap lookup is O(1)
- Total: Linear in AST size

**Writing references**: O(M) where M = total SourceInfo references
- Just write `{"$ref": id}`
- Very fast

**Overall**: Should be faster than current approach (no recursive serialization)

### Deserialization Cost

**Building pool**: O(N)
- Process each pool entry once
- Parent references already in pool (topological order)

**Resolving references**: O(M)
- Array lookup is O(1)
- Clone the Rc (just increment refcount)

**Overall**: Similar speed to current approach, possibly faster

### Memory Overhead

**Writer**: HashMap<*const SourceInfo, usize>
- Pointer = 8 bytes
- usize = 8 bytes
- ~16 bytes per unique SourceInfo
- For 1000 unique SourceInfos: ~16KB

**Reader**: Vec<SourceInfo>
- Temporary during deserialization
- Dropped after AST is built
- For 1000 SourceInfos with Rc: ~100KB

**Verdict**: Negligible overhead

## Testing Plan

### Unit Tests

1. `test_source_info_pool_basic` - Simple Original
2. `test_source_info_pool_substring` - Substring with parent
3. `test_source_info_pool_nested` - Deep nesting (5+ levels)
4. `test_source_info_pool_siblings` - Many siblings sharing parent
5. `test_source_info_pool_concat` - Concat mapping
6. `test_source_info_pool_transformed` - Transformed mapping

### Integration Tests

1. Re-run all existing JSON roundtrip tests with new format
2. Compare sizes: old vs new format
3. Verify snapshots still match (after updating)

### Benchmarks

```rust
#[bench]
fn bench_serialize_large_document(b: &mut Bencher) {
    let qmd = generate_large_qmd(1000); // 1000 metadata nodes
    let (pandoc, context) = readers::qmd::read(...);

    b.iter(|| {
        let mut output = Vec::new();
        writers::json::write(&pandoc, &context, &mut output).unwrap();
        output
    });
}
```

## Risks and Mitigations

### Risk 1: Pointer Instability

**Problem**: Using `*const SourceInfo` as HashMap key assumes pointers don't change

**Mitigation**:
- SourceInfo is never moved during serialization (only borrowed)
- Rc keeps objects stable in memory
- If this becomes an issue, use ID generation instead of pointer addresses

### Risk 2: Topological Order

**Problem**: Deserialization requires parents before children

**Mitigation**:
- Serializer naturally produces topological order (depth-first traversal)
- Add assertion in deserializer to catch violations
- Could add explicit topological sort if needed

### Risk 3: Breaking Changes

**Problem**: Changes JSON format

**Mitigation**:
- Keep backward compatibility (read both formats)
- Feature flag for gradual rollout
- Clear versioning in astContext

## Future Optimizations

### 1. Range Pooling

Ranges are also duplicated. Could pool them too:

```json
{
  "rangePool": [
    {"id": 0, "start": {...}, "end": {...}}
  ],
  "sourceInfoPool": [
    {"id": 0, "range_id": 0, "mapping": {...}}
  ]
}
```

Expected additional savings: 10-20%

### 2. Compression-Aware Format

For network transfer, combine with gzip:
- Pool already makes compression more effective
- Could further optimize field names (`p` instead of `parent_id`)

### 3. Binary Format

For WASM boundaries, could use binary encoding:
- Pool as fixed-size array
- References as u32 indices
- Even more compact than JSON

## Recommendation

**Proceed with pool-based serialization (Proposed Solution)**

**Why**:
1. ✅ Solves the core problem (93% size reduction)
2. ✅ Maintains all information
3. ✅ Reasonable implementation complexity
4. ✅ Backward compatible migration path
5. ✅ No performance regression
6. ✅ Enables future optimizations

**Timeline Estimate**:
- Phase 1 (Writer): 2-3 days
- Phase 2 (Reader): 2-3 days
- Phase 3 (Testing & Snapshots): 1-2 days

**Total**: ~1 week of focused work

## Simplified Approach (No Feature Flags)

Since all SourceInfo serialization is internal to our crate:
- ✅ No backward compatibility needed
- ✅ No gradual rollout needed
- ✅ Just implement, test, and ship
- ✅ Update snapshots to reflect new format

The Pandoc-standard JSON format (meta, blocks) remains untouched, so we maintain full Pandoc compatibility where it matters.

## Open Questions for Discussion

1. Should range pooling be part of this PR or a follow-up?
2. Should we add a version field to astContext for future format changes?
