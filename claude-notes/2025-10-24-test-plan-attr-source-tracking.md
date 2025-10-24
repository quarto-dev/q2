# Test Plan: Attr/Target Source Location Tracking

**Date**: 2025-10-24
**Status**: Phase 1 Complete ✅

## Overview

Testing strategy for the new `attr_source`, `target_source`, and `id_source` fields added to Pandoc AST types to enable precise source location tracking for tuple-based structures.

## Test Philosophy

**Core Principle**: *Tests should be simpler than the code they test*

**Strategy**:
1. **Tiny examples** - Single feature per test
2. **Visual verification** - Humans can check correctness by inspection
3. **Property tests** - Roundtrip should preserve structure
4. **Progressive complexity** - Build from simple to complex
5. **Fail fast** - First failure should pinpoint exact problem

## Test Phases

### ✅ Phase 1: Structure Tests (COMPLETE)

**Goal**: Verify Rust types compile and have correct shape

**Status**: 26/26 tests passing

**Coverage**:
- AttrSourceInfo structure (4 tests)
- TargetSourceInfo structure (3 tests)
- Inline types with attr_source (4 types × 2 tests = 8 tests)
- Block types with attr_source (4 types × 1 test = 4 tests)
- Table components with attr_source (5 types × 1 test = 5 tests)
- Citation with id_source (2 tests)
- Nested table complexity (1 test)
- Enum pattern matching (2 tests)

**Key Files**:
- `tests/test_attr_source_structure.rs` - All structure tests

**Result**: ✅ All 15 affected types verified to compile correctly

### 🚧 Phase 2A: Parser Unit Tests (TODO)

**Goal**: Test individual attr/target parsing in isolation

**Test Cases** (not yet implemented):
```rust
test_parse_attr_simple_id()         // Input: "{#my-id}"
test_parse_attr_empty_id()          // Input: "{.class}"
test_parse_attr_multiple_classes()  // Input: "{.c1 .c2 .c3}"
test_parse_attr_with_attributes()   // Input: "{k1=v1 k2=v2}"
test_parse_attr_with_emoji()        // Input: "{#emoji-😀 .class-🎉}"
test_parse_target_with_title()      // Input: "(url \"title\")"
test_parse_target_no_title()        // Input: "(url)"
test_parse_citation_id()            // Input: "@knuth84"
```

**Note**: These tests cannot be implemented until parser changes are made to actually populate the source tracking fields.

### 🚧 Phase 2B: Integration Tests (TODO)

**Goal**: Test complete constructs end-to-end

**Test Cases**:
```rust
test_span_with_full_attr()          // [text]{#id .c1 .c2 k1=v1 k2=v2}
test_link_with_attr_and_target()    // [text](url "title"){.class}
test_image_with_full_annotations()  // ![alt](img.png "title"){#fig}
test_header_with_attr()             // # Header {#custom-id}
test_code_block_with_attr()         // ```python {.class}
```

### 🚧 Phase 3: JSON Serialization Tests (TODO)

**Goal**: Verify correct JSON output with `attrS`, `targetS`, `citationIdS` fields

**Test Cases**:
```rust
test_serialize_attr_source_empty_id()       // null for empty id
test_serialize_attr_source_mirrors_attr()   // Structure matching
test_serialize_target_source()              // targetS array format
test_serialize_citation_id_source()         // citationIdS field
```

**Note**: Cannot implement until JSON writer is updated to output these fields.

### 🚧 Phase 4: JSON Deserialization Tests (TODO)

**Goal**: Verify reading JSON with new fields, including backward compatibility

**Test Cases**:
```rust
test_deserialize_with_attr_source()         // New JSON format
test_deserialize_without_attr_source()      // Backward compatibility
test_deserialize_null_handling()            // null → None conversion
```

### 🚧 Phase 5: Roundtrip Property Tests (TODO)

**Goal**: Verify qmd → AST → JSON → AST preserves structure

**Test Cases**:
```rust
prop_roundtrip_preserves_structure()        // QuickCheck property test
test_golden_files()                         // Known-good reference files
```

### 🚧 Phase 6: Complexity Tests (TODO)

**Goal**: Test deeply nested structures (especially tables)

**Test Cases**:
```rust
test_table_nested_attr_source()             // All table components
test_table_with_20_cells()                  // Performance/memory
test_mixed_nested_structures()              // Divs with tables with figures
```

## Bug Risk Areas

### High-Risk
1. **Off-by-one errors**: Including/excluding delimiters
2. **UTF-8 handling**: Multi-byte characters in identifiers
3. **Escaping**: Quoted strings with escape sequences
4. **Null vs empty**: `""` → `None` vs `Some("")` confusion

### Medium-Risk
5. **Whitespace normalization**: `{ .class }` vs `{.class}`
6. **Structure mirroring**: `attrS` must exactly match `attr` shape
7. **Nested complexity**: Tables with many attr_source fields

### Lower-Risk
8. **Backward compatibility**: Old JSON without new fields
9. **Memory efficiency**: Large documents with many attrs

## Current Status Summary

### ✅ Completed Work

**Rust AST Changes**:
- Added `AttrSourceInfo` struct with `empty()` method
- Added `TargetSourceInfo` struct with `empty()` method
- Added `attr_source` field to 14 types
- Added `target_source` field to Link and Image
- Added `id_source` field to Citation
- Fixed all compilation errors (62 total)
- All existing tests pass

**Test Infrastructure**:
- Created comprehensive Phase 1 structure tests (26 tests)
- All tests passing
- Test file: `tests/test_attr_source_structure.rs`

### 🚧 Remaining Work

**Parser** (Not Started):
- Track Attr component source locations during parsing
- Track Target component source locations
- Track Citation id source locations
- Handle edge cases (empty strings, special characters, UTF-8)

**JSON Writer** (Not Started):
- Add `attrS` field serialization
- Add `targetS` field serialization
- Add `citationIdS` field serialization
- Handle null values correctly

**JSON Reader** (Not Started):
- Add `attrS` field deserialization
- Add `targetS` field deserialization
- Add `citationIdS` field deserialization
- Maintain backward compatibility

**Tests** (Phase 2-6 Not Started):
- Parser unit tests
- Integration tests
- JSON serialization tests
- JSON deserialization tests
- Roundtrip tests
- Complexity tests
- Golden file infrastructure

## Test Organization

```
tests/
├── test_attr_source_structure.rs         ✅ Phase 1 (26 tests passing)
├── test_attr_parsing.rs                  🚧 Phase 2A (TODO)
├── test_target_parsing.rs                🚧 Phase 2A (TODO)
├── test_citation_parsing.rs              🚧 Phase 2A (TODO)
├── test_integration_parsing.rs           🚧 Phase 2B (TODO)
├── test_json_serialization_attrs.rs      🚧 Phase 3 (TODO)
├── test_json_deserialization_attrs.rs    🚧 Phase 4 (TODO)
├── test_roundtrip_attrs.rs               🚧 Phase 5 (TODO)
├── test_table_complexity.rs              🚧 Phase 6 (TODO)
└── golden/                               🚧 TODO: Create directory
    ├── simple_span.qmd
    ├── simple_span.json
    ├── complex_link.qmd
    ├── complex_link.json
    ├── nested_table.qmd
    └── nested_table.json
```

## Next Steps

1. **Decide on implementation order**:
   - Option A: Implement parser changes first, then add Phase 2 tests
   - Option B: Implement JSON writer first, then add Phase 3 tests
   - Recommendation: Parser first (more foundational)

2. **Start with simplest case**:
   - Pick `Span` with `{#id}` as first implementation
   - Single feature, easy to verify
   - Serves as template for other types

3. **Create golden file infrastructure**:
   - Set up `tests/golden/` directory
   - Create simple reference examples
   - Document expected JSON format

4. **Incremental development**:
   - One type at a time
   - Test each before moving on
   - Keep changes small and reviewable

## Success Criteria

### MVP (Minimum Viable Product)
- ✅ All types compile with new fields
- ⬜ Parse single Span `{#id .class}` with correct offsets
- ⬜ Serialize to JSON with `attrS` field
- ⬜ Deserialize back and verify structure
- ⬜ Visual inspection: offsets match expected positions

### Production Ready
- ⬜ All 14 types tested individually
- ⬜ Golden files for complex examples
- ⬜ Roundtrip tests pass
- ⬜ Backward compatibility verified
- ⬜ Performance acceptable (large tables)
- ⬜ Phase 1-6 tests all passing

## References

- Design doc: `claude-notes/plans/2025-10-24-attr-target-sideloading.md`
- Related issue: k-161 (recursive type annotation problem)
- Test file: `tests/test_attr_source_structure.rs`
