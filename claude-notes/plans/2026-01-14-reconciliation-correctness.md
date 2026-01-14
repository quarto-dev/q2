# Reconciliation Correctness Plan

**Issue:** kyoto-72j
**Date:** 2026-01-14
**Status:** Phase 3 Complete - Bug Fixed

## Fundamental Property

The reconciliation algorithm must satisfy:

```
For any pair of Pandoc values "before" and "after":
  let after2 = after.clone()
  let plan = compute_plan(before, after)
  let result = apply_plan(before, after, plan)
  assert_structural_equality(result, after2)
```

Where `structural_equality` compares AST structure while ignoring source location metadata.

## Property Testing Strategy

Following the pattern from `2025-12-16-property-testing-commonmark-subset.md`, we use **proptest** with **feature sets** and **progressive complexity levels**.

### Core Property Test

```rust
proptest! {
    #[test]
    fn reconciliation_preserves_after_structure(
        before in gen_pandoc_ast(features),
        after in gen_pandoc_ast(features),
    ) {
        let after_clone = after.clone();
        let plan = compute_reconciliation(&before, &after);
        let result = apply_reconciliation(before, after, &plan);

        prop_assert!(
            structural_eq_ignoring_source(&result, &after_clone),
            "Result should be structurally equal to 'after'"
        );
    }
}
```

### Feature Sets for Generation

```rust
#[derive(Clone, Default)]
struct ReconcileBlockFeatures {
    paragraph: bool,
    header: bool,
    code_block: bool,
    blockquote: bool,
    bullet_list: bool,
    ordered_list: bool,
    div: bool,
    // Add more as needed
}

#[derive(Clone, Default)]
struct ReconcileInlineFeatures {
    str_: bool,
    emph: bool,
    strong: bool,
    code: bool,
    link: bool,
    // Add more as needed
}
```

### Progressive Complexity Levels

**Block Progression:**

| Level | Name | Features | Tests |
|-------|------|----------|-------|
| B0 | `SINGLE_PARA` | Single Paragraph | Same/different content |
| B1 | `MULTI_PARA` | Multiple Paragraphs | Same/fewer/more blocks |
| B2 | `WITH_HEADER` | + Header | Mixed block types |
| B3 | `WITH_CODE` | + CodeBlock | Leaf blocks |
| B4 | `WITH_BLOCKQUOTE` | + BlockQuote | Nested containers |
| B5 | `WITH_LISTS` | + BulletList, OrderedList | **Critical: length changes** |
| B6 | `WITH_DIV` | + Div | Nested containers |
| B7 | `FULL_BLOCKS` | All blocks | Full complexity |

**Inline Progression:**

| Level | Name | Features |
|-------|------|----------|
| I0 | `PLAIN_TEXT` | Str, Space only |
| I1 | `WITH_FORMATTING` | + Emph, Strong |
| I2 | `WITH_CODE` | + Code |
| I3 | `WITH_LINKS` | + Link, Image |
| I4 | `FULL_INLINES` | All inlines |

### Structural Equality Function

```rust
/// Compare two Pandoc ASTs ignoring source_info fields
fn structural_eq_ignoring_source(a: &Pandoc, b: &Pandoc) -> bool {
    structural_eq_blocks(&a.blocks, &b.blocks)
}

fn structural_eq_blocks(a: &[Block], b: &[Block]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| structural_eq_block(x, y))
}

fn structural_eq_block(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Paragraph(pa), Block::Paragraph(pb)) => {
            structural_eq_inlines(&pa.content, &pb.content)
        }
        (Block::BulletList(la), Block::BulletList(lb)) => {
            if la.content.len() != lb.content.len() {
                return false;  // Length must match!
            }
            la.content.iter().zip(&lb.content)
                .all(|(ia, ib)| structural_eq_blocks(ia, ib))
        }
        // ... other block types
        _ => false,
    }
}
```

### Test Scenarios for Lists

The list bug is the primary motivation, so we specifically test:

```rust
#[test]
fn list_same_length() {
    // before: [a, b, c], after: [a, b, c] -> result: [a, b, c]
}

#[test]
fn list_item_removed() {
    // before: [a, b, c], after: [a, c] -> result: [a, c] (2 items, not 3!)
}

#[test]
fn list_item_added() {
    // before: [a], after: [a, b, c] -> result: [a, b, c] (3 items, not 1!)
}

#[test]
fn list_all_items_removed() {
    // before: [a, b, c], after: [] -> result: [] (empty!)
}

#[test]
fn list_from_empty() {
    // before: [], after: [a, b, c] -> result: [a, b, c]
}
```

## Current Bugs

### 1. List Length Mismatch (Critical)

**Location:** `apply_list_reconciliation` in `crates/quarto-pandoc-types/src/reconcile/apply.rs:141-151`

**Problem:** Returns `orig_items` which preserves original's length:

```rust
fn apply_list_reconciliation(
    mut orig_items: Vec<Vec<Block>>,
    exec_items: Vec<Vec<Block>>,
    plan: &ReconciliationPlan,
) -> Vec<Vec<Block>> {
    for (orig_item, exec_item) in orig_items.iter_mut().zip(exec_items.into_iter()) {
        *orig_item = apply_reconciliation_to_blocks(std::mem::take(orig_item), exec_item, plan);
    }
    orig_items  // BUG: wrong length!
}
```

**Impact:**
- Executed has fewer items → result has extra items from original
- Executed has more items → new items are dropped

### 2. Plan Doesn't Capture List Item Operations

**Location:** `compute_list_plan` in `crates/quarto-pandoc-types/src/reconcile/compute.rs:241-260`

**Problem:** Only computes stats, doesn't generate `block_alignments`:

```rust
fn compute_list_plan<'a>(...) -> ReconciliationPlan {
    let mut plan = ReconciliationPlan::new();

    // Pairwise matching - doesn't generate alignments!
    for (orig_item, exec_item) in orig_items.iter().zip(exec_items) {
        let nested = compute_reconciliation_for_blocks(orig_item, exec_item, cache);
        plan.stats.merge(&nested.stats);
    }

    // Just counts extras, doesn't track them
    if exec_items.len() > orig_items.len() {
        plan.stats.blocks_replaced += exec_items.len() - orig_items.len();
    }

    plan  // Empty block_alignments!
}
```

### 3. DefinitionList Has Same Pattern

Similar issues exist in definition list handling.

## Design Questions

### Q1: What Should List Reconciliation Do?

**Option A: Simple positional matching** (current attempt, broken)
- Match item 0 with item 0, item 1 with item 1, etc.
- Extra items from executed are appended
- Fewer items in executed means truncation
- Pros: Simple, predictable
- Cons: Poor source location preservation when items are inserted/deleted in the middle

**Option B: Hash-based matching** (like top-level blocks)
- Use structural hashes to match list items across positions
- Better preserves source locations for reordered/inserted/deleted items
- Pros: Better source preservation
- Cons: More complex, may have surprising behavior

**Option C: LCS-based matching**
- Use longest common subsequence to align items
- Best for detecting insertions/deletions
- Pros: Most accurate alignment
- Cons: O(n²) complexity

### Q2: Should Plans Be Inspectable/Debuggable?

Currently, list plans have empty `block_alignments`, making them impossible to debug. The plan should capture what operations will be performed so they can be inspected.

## Proposed Fix

### Phase 1: Fix the Bug (Minimal)

1. **Fix `apply_list_reconciliation`** to return the correct length:

```rust
fn apply_list_reconciliation(
    orig_items: Vec<Vec<Block>>,
    exec_items: Vec<Vec<Block>>,
    plan: &ReconciliationPlan,
) -> Vec<Vec<Block>> {
    let mut result = Vec::with_capacity(exec_items.len());

    for (i, exec_item) in exec_items.into_iter().enumerate() {
        if let Some(orig_item) = orig_items.get(i).cloned() {
            // Reconcile matching positions
            result.push(apply_reconciliation_to_blocks(orig_item, exec_item, plan));
        } else {
            // Extra item from executed - use as-is
            result.push(exec_item);
        }
    }

    result  // Correct length!
}
```

2. **Add structural equality test** as a property-based invariant

### Phase 2: Improve Plan Quality (Optional)

1. Generate proper `block_alignments` for list items
2. Consider hash-based or LCS matching for better source preservation

## Testing Strategy

### Required Test

```rust
#[test]
fn reconciliation_produces_structural_equality_to_after() {
    // Property: for any before/after, result is structurally equal to after

    fn structural_eq_blocks(a: &[Block], b: &[Block]) -> bool {
        // Compare ignoring source_info
    }

    // Test cases:
    // 1. Same length lists
    // 2. Executed has fewer items
    // 3. Executed has more items
    // 4. Empty lists
    // 5. Nested containers
    // 6. Mixed changes
}
```

### Specific Bug Reproduction Test

```rust
#[test]
fn list_item_removal_preserves_executed_length() {
    // before: [one, two, three]
    // after: [one, three]
    // result should have 2 items, not 3
}

#[test]
fn list_item_addition_includes_new_items() {
    // before: [one]
    // after: [one, two, three]
    // result should have 3 items, not 1
}
```

## Implementation Steps

### Phase 1: Property Testing Infrastructure ✅ COMPLETED

1. [x] Add `proptest` to `quarto-pandoc-types/Cargo.toml` dev-dependencies
2. [x] Create `src/reconcile/generators.rs` - feature sets and AST generators
3. [x] Create `src/reconcile/structural_eq.rs` - used existing `structural_eq_*` in hash.rs
4. [x] Create first property test at B0/I0 level (single paragraph, plain text)
5. [x] Verify test passes with current implementation

### Phase 2: Find the Bug with Property Tests ✅ COMPLETED

6. [x] Add B5 level (lists) to generators
7. [x] Run property tests - exposed the bug (even same-length lists failed!)
8. [x] Discovered root cause: `compute_list_plan` discards nested plans (only merges stats)

### Phase 3: Fix the Bug ✅ COMPLETED

**Root Cause:** The bug was worse than initially thought. Not just list length was wrong, but list items became EMPTY because:
- `compute_list_plan` only merged stats, threw away actual alignments
- `apply_list_reconciliation` called `apply_reconciliation_to_blocks` with empty plan
- Empty plan → empty block_alignments → empty result vector

**Fix Applied:**
1. Added `list_item_plans: Vec<ReconciliationPlan>` field to `ReconciliationPlan` (types.rs:139-143)
2. Modified `compute_list_plan` to store per-item plans in `list_item_plans` (compute.rs:240-266)
3. Modified `apply_list_reconciliation` to use per-item plans and handle length differences (apply.rs:140-167)

9. [x] Write targeted unit tests for list length changes (5 tests in `list_length_tests` module)
10. [x] Fix `apply_list_reconciliation` to return correct length
11. [ ] Fix `apply_definition_list_reconciliation` (same pattern) - NOT YET DONE
12. [x] Verify property tests now pass (304 tests pass)

### Phase 4: Full Coverage

13. [ ] Incrementally enable all complexity levels (B0→B7, I0→I4)
14. [ ] Run extended property test campaigns
15. [ ] Document any discovered edge cases

### Phase 5: Optional Improvements

16. [x] `compute_list_plan` now stores per-item plans (enables debuggability)
17. [ ] Consider hash-based or LCS matching for better source preservation
18. [ ] Update reconcile-viewer to show list operations

## References

- Original design: `claude-notes/plans/2025-12-17-structural-hash-reconciliation-design.md`
- Problem statement: `claude-notes/plans/2025-12-15-engine-output-source-location-reconciliation.md`
- Bug location: `crates/quarto-pandoc-types/src/reconcile/apply.rs:141-151`
