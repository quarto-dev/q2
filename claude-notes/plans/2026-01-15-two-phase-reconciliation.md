# Three-Phase Reconciliation Algorithm

## Implementation Status

| Phase | Status | Notes |
|-------|--------|-------|
| **A: Block Matching** | ✅ COMPLETE | Implemented and tested |
| **B: List Items** | 🔲 TODO | Not started |

### What Was Done (Phase A)

1. **Rewrote `compute_reconciliation_for_blocks`** in `compute.rs` (lines 34-213) to use three-phase algorithm
2. **Added 6 new tests** in `compute.rs` (search for `test_three_phase_`):
   - `test_three_phase_exact_match_at_different_position` - the ex6 pattern
   - `test_three_phase_positional_match_when_no_exact`
   - `test_three_phase_no_recurse_when_positional_already_used`
   - `test_three_phase_type_mismatch_at_position_uses_after`
   - `test_three_phase_multiple_exact_matches_at_shifted_positions`
   - `test_three_phase_exact_match_priority_over_positional`
3. **All 193 reconcile tests pass**
4. **Verified ex6 example** produces correct output via `reconcile-viewer`

### What Remains (Phase B)

1. Add hash functions for `Vec<Block>` in `hash.rs`
2. Add `structural_eq_blocks` in `hash.rs`
3. Rewrite `compute_list_plan` to use three-phase matching
4. Update `apply_list_reconciliation` if needed
5. Add tests for list item matching (ex1 pattern)

---

## Problem Statement

The current reconciliation algorithm for matching elements between two vectors of blocks has a flaw: it processes blocks in a single pass, greedily assigning matches. This causes suboptimal alignments when blocks are inserted at the beginning or middle of a sequence.

### Current Algorithm (Single-Pass Greedy)

For each executed block in order:
1. Try exact hash match → `KeepBefore`
2. If no exact match, try type-based container matching → `RecurseIntoContainer`
3. If no container match, try type-based inline content matching → `RecurseIntoContainer`
4. Fallback → `UseAfter`

### The Problem

Consider this example (ex6):

**Before (outer Div contains):**
```
::: {}
1
:::

::: {}
2
:::

::: {}
3
:::
```

**After (outer Div contains):**
```
::: {}
0
:::

::: {}
1
:::

::: {}
2
:::
```

Current algorithm behavior:
1. `after[0]` (Div with "0"): No exact hash match. Type match finds `before[0]` (Div with "1") → RecurseIntoContainer(0, 0)
2. `after[1]` (Div with "1"): Exact hash match would be `before[0]`, but it's already used! Falls back to type match with `before[1]` → RecurseIntoContainer(1, 1)
3. `after[2]` (Div with "2"): Same problem → RecurseIntoContainer(2, 2)

**Result:** All three matches are `RecurseIntoContainer` with misaligned pairs. No source locations are preserved.

**Ideal behavior:**
1. `after[1]` should match `before[0]` (exact hash match) → `KeepBefore(0)`
2. `after[2]` should match `before[1]` (exact hash match) → `KeepBefore(1)`
3. `after[0]` has no exact match, pairs with remaining `before[2]` → `RecurseIntoContainer(2, 0)`

**Result:** Source locations for "1" and "2" divs are preserved.

### Why ex4 Works But ex6 Doesn't

In ex4, the paragraphs "1", "2", "3" are direct children of a Div. The algorithm processes them and finds exact hash matches because no earlier paragraph "steals" a later one via type matching.

In ex6, the problem is that `after[0]` (the NEW element) comes first in iteration order. It has no exact match, so it immediately falls back to type matching and claims `before[0]`. This "steals" the original that `after[1]` should have matched exactly.

### List Items Have the Same Problem

Lists (`Vec<Vec<Block>>`) use purely positional matching—item 0 vs item 0, etc. They don't even attempt hash matching for items.

**Before list:**
```
- 1
- 2
- 3
```

**After list:**
```
- 0
- 1
- 2
```

Current algorithm pairs: (0↔0), (1↔1), (2↔2), resulting in all mismatches.

Ideal: after[1]↔before[0], after[2]↔before[1], after[0]↔new

---

## Proposed Solution: Three-Phase Matching

The key insight: **recursion is a claim that two containers represent the same logical entity**. We should only make this claim when we have evidence:

1. **Hash match** = definitive proof (content identical)
2. **Position match** = reasonable evidence (same index in parent)
3. **Neither** = no evidence, don't recurse

### Phase 1: Exact Hash Matches (Any Position)

Find exact hash matches anywhere in the original array. Position mismatch is fine because we have **proof** the content is identical.

```
for each (exec_idx, exec_block) in executed:
    hash = compute_hash(exec_block)
    if unused_original_with_hash(hash):
        alignment[exec_idx] = KeepBefore(orig_idx)
        mark orig_idx as used
    else:
        mark exec_idx as "needs phase 2"
```

### Phase 2: Positional Type Matches (Same Index Only)

For unmatched executed blocks, check if the **positionally corresponding** original is available and has the same type. Only recurse when indices match—don't hunt for other originals.

```
for each exec_idx in "needs phase 2":
    exec_block = executed[exec_idx]
    if exec_idx < original.len()
       AND original[exec_idx] is unused
       AND same_type(original[exec_idx], exec_block):
        alignment[exec_idx] = RecurseIntoContainer(exec_idx, exec_idx)
        mark exec_idx as used in original
    else:
        mark exec_idx as "needs phase 3"
```

### Phase 3: Fallback (New Content)

For still-unmatched executed blocks, use the executed version as-is. Don't try to recurse into arbitrarily-positioned originals.

```
for each exec_idx in "needs phase 3":
    alignment[exec_idx] = UseAfter(exec_idx)
```

### Why Not Recurse Into Misaligned Containers?

When we recurse into `before[j]` and `after[i]` where `j ≠ i`, we're claiming the user performed this edit:
- "Move container from position j to position i"
- "Then modify its contents"

This is an unlikely edit pattern. The more parsimonious interpretation:
- "Container at position i is new"
- "Container at position j is deleted"

Recursing into misaligned containers "tries too hard" to preserve source locations for content that isn't actually related.

### Trace Through ex6 with Three-Phase

**Phase 1 (exact matches):**
- `after[0]` (Div "0"): hash has no match → needs phase 2
- `after[1]` (Div "1"): hash matches `before[0]` → `KeepBefore(0)`, mark before[0] used
- `after[2]` (Div "2"): hash matches `before[1]` → `KeepBefore(1)`, mark before[1] used

**Phase 2 (positional matches):**
- `after[0]`: Is `before[0]` unused? **NO** (claimed in phase 1) → needs phase 3

**Phase 3 (fallback):**
- `after[0]`: `UseAfter(0)` — treat as new content

**Final alignments:**
```
alignments[0] = UseAfter(0)       // Div "0" is NEW
alignments[1] = KeepBefore(0)     // Div "1" exact match, source preserved
alignments[2] = KeepBefore(1)     // Div "2" exact match, source preserved
```

**Interpretation:** "Div '0' appeared at the beginning. Divs '1' and '2' were preserved (shifted down). Div '3' disappeared."

This is much cleaner than claiming "Div '3' moved to position 0 and had its content replaced with '0'."

---

## Implementation Plan

### Phase A: Fix Block Matching (ex6 case)

**File:** `crates/quarto-pandoc-types/src/reconcile/compute.rs`

**Function:** `compute_reconciliation_for_blocks`

**Changes:**

1. Split the main loop into three passes:

```rust
// Phase 1: Exact hash matches only (any position)
let mut alignments: Vec<Option<BlockAlignment>> = vec![None; executed.len()];
let mut needs_phase_2: Vec<usize> = Vec::new();

for (exec_idx, exec_block) in executed.iter().enumerate() {
    let exec_hash = compute_block_hash_fresh(exec_block);

    if let Some(indices) = hash_to_indices.get(&exec_hash)
        && let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i))
        && structural_eq_block(&original[orig_idx], exec_block)
    {
        used_original.insert(orig_idx);
        alignments[exec_idx] = Some(BlockAlignment::KeepBefore(orig_idx));
        stats.blocks_kept += 1;
    } else {
        needs_phase_2.push(exec_idx);
    }
}

// Phase 2: Positional type matches (same index only)
let mut needs_phase_3: Vec<usize> = Vec::new();

for exec_idx in needs_phase_2 {
    let exec_block = &executed[exec_idx];

    // Only try positional match: before[exec_idx] with after[exec_idx]
    if exec_idx < original.len()
        && !used_original.contains(&exec_idx)
        && same_type_and_should_recurse(&original[exec_idx], exec_block)
    {
        used_original.insert(exec_idx);
        // Compute nested plan and store in appropriate map
        let nested_plan = compute_nested_plan(&original[exec_idx], exec_block, cache);
        // ... store in block_container_plans, inline_plans, etc.
        alignments[exec_idx] = Some(BlockAlignment::RecurseIntoContainer {
            before_idx: exec_idx,
            after_idx: exec_idx,
        });
        stats.blocks_recursed += 1;
    } else {
        needs_phase_3.push(exec_idx);
    }
}

// Phase 3: Fallback - treat as new content
for exec_idx in needs_phase_3 {
    alignments[exec_idx] = Some(BlockAlignment::UseAfter(exec_idx));
    stats.blocks_replaced += 1;
}
```

2. Helper function `same_type_and_should_recurse` checks:
   - Same discriminant (both Div, both Paragraph, etc.)
   - Is a container block OR has inline content
   - For Custom blocks: also check type_name matches

3. Convert `alignments` from `Vec<Option<BlockAlignment>>` to `Vec<BlockAlignment>` at the end.

**Key behavioral change:** In phase 2, we **only** try `before[exec_idx]` for `after[exec_idx]`. We don't search for other unused originals of the same type. If the positional original is already used or wrong type, we fall through to UseAfter.

**Test cases to add:**
- ex6 pattern: nested containers where inner elements have exact matches at different indices
- Verify that exact matches are prioritized over positional matches
- Verify that misaligned containers don't recurse (UseAfter instead)

### Phase B: Extend to List Items

**Problem:** List items are `Vec<Block>`, not `Block`. We need to hash them and apply the same three-phase approach.

**Changes:**

1. **Add hash function for `Vec<Block>`** in `hash.rs`:

```rust
pub fn compute_blocks_hash(blocks: &[Block], cache: &mut HashCache) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write_usize(blocks.len());
    for block in blocks {
        hasher.write_u64(cache.hash_block(block));
    }
    hasher.finish()
}

pub fn compute_blocks_hash_fresh(blocks: &[Block]) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write_usize(blocks.len());
    for block in blocks {
        hasher.write_u64(compute_block_hash_fresh(block));
    }
    hasher.finish()
}
```

2. **Add structural equality for `Vec<Block>`** in `hash.rs`:

```rust
pub fn structural_eq_blocks(a: &[Block], b: &[Block]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(a, b)| structural_eq_block(a, b))
}
```

3. **Rewrite `compute_list_plan`** to use three-phase matching:

```rust
fn compute_list_plan<'a>(
    orig_items: &'a [Vec<Block>],
    exec_items: &[Vec<Block>],
    cache: &mut HashCache<'a>,
) -> ReconciliationPlan {
    // Build hash → indices map for original items
    let orig_hashes: Vec<u64> = orig_items
        .iter()
        .map(|item| compute_blocks_hash(item, cache))
        .collect();

    let mut hash_to_indices: LinkedHashMap<u64, Vec<usize>> = LinkedHashMap::new();
    for (idx, &hash) in orig_hashes.iter().enumerate() {
        hash_to_indices.entry(hash).or_default().push(idx);
    }

    let mut used_original: FxHashSet<usize> = FxHashSet::default();
    let mut list_item_plans: Vec<ReconciliationPlan> = vec![ReconciliationPlan::new(); exec_items.len()];
    let mut item_sources: Vec<ListItemSource> = vec![ListItemSource::New; exec_items.len()];
    let mut needs_phase_2: Vec<usize> = Vec::new();

    // Phase 1: Exact hash matches (any position)
    for (exec_idx, exec_item) in exec_items.iter().enumerate() {
        let exec_hash = compute_blocks_hash_fresh(exec_item);

        if let Some(indices) = hash_to_indices.get(&exec_hash)
            && let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i))
            && structural_eq_blocks(&orig_items[orig_idx], exec_item)
        {
            used_original.insert(orig_idx);
            item_sources[exec_idx] = ListItemSource::Keep(orig_idx);
            // Plan is empty - content is identical, just use original
        } else {
            needs_phase_2.push(exec_idx);
        }
    }

    // Phase 2: Positional matches (same index only)
    let mut needs_phase_3: Vec<usize> = Vec::new();

    for exec_idx in needs_phase_2 {
        let exec_item = &exec_items[exec_idx];

        // Only try positional match: orig_items[exec_idx] with exec_items[exec_idx]
        if exec_idx < orig_items.len() && !used_original.contains(&exec_idx) {
            used_original.insert(exec_idx);
            // Recurse into the item to reconcile its blocks
            let nested_plan = compute_reconciliation_for_blocks(
                &orig_items[exec_idx],
                exec_item,
                cache,
            );
            item_sources[exec_idx] = ListItemSource::Reconcile(exec_idx);
            list_item_plans[exec_idx] = nested_plan;
        } else {
            needs_phase_3.push(exec_idx);
        }
    }

    // Phase 3: Fallback - treat as new content
    for exec_idx in needs_phase_3 {
        item_sources[exec_idx] = ListItemSource::New;
        // Plan stays empty - use executed item as-is
    }

    // ... convert to final plan format
}

enum ListItemSource {
    Keep(usize),      // Exact match with orig_items[usize]
    Reconcile(usize), // Positional match, recurse into orig_items[usize]
    New,              // No match, use executed item as-is
}
```

**Key behavioral change:** In phase 2, we **only** try `orig_items[exec_idx]` for `exec_items[exec_idx]`. We don't search for other unused originals. If the positional original is already used, we fall through to treating it as new.

4. **Update `apply_list_reconciliation`** to handle the new alignment types.

**Test cases to add:**
- ex1 pattern: list with item inserted at beginning
- List with item inserted in middle
- List with items reordered (should NOT try to track them)
- Verify source locations are preserved for unchanged items
- Verify positional matches recurse correctly when content changes but position doesn't

---

## Summary

| Phase | Scope | Key Change |
|-------|-------|------------|
| A | Block matching | Three-pass: exact → positional → fallback |
| B | List items | Hash `Vec<Block>`, apply three-pass to list items |

### The Three-Phase Algorithm

| Phase | Condition | Action | Rationale |
|-------|-----------|--------|-----------|
| 1 | Hash matches (any position) | `KeepBefore` | Definitive proof content is identical |
| 2 | Same position + same type | `RecurseIntoContainer` | Reasonable evidence they're related |
| 3 | Neither | `UseAfter` | No evidence, treat as new |

### Key Insight: Recursion Requires Evidence

Recursion is a claim that two containers represent the same logical entity. We should only make this claim when we have evidence:

1. **Hash match** = definitive proof → recurse (or rather, keep as-is)
2. **Position match** = reasonable evidence → recurse to find inner matches
3. **Neither** = no evidence → don't recurse, treat as new

The current algorithm "tries too hard" by recursing into any container of the same type, even when positions don't match. This creates implausible edit interpretations like "moved container from position j to i, then changed its content."

### Why This Matters

When we refuse to recurse into misaligned containers:
- We get cleaner semantics ("X is new, Y is gone" vs "Y moved and became X")
- We avoid propagating source locations from unrelated content
- We match user intuition about what editing operations occurred
