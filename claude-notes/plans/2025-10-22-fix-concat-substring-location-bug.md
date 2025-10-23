# Fix Plan: Concat/Substring End Location Bug

**Date**: 2025-10-22
**Issue**: k-135
**Root Issue**: k-129 (discovered by location health tests)

## Root Cause Analysis

### The Bug

In `crates/quarto-source-map/src/source_info.rs`, the `concat()` and `substring()` functions create SourceInfo with ranges where the **end location has row=0, column=0** instead of computed values.

**Concat function** (lines 135-147):
```rust
SourceInfo {
    range: Range {
        start: Location { offset: 0, row: 0, column: 0 },
        end: Location {
            offset: total_length,
            row: 0,           // BUG!
            column: 0,        // BUG!
        },
    },
    ...
}
```

**Substring function** (lines 92-103):
```rust
range: Range {
    start: Location { offset: 0, row: 0, column: 0 },
    end: Location {
        offset: length,
        row: 0,           // BUG!
        column: 0,        // BUG!
    },
},
```

### Why This Is Wrong

The problem is conceptual: `Concat` and `Substring` SourceInfos represent **transformed content**, where:
- The `range` field describes positions in the transformed/derived text
- The `mapping` field tracks how to map back to the original source

However, our location health tests validate that:
1. `offset_to_location(offset)` should return the correct (row, col) for that offset
2. `line_col_to_offset(row, col)` should return the correct offset
3. These should be inverses

When we pass the **original source text** to these validation functions, but the `range` describes **transformed text**, we get mismatches.

### The Deeper Issue

The real question is: **What text does the range describe?**

For `Original` mappings:
- Range describes position in the original source file
- row/column can be computed from offset using the original source text

For `Concat`/`Substring` mappings:
- Range describes position in a **virtual/transformed text**
- We don't have that text available!
- We only have pieces/lengths, not actual transformed content

### Options

**Option 1**: Ranges in Concat/Substring should have proper row/column
- Problem: We don't have the transformed text to compute from!
- We'd need to pass the actual concatenated/substring text to these functions

**Option 2**: Ranges in Concat/Substring can have (0, 0) for row/column
- This is the current behavior
- But then our validation tests need to understand this is OK
- Health tests would need to skip checking Concat/Substring SourceInfos

**Option 3**: Don't use Concat/Substring - use Original instead
- When combining SourceInfos, just expand the range in the original source
- This only works if the combined content is contiguous in the original

## Investigation Needed

Before deciding on a fix, we need to answer:

1. **When are Concat SourceInfos created?**
   - Found: `SourceInfo::combine()` method uses `concat()`
   - Need to find all callers of `combine()`

2. **Are the pieces actually contiguous in the original source?**
   - If yes, we could use an Original mapping with expanded range
   - If no, we truly need Concat

3. **Do we ever need the row/column of Concat/Substring ranges?**
   - Or is offset sufficient?
   - Is anyone calling `offset_to_location` on these?

## Proposed Investigation Steps

1. Find all callers of `SourceInfo::combine()` in quarto-markdown-pandoc
2. Examine a specific case from 002.qmd
   - What SourceInfos are being combined?
   - Are they contiguous in the original source?
3. Determine if we can replace Concat with Original (expanded range)
4. If not, decide whether to:
   - Option A: Pass transformed text to concat/substring
   - Option B: Mark Concat/Substring as special in health tests
   - Option C: Compute row/column from offset using dummy line counting

## Recommended Approach

**After investigation, likely fix:**

1. For `combine()`, if the two SourceInfos are:
   - Both Original mappings
   - Same file_id
   - Contiguous or have calculable gap
   → Use Original mapping with combined range

2. If they're not contiguous or different files:
   → Use Concat (current behavior)

3. For Concat/Substring, when we don't have transformed text:
   → Compute row/column by treating content as single-line
   → offset=N means row=0, column=N
   → This makes the ranges internally consistent

This way:
- Original mappings have correct row/column from source
- Concat/Substring have row/column that match their offset (single-line assumption)
- Health tests can validate consistency within each range

## Next Step

Run investigation queries to understand usage patterns before implementing fix.
