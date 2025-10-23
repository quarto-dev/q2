# Concat/Substring Semantics Analysis

## The Question

Should we fix `combine()` or fix `concat()`? What should row/column be for derived text?

## Two Interpretations

### Interpretation 1: Ranges describe ORIGINAL source positions

In this view:
- `concat()` should NOT be used when combining contiguous original text
- Use `Original` mapping with expanded range instead
- `concat()` is only for truly non-contiguous pieces or different files
- Row/column should always refer to original source positions

**Pros:**
- All ranges can be validated against original source
- Simpler mental model: ranges = positions in original file
- Health tests work without special cases

**Cons:**
- `concat()` exists but shouldn't be used for common case
- Wastes code (concat implementation)

### Interpretation 2: Ranges describe DERIVED text positions

In this view:
- `concat()` creates a NEW virtual text by concatenating pieces
- The range describes positions in that VIRTUAL text
- Row/column should be computed for that virtual text
- Original source mapping is in the `mapping` field

**Pros:**
- Clean separation: `range` = virtual text, `mapping` = original source
- More general: can handle truly non-contiguous pieces
- `concat()` makes sense as a concept

**Cons:**
- Without actual virtual text, how do we compute row/column?
- Health tests would need to understand this distinction
- More complex to reason about

## The Reality Check

Looking at our actual use case in 002.qmd:
- "There" + "'" + "s" are CONTIGUOUS in the original source
- They're from the SAME original file at ADJACENT positions
- The "virtual text" is literally identical to a contiguous span in original!

**Key insight**: In practice, `combine()` is being used to combine contiguous pieces, not create truly virtual text.

## What Should We Do?

### Option A: Fix combine() to avoid concat for contiguous Original mappings

```rust
pub fn combine(&self, other: &SourceInfo) -> SourceInfo {
    // If both Original from same file, create expanded Original
    if let (SourceMapping::Original { file_id: f1 },
            SourceMapping::Original { file_id: f2 }) = (&self.mapping, &other.mapping) {
        if f1 == f2 {
            return SourceInfo::original(*f1, Range {
                start: min(self.range.start, other.range.start),
                end: max(self.range.end, other.range.end),
            });
        }
    }
    // Fall back to concat for other cases
    ...
}
```

**Result**: Concat rarely/never used, ranges always refer to original source.

### Option B: Fix concat() to compute row/column properly

```rust
pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self {
    let total_length = cumulative_offset;

    // Compute row/column assuming single-line virtual text
    // (without actual text, this is the best we can do)
    SourceInfo {
        range: Range {
            start: Location { offset: 0, row: 0, column: 0 },
            end: Location {
                offset: total_length,
                row: 0,
                column: total_length,  // Make it consistent!
            },
        },
        ...
    }
}
```

**Result**: Concat ranges are internally consistent (row=0, col=offset), but can't be validated against original source.

### Option C: Fix concat() to infer from pieces when possible

```rust
pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self {
    // If all pieces are Original from same file and contiguous,
    // compute actual row/column from their ranges
    if all_original_same_file_contiguous(&pieces) {
        let start = pieces.first().range.start;
        let end = pieces.last().range.end;
        return SourceInfo::original(file_id, Range { start, end });
    }

    // Otherwise, single-line assumption
    ...
}
```

**Result**: Best of both worlds, but adds complexity to concat().

## My Recommendation

**Option A** (fix `combine()`) because:

1. It matches actual usage - we're combining contiguous original text
2. Simpler - let `concat()` keep current semantics for rare true-concat cases
3. Health tests work without modification
4. Clearer intent: "combining contiguous text" vs "creating virtual text"

The bug isn't really in `concat()` - it's that `combine()` is calling `concat()` for a case where `concat()` isn't appropriate.
