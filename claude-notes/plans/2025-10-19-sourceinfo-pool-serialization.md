# SourceInfo Pool-Based Serialization Implementation Plan

**Date**: 2025-10-19
**Issue**: k-44
**Design**: See ../sourceinfo-serialization-optimization-design.md

## Goal

Reduce SourceInfo JSON serialization size from 25-55x blowup to ~2-5x blowup by using an interned pool with ID references instead of duplicating parent chains.

## Implementation Tasks

### Phase 1: Data Structures and Serialization Logic

#### Task 1.1: Define Serializable Types

Create serializable versions of SourceInfo structures that use IDs instead of Rc pointers.

**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

**Add**:
```rust
#[derive(Serialize)]
struct SerializableSourceInfo {
    id: usize,
    range: Range,
    mapping: SerializableSourceMapping,
}

#[derive(Serialize)]
#[serde(tag = "t", content = "c")]
enum SerializableSourceMapping {
    Original { file_id: FileId },
    Substring { parent_id: usize, offset: usize },
    Concat { pieces: Vec<SerializableSourcePiece> },
    Transformed { parent_id: usize, mapping: Vec<RangeMapping> },
}

#[derive(Serialize)]
struct SerializableSourcePiece {
    source_info_id: usize,
    offset_in_concat: usize,
    length: usize,
}
```

#### Task 1.2: Implement SourceInfoSerializer

Create the pool builder that interns SourceInfo objects.

**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

**Add**:
```rust
use std::collections::HashMap;

struct SourceInfoSerializer {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
}

impl SourceInfoSerializer {
    fn new() -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            id_map: HashMap::new(),
        }
    }

    fn intern(&mut self, source_info: &SourceInfo) -> usize {
        // Implementation from design doc
    }

    fn to_json_ref(&mut self, source_info: &SourceInfo) -> serde_json::Value {
        let id = self.intern(source_info);
        json!({"$ref": id})
    }
}
```

#### Task 1.3: Update write_* Functions

Modify all write functions to accept and use SourceInfoSerializer.

**Files**:
- `write_meta()` → `write_meta(meta, serializer)`
- `write_meta_value_with_source_info()` → add serializer param
- `write_blocks()` → `write_blocks(blocks, serializer)`
- `write_block()` → add serializer param
- `write_inlines()` → add serializer param
- `write_inline()` → add serializer param

**Pattern**:
```rust
// Old
fn write_meta(meta: &MetaValueWithSourceInfo) -> Value {
    // ...
    "key_source": json!(entry.key_source),  // Inlines full SourceInfo
}

// New
fn write_meta(meta: &MetaValueWithSourceInfo, serializer: &mut SourceInfoSerializer) -> Value {
    // ...
    "key_source": serializer.to_json_ref(&entry.key_source),  // Just {"$ref": N}
}
```

#### Task 1.4: Update write_pandoc

Update the top-level writer to create serializer and add pool to astContext.

**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

**Modify**:
```rust
fn write_pandoc(pandoc: &Pandoc, context: &ASTContext) -> Value {
    let mut serializer = SourceInfoSerializer::new();

    // Serialize AST with refs
    let meta_json = write_meta(&pandoc.meta, &mut serializer);
    let blocks_json = write_blocks(&pandoc.blocks, &mut serializer);

    // Build astContext with pool
    let mut ast_context_obj = serde_json::Map::new();
    ast_context_obj.insert("filenames".to_string(), json!(context.filenames));

    // Only include pool if non-empty
    if !serializer.pool.is_empty() {
        ast_context_obj.insert("sourceInfoPool".to_string(), json!(serializer.pool));
    }

    // Include metaTopLevelKeySources if non-empty
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
        let key_sources: serde_json::Map<String, Value> = entries
            .iter()
            .map(|e| (e.key.clone(), serializer.to_json_ref(&e.key_source)))
            .collect();
        if !key_sources.is_empty() {
            ast_context_obj.insert("metaTopLevelKeySources".to_string(), Value::Object(key_sources));
        }
    }

    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": write_meta_to_pandoc_format(&pandoc.meta),
        "blocks": blocks_json,
        "astContext": ast_context_obj,
    })
}
```

### Phase 2: Deserialization Logic

#### Task 2.1: Implement SourceInfoDeserializer

Create the pool reader that reconstructs SourceInfo from IDs.

**File**: `crates/quarto-markdown-pandoc/src/readers/json.rs`

**Add**:
```rust
struct SourceInfoDeserializer {
    pool: Vec<SourceInfo>,
}

impl SourceInfoDeserializer {
    fn new(pool_json: &Value) -> Result<Self> {
        // Build pool in topological order
        // Parent IDs must be < current ID
    }

    fn get(&self, id: usize) -> Result<SourceInfo> {
        self.pool.get(id)
            .cloned()
            .ok_or_else(|| JsonReadError::InvalidSourceInfoRef(id))
    }

    fn from_json_ref(&self, value: &Value) -> Result<SourceInfo> {
        if let Some(id) = value.get("$ref").and_then(|v| v.as_u64()) {
            self.get(id as usize)
        } else {
            Err(JsonReadError::ExpectedSourceInfoRef)
        }
    }
}
```

#### Task 2.2: Update read_* Functions

Modify all read functions to accept and use SourceInfoDeserializer.

**Files**:
- `read_meta()` → `read_meta(value, deserializer)`
- `read_meta_value_with_source_info()` → add deserializer param
- `read_blocks()` → add deserializer param
- `read_block()` → add deserializer param
- `read_inlines()` → add deserializer param
- `read_inline()` → add deserializer param

**Pattern**:
```rust
// Old
fn read_meta(value: &Value) -> Result<MetaValueWithSourceInfo> {
    // ...
    key_source: serde_json::from_value(entry["key_source"].clone())?,
}

// New
fn read_meta(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<MetaValueWithSourceInfo> {
    // ...
    key_source: deserializer.from_json_ref(&entry["key_source"])?,
}
```

#### Task 2.3: Update read_pandoc

Update the top-level reader to extract pool and create deserializer.

**File**: `crates/quarto-markdown-pandoc/src/readers/json.rs`

**Modify**:
```rust
fn read_pandoc(value: &Value) -> Result<(Pandoc, ASTContext)> {
    let obj = value.as_object().ok_or(...)?;

    // Extract and build pool
    let deserializer = if let Some(pool_json) = obj.get("astContext")
        .and_then(|ctx| ctx.as_object())
        .and_then(|ctx| ctx.get("sourceInfoPool"))
    {
        SourceInfoDeserializer::new(pool_json)?
    } else {
        // Empty pool for documents without SourceInfo
        SourceInfoDeserializer::empty()
    };

    // Read with deserializer
    let meta = read_meta_with_key_sources(
        obj.get("meta").ok_or(...)?,
        obj.get("astContext")
            .and_then(|ctx| ctx.as_object())
            .and_then(|ctx| ctx.get("metaTopLevelKeySources")),
        &deserializer,
    )?;

    let blocks = read_blocks(
        obj.get("blocks").ok_or(...)?,
        &deserializer,
    )?;

    // ...
}
```

### Phase 3: Error Handling

#### Task 3.1: Add New Error Types

**File**: `crates/quarto-markdown-pandoc/src/readers/json.rs`

**Add to JsonReadError**:
```rust
#[derive(Debug)]
pub enum JsonReadError {
    // ... existing variants
    InvalidSourceInfoRef(usize),
    ExpectedSourceInfoRef,
    MalformedSourceInfoPool,
    CircularSourceInfoReference,
}
```

### Phase 4: Testing

#### Task 4.1: Add Unit Tests for Serializer

**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs` (in tests module)

Tests:
- `test_source_info_pool_original` - Single Original
- `test_source_info_pool_substring` - Substring with parent
- `test_source_info_pool_siblings` - Multiple nodes sharing parent
- `test_source_info_pool_nested_deep` - 5+ level nesting
- `test_source_info_pool_concat` - Concat mapping
- `test_source_info_pool_deduplication` - Verify same Rc gets same ID

#### Task 4.2: Add Unit Tests for Deserializer

**File**: `crates/quarto-markdown-pandoc/src/readers/json.rs` (in tests module)

Tests:
- `test_deserialize_source_info_pool_basic`
- `test_deserialize_source_info_pool_with_refs`
- `test_deserialize_invalid_ref` - Error handling
- `test_deserialize_circular_ref` - Error handling

#### Task 4.3: Update Roundtrip Tests

**File**: `crates/quarto-markdown-pandoc/tests/test_json_roundtrip.rs`

Verify all existing roundtrip tests still pass with new format.

#### Task 4.4: Update Metadata Source Tracking Test

**File**: `crates/quarto-markdown-pandoc/tests/test_metadata_source_tracking.rs`

Verify the test still passes - SourceInfo should be preserved through roundtrip.

#### Task 4.5: Update Nested YAML Serialization Tests

**File**: `crates/quarto-markdown-pandoc/tests/test_nested_yaml_serialization.rs`

Add assertions to verify pool size and measure size reduction:

```rust
#[test]
fn test_binary_tree_serialization_with_pool() {
    for depth in 1..=6 {
        let qmd_content = generate_binary_tree_yaml(depth);

        // Parse and serialize
        let (pandoc, context) = readers::qmd::read(...)?;
        let mut json_output = Vec::new();
        writers::json::write(&pandoc, &context, &mut json_output)?;

        // Parse JSON to verify pool
        let json_value: serde_json::Value = serde_json::from_slice(&json_output)?;
        let pool = &json_value["astContext"]["sourceInfoPool"];

        println!("Depth {}: Pool size {} entries, JSON {} bytes",
                 depth,
                 pool.as_array().map(|a| a.len()).unwrap_or(0),
                 json_output.len());

        // Verify roundtrip
        let (pandoc2, _) = readers::json::read(&mut Cursor::new(json_output))?;
        // Compare structures
    }
}
```

#### Task 4.6: Update Snapshots

Regenerate all JSON snapshot tests:

```bash
cargo test -p quarto-markdown-pandoc unit_test_snapshots_json -- --nocapture
# Review diffs
# Update snapshots in tests/snapshots/json/
```

### Phase 5: Documentation

#### Task 5.1: Update Design Doc

Mark the design as implemented and add any learnings.

#### Task 5.2: Add Code Comments

Document the serialization format in both writer and reader:

```rust
/// Serializes SourceInfo using a pool-based approach to avoid duplication.
///
/// Instead of embedding full parent chains in every SourceInfo, we maintain
/// a pool of unique SourceInfo objects and reference them by ID:
///
/// ```json
/// {
///   "astContext": {
///     "sourceInfoPool": [
///       {"id": 0, "mapping": {"Original": {...}}},
///       {"id": 1, "mapping": {"Substring": {"parent_id": 0, "offset": 10}}}
///     ]
///   }
/// }
/// ```
///
/// Then SourceInfo references become: `{"$ref": 1}`
///
/// This reduces JSON size by ~93% for documents with shared parent chains.
struct SourceInfoSerializer { /* ... */ }
```

## Testing Strategy

### Before Implementation
- [x] Measurements complete (test_nested_yaml_serialization.rs)
- [x] Design reviewed and approved

### During Implementation
- [ ] Each phase tested independently
- [ ] All existing tests pass after each phase
- [ ] New unit tests added for new functionality

### After Implementation
- [ ] All tests pass (including snapshots)
- [ ] Measure size reduction on test documents
- [ ] Verify no performance regression
- [ ] Document results

## Success Criteria

1. ✅ All existing tests pass
2. ✅ JSON roundtrip preserves SourceInfo correctly
3. ✅ JSON size reduced by >80% on sibling-heavy documents
4. ✅ No performance regression in serialization/deserialization
5. ✅ Snapshots updated and reviewed

## Rollout Plan

Since this is an internal format change:
1. Implement all phases
2. Run full test suite
3. Update snapshots
4. Merge - no gradual rollout needed

## Risk Mitigation

### Risk: Pointer stability
**Mitigation**: SourceInfo is never moved during serialization (only borrowed via &)

### Risk: Topological order violations
**Mitigation**: Depth-first traversal naturally produces topological order; add assertion in deserializer

### Risk: Breaking existing tests
**Mitigation**: Run tests after each phase; fix issues immediately

## Implementation Order

1. Phase 1.1 → 1.2 → 1.3 → 1.4 (Writer, test with snapshot inspection)
2. Phase 2.1 → 2.2 → 2.3 (Reader, test with roundtrips)
3. Phase 3 (Error handling)
4. Phase 4 (All tests)
5. Phase 5 (Documentation)

## Estimated Timeline

- Day 1: Phase 1 (Writer implementation)
- Day 2: Phase 1 testing, Phase 2 (Reader implementation)
- Day 3: Phase 2 testing, Phase 3 (Error handling)
- Day 4: Phase 4.1-4.4 (Unit tests)
- Day 5: Phase 4.5-4.6 (Integration tests, snapshots)
- Day 6: Phase 5 (Documentation, cleanup)

**Total**: ~6 days focused work
