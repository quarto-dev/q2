# Fix Strategy for k-135

**Date**: 2025-10-22
**Issue**: k-135 - End locations have row=0,col=0

## Investigation Results

### Call Sites Analysis

Found only **2 call sites** for `SourceInfo::combine()` in production code:
1. `postprocess.rs:149` - Combining abbreviations like "Mr" + "."
2. `postprocess.rs:768` - Merging adjacent Str nodes

Both cases are combining **contiguous text from the same original source**.

### Current Behavior

`combine()` implementation (source_info.rs:183-191):
```rust
pub fn combine(&self, other: &SourceInfo) -> SourceInfo {
    let self_length = self.range.end.offset - self.range.start.offset;
    let other_length = other.range.end.offset - other.range.start.offset;

    SourceInfo::concat(vec![
        (self.clone(), self_length),
        (other.clone(), other_length),
    ])
}
```

This creates a Concat mapping, which then creates ranges with `row: 0, column: 0` for the end location.

## Fix Strategy

**Option chosen**: Improve `combine()` to avoid Concat when combining contiguous Original mappings.

### New `combine()` Logic

```rust
pub fn combine(&self, other: &SourceInfo) -> SourceInfo {
    // If both are Original mappings with the same file_id
    if let (SourceMapping::Original { file_id: file_id1 },
            SourceMapping::Original { file_id: file_id2 }) = (&self.mapping, &other.mapping) {
        if file_id1 == file_id2 {
            // Create a single Original mapping that spans from min(start) to max(end)
            let start = if self.range.start.offset < other.range.start.offset {
                self.range.start.clone()
            } else {
                other.range.start.clone()
            };

            let end = if self.range.end.offset > other.range.end.offset {
                self.range.end.clone()
            } else {
                other.range.end.clone()
            };

            return SourceInfo {
                range: Range { start, end },
                mapping: SourceMapping::Original { file_id: *file_id1 },
            };
        }
    }

    // Fall back to Concat for non-Original or different files
    let self_length = self.range.end.offset - self.range.start.offset;
    let other_length = other.range.end.offset - other.range.start.offset;

    SourceInfo::concat(vec![
        (self.clone(), self_length),
        (other.clone(), other_length),
    ])
}
```

### Why This Fixes the Bug

1. When combining "Mr" (offset 0-2, row 0, col 0-2) with "." (offset 2-3, row 0, col 2-3):
   - Both are Original mappings with FileId(0)
   - We create a new Original mapping: offset 0-3, row 0, col 0-3
   - **All row/column values are preserved from the original nodes!**

2. For non-Original or different files:
   - Falls back to Concat (current behavior)
   - This case doesn't happen in practice based on call site analysis

### Testing

The fix will make the location health tests pass because:
- Combined SourceInfos will be Original mappings
- Their ranges will have proper row/column from tree-sitter
- `offset_to_location` and `line_col_to_offset` will work correctly

## Implementation Plan

1. Modify `combine()` in `crates/quarto-source-map/src/source_info.rs`
2. Run location health tests to verify fix
3. Run full test suite to ensure no regressions
4. If tests pass, the fix is complete!

## Risk Assessment

**Low risk** because:
- Only 2 call sites in production code
- Both combine contiguous Original mappings
- New code path only activates for Original+Original with same file_id
- Fall back to existing behavior otherwise
- Location health tests will validate the fix

## Alternative Considered

Could also fix `concat()` to compute row/column assuming single-line content (row=0, column=offset). But the above approach is cleaner because it avoids Concat entirely when not needed.
