# HashMap to LinkedHashMap Migration Plan

**Beads Issue:** k-318  
**Date:** 2025-11-03  
**Goal:** Replace HashMap with LinkedHashMap in Attr type for deterministic attribute ordering

## Problem Statement

The `Attr` type in `src/pandoc/attr.rs` is defined as:
```rust
pub type Attr = (String, Vec<String>, HashMap<String, String>);
```

The `HashMap<String, String>` does not preserve insertion order, causing non-deterministic output in tests like 030.qmd where shortcode attributes appear in different orders on different test runs.

Example from test 030.qmd output variations:
- Run 1: `[("data-value", ...), ("data-raw", ...), ("data-is-shortcode", ...)]`
- Run 2: `[("data-raw", ...), ("data-is-shortcode", ...), ("data-value", ...)]`

## Root Cause Analysis

The issue originates in `src/pandoc/shortcode.rs` where attributes are inserted in order:
```rust
let mut attr_hash = HashMap::new();
attr_hash.insert("data-raw".to_string(), str.clone());      // Inserted 1st
attr_hash.insert("data-value".to_string(), str);            // Inserted 2nd  
attr_hash.insert("data-is-shortcode".to_string(), "1");     // Inserted 3rd
```

But HashMap doesn't guarantee iteration order matches insertion order.

## Solution

The codebase already successfully uses `hashlink::LinkedHashMap` in:
- `src/pandoc/meta.rs` - For MetaMap preservation
- `src/writers/qmd.rs` - For YAML map ordering
- Dependency already in Cargo.toml: `hashlink = { version = "0.10.0", features = ["serde_impl"] }`

LinkedHashMap provides:
- O(1) lookup like HashMap
- Preserves insertion order for deterministic iteration
- Serde support for serialization
- No performance penalty for our use case

## Files Requiring Changes

### 1. Core Type Definition
**src/pandoc/attr.rs** (3 changes)
- Line 8: Import change
- Line 14: Type definition change
- Line 11: empty_attr() change

### 2. Direct Attr HashMap Construction (10 files)
All files creating `HashMap::new()` for attrs:

1. **src/pandoc/shortcode.rs** - Critical for test 030 fix
2. **src/pandoc/treesitter.rs**
3. **src/pandoc/treesitter_utils/inline_link.rs**
4. **src/pandoc/treesitter_utils/uri_autolink.rs**
5. **src/pandoc/treesitter_utils/shortcode.rs**
6. **src/pandoc/treesitter_utils/commonmark_attribute.rs**
7. **src/pandoc/treesitter_utils/editorial_marks.rs**
8. **src/pandoc/treesitter_utils/image.rs**
9. **src/pandoc/treesitter_utils/code_span.rs**
10. **src/pandoc/treesitter_utils/code_span_helpers.rs** (if applicable)

## Implementation Steps

### Step 1: Update Core Definition
File: `src/pandoc/attr.rs`

Change:
```rust
use std::collections::HashMap;

pub type Attr = (String, Vec<String>, HashMap<String, String>);

pub fn empty_attr() -> Attr {
    ("".to_string(), vec![], HashMap::new())
}
```

To:
```rust
use hashlink::LinkedHashMap;

pub type Attr = (String, Vec<String>, LinkedHashMap<String, String>);

pub fn empty_attr() -> Attr {
    ("".to_string(), vec![], LinkedHashMap::new())
}
```

### Step 2: Update All Attr Construction Sites
For each of the 10 files listed above:

Change:
```rust
use std::collections::HashMap;
// ...
let mut attr = ("".to_string(), vec![], HashMap::new());
```

To:
```rust
use hashlink::LinkedHashMap;
// ...
let mut attr = ("".to_string(), vec![], LinkedHashMap::new());
```

Pattern to search for: `HashMap::new()` in attr construction contexts

### Step 3: Verification
1. Build: `cargo build --bin quarto-markdown-pandoc`
2. Test 030 determinism: Run `cargo test unit_test_snapshots_native` 3-5 times
3. Verify insertion order preserved in output
4. Full test suite: `cargo test --package quarto-markdown-pandoc`
5. Accept any snapshot updates if ordering changed but is now stable

## Expected Outcomes

After migration:
- ✅ Shortcode attributes appear in consistent insertion order
- ✅ Test 030.qmd produces deterministic output
- ✅ All attribute key-value pairs maintain insertion order
- ✅ No functional changes to behavior
- ✅ No performance degradation
- ✅ Serialization remains compatible (via serde_impl)

## Risks & Mitigations

**Risk:** Breaking existing code that depends on HashMap
**Mitigation:** LinkedHashMap is a drop-in replacement with same API

**Risk:** Performance impact
**Mitigation:** LinkedHashMap has same O(1) lookup, minimal overhead for small maps (typical attr size)

**Risk:** Snapshot test changes
**Mitigation:** May need to accept new snapshots if ordering changed, but new order will be deterministic

## Notes

- The comment on line 49 of shortcode.rs mentions "this needs to be fixed and needs to use the actual source" - this migration doesn't address that TODO but sets foundation for it
- Other HashMap uses in the codebase (e.g., SourceInfoSerializer in writers/json.rs) don't need ordering and can stay as HashMap
- Focus only on Attr-related HashMap usage
