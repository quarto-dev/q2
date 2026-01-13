# Plan: Fix JSON Serialization Ordering with Typed Structs

**STATUS: COMPLETED**

## Problem Summary

The `unit_test_snapshots_json` test in the `pampa` crate is failing because JSON field ordering changed from alphabetical to insertion order.

**Root Cause**: Commit `3f138f7` added `deno_core`, which enables `serde_json`'s `preserve_order` feature workspace-wide. This changed `serde_json::Map` from `BTreeMap` (alphabetical) to `IndexMap` (insertion order).

**Before**: `{"c":"This","s":0,"t":"Str"}` (alphabetical)
**After**: `{"t":"Str","c":"This","s":0}` (insertion order)

## Solution Approach

Define intermediate structs with `#[derive(Serialize)]` where field declaration order matches the expected alphabetical output. Serde serializes struct fields in declaration order, giving us compile-time ordering guarantees.

## Implementation Plan

### Step 1: Add struct definitions (DONE)

Added to `crates/pampa/src/writers/json.rs`:
- `PandocDocumentJson` - top-level document
- `AstContextJson` - AST context with source pool
- `FileEntryJson` - file entries
- `SourceInfoJson` - source info pool entries
- `NodeJson` - generic node with c, s, t fields
- `AttrSourceJson` - attribute source info
- `NodeWithAttrJson` - node with attribute source

### Step 2: Update SerializableSourceInfo (DONE)

Added `to_json()` method to convert `SerializableSourceInfo` to `SourceInfoJson`.

### Step 3: Update write_pandoc (DONE)

Modified to use `PandocDocumentJson` struct instead of `json!` macro.

### Step 4: Update node_with_source (DONE)

Updated to use `NodeJson` struct for deterministic field ordering.

### Step 5: Fix remaining serializers (DONE)

- Updated `write_attr_source` to use `AttrSourceJson` struct
- Updated `write_config_value` to use `meta_node` helper for alphabetical ordering
- Updated `write_config_value_as_meta` to sort keys
- Updated `metaTopLevelKeySources` to sort keys
- Fixed Div block serialization to use alphabetical field order

### Step 6: Run tests and iterate (DONE)

All 2966 pampa tests pass.

## Files to Modify

1. `crates/pampa/src/writers/json.rs`:
   - Add new struct definitions at the top ✓
   - Add conversion method for `SerializableSourceInfo`
   - Update `write_pandoc` to construct `PandocDocumentJson`
   - Update individual node writers as needed

## Verification

1. `cargo nextest run -p pampa unit_test_snapshots_json` - should pass
2. `cargo nextest run -p pampa test_error_corpus_json_snapshots` - verify passes
3. `cargo nextest run -p pampa` - full test suite should pass

## Why This Approach

**Pros**:
- Compile-time ordering guarantees (no runtime overhead)
- Self-documenting: struct fields show expected JSON structure
- Type-safe: compiler catches missing fields
- Explicit about the contract with consumers

**Cons**:
- More boilerplate (struct definitions)
- Need to keep struct field order aligned with expected output
