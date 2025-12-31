# Plan: Fix HashMap-induced Non-determinism

**Issue**: k-p39g
**Date**: 2025-12-31
**Status**: Investigation complete, awaiting review

## Problem Summary

Three tests are failing due to non-deterministic behavior:
1. `test_chicago_author_date_style` (citeproc integration test)
2. `test_bibliography_delimiters` (citeproc integration test)
3. `yaml-tags` (pampa snapshot test)

This codebase follows a convention of using `hashlink::LinkedHashMap` instead of `std::collections::HashMap` to ensure deterministic iteration order. However, several files have been using `HashMap` or `FxHashMap` where order could affect output.

## Findings

### 1. Citeproc Reference Struct (`crates/quarto-citeproc/src/reference.rs`)

**HIGH PRIORITY - Likely cause of citeproc test failures**

```rust
// Line 7
use std::collections::HashMap;

// Line 17 - name_hints used during disambiguation
pub name_hints: HashMap<String, NameHint>,

// Line 142 - CRITICAL: serialized with #[serde(flatten)]
#[serde(flatten)]
pub other: HashMap<String, serde_json::Value>,
```

The `other` field is particularly problematic because:
- It's serialized with `#[serde(flatten)]`
- When serialized to JSON, HashMap iteration order is undefined
- This affects bibliography output if any "other" fields are present in references

**Fix**: Change both to `LinkedHashMap` with appropriate serde configuration.

### 2. Citeproc Types (`crates/quarto-citeproc/src/types.rs`)

```rust
// Line 11
use std::collections::HashMap;

// Lines 260-265 - citation number tracking
initial_citation_numbers: HashMap<String, i32>,
final_citation_numbers: Option<HashMap<String, i32>>,

// Lines 283-290 - citation history tracking
last_item_in_note: HashMap<i32, (String, Option<String>, Option<String>)>,
last_cited: HashMap<String, (i32, Option<String>, Option<String>)>,
```

These are used for lookups, not iteration over output, so they're **lower priority**. However, for consistency and future-proofing, they should still be changed.

### 3. Reconciliation Types (`crates/quarto-pandoc-types/src/reconcile/types.rs`)

```rust
// Line 8
use rustc_hash::FxHashMap;

// Lines 25-30, 102-114, 125-137
pub block_slot_plans: FxHashMap<String, ReconciliationPlan>,
pub inline_slot_plans: FxHashMap<String, InlineReconciliationPlan>,
// ... and several others
```

These are used with `#[serde(skip_serializing_if = "FxHashMap::is_empty")]`. While the keys are usize indices (looked up from Vec alignments), if these are ever serialized, order matters.

**Fix**: Change to `LinkedHashMap` or use `IndexMap` for serialization.

### 4. Reconciliation Apply (`crates/quarto-pandoc-types/src/reconcile/apply.rs`)

```rust
// Line 18
use std::collections::HashMap;

// Line 361 - temporary lookup map
let mut orig_slots: HashMap<String, Slot> = orig.slots.into_iter().collect();
```

This is used only for lookups during slot reconciliation, but for consistency should be changed.

### 5. Reconciliation Hash (`crates/quarto-pandoc-types/src/reconcile/hash.rs`)

```rust
// Line 15
use rustc_hash::FxHashMap;

// Line 40 - hash cache
cache: FxHashMap<NodePtr, u64>,
```

This is used for memoization with pointer keys, order doesn't matter for output. **No change needed.**

### 6. JSON Writer (`crates/pampa/src/writers/json.rs`)

```rust
// Line 16
use std::collections::HashMap;

// Line 129 - pointer deduplication
id_map: HashMap<*const SourceInfo, usize>,
```

This is used for pointer-based deduplication. The IDs are assigned based on traversal order (deterministic), and lookup is by pointer. **No change needed** as long as traversal order is deterministic.

### 7. YAML Validation (`crates/quarto-yaml-validation/src/schema/types.rs`)

```rust
// Line 11
use std::collections::HashMap;

// Lines 61, 137-138
pub tags: Option<HashMap<String, serde_json::Value>>,
pub properties: HashMap<String, Schema>,
pub pattern_properties: HashMap<String, Schema>,
```

These are used in schema validation, and `properties`/`pattern_properties` could affect validation order or error message ordering.

## Proposed Fix Order

### Phase 1: Fix Likely Causes (citeproc)

1. **`crates/quarto-citeproc/src/reference.rs`**
   - Change `name_hints: HashMap<String, NameHint>` to `LinkedHashMap<String, NameHint>`
   - Change `other: HashMap<String, serde_json::Value>` to `LinkedHashMap<String, serde_json::Value>`
   - Update serde derives as needed

2. **`crates/quarto-citeproc/src/types.rs`**
   - Change `initial_citation_numbers`, `final_citation_numbers`, `last_item_in_note`, `last_cited` to `LinkedHashMap`

### Phase 2: Fix Reconciliation Module

3. **`crates/quarto-pandoc-types/src/reconcile/types.rs`**
   - Change all `FxHashMap` to `LinkedHashMap` or `IndexMap`
   - Update serde configuration

4. **`crates/quarto-pandoc-types/src/reconcile/apply.rs`**
   - Change local `HashMap` to `LinkedHashMap`

### Phase 3: Fix YAML Validation (if tests still fail)

5. **`crates/quarto-yaml-validation/src/schema/types.rs`**
   - Evaluate if HashMap usage affects output
   - Change to LinkedHashMap if needed

## Testing Plan

After each phase:
1. Run the three failing tests specifically:
   ```bash
   cargo nextest run test_chicago_author_date_style test_bibliography_delimiters
   cargo nextest run yaml-tags
   ```

2. Run multiple times to verify determinism:
   ```bash
   for i in {1..5}; do cargo nextest run test_chicago_author_date_style test_bibliography_delimiters yaml-tags; done
   ```

3. Run full test suite to check for regressions

## Notes

- `rustc_hash::FxHashMap` is used for performance (faster hashing) but has undefined iteration order
- `hashlink::LinkedHashMap` preserves insertion order at a small performance cost
- `indexmap::IndexMap` is another option that preserves order and is used by serde_json internally
- The json.rs `id_map` HashMap is safe because IDs are assigned sequentially during deterministic tree traversal
