# Plan: Remove source_info_to_range Function

**Date:** 2025-10-22
**Context:** Code review feedback - `source_info_to_range` is only used for extracting row numbers in `process_list`

## Current Situation

### What `source_info_to_range` Does

Located in `crates/quarto-markdown-pandoc/src/pandoc/source_map_compat.rs:122-138`:

```rust
pub fn source_info_to_range(source_info: &SourceInfo, ctx: &ASTContext)
    -> Option<crate::pandoc::location::Range>
{
    let start_mapped = source_info.map_offset(0, &ctx.source_context)?;
    let end_mapped = source_info.map_offset(source_info.length(), &ctx.source_context)?;

    Some(crate::pandoc::location::Range {
        start: crate::pandoc::location::Location {
            offset: start_mapped.location.offset,
            row: start_mapped.location.row,
            column: start_mapped.location.column,
        },
        end: crate::pandoc::location::Location {
            offset: end_mapped.location.offset,
            row: end_mapped.location.row,
            column: end_mapped.location.column,
        },
    })
}
```

**Purpose:** Converts `quarto_source_map::SourceInfo` → `pandoc::location::Range` by:
1. Mapping offset 0 (start) through the SourceInfo chain to get start row/column
2. Mapping offset `length()` (end) through the SourceInfo chain to get end row/column
3. Constructing a `pandoc::location::Range` with full Location info

### Current Usage in `process_list`

Located in `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`:

**Usage 1 - Line 192-195:** Extract `last_item_end_row` when item has multiple paragraphs
```rust
last_item_end_row = blocks.last().and_then(|b| {
    crate::pandoc::source_map_compat::source_info_to_range(get_block_source_info(b), context)
        .map(|r| r.end.row)  // ← Only need the end row!
});
```

**Usage 2 - Line 205-206:** Extract `last_para_range` for single-paragraph items
```rust
last_para_range =
    crate::pandoc::source_map_compat::source_info_to_range(&para.source_info, context);
```

**Usage 3 - Line 217-220:** Extract `last_item_end_row` for all items
```rust
last_item_end_row = blocks.last().and_then(|b| {
    crate::pandoc::source_map_compat::source_info_to_range(get_block_source_info(b), context)
        .map(|r| r.end.row)  // ← Only need the end row!
});
```

### How `last_item_end_row` and `last_para_range` Are Used

**`last_item_end_row` (Line 166-171):**
```rust
if let Some(last_end) = last_item_end_row {
    if child_range.start.row > last_end {
        // There's at least one blank line between items
        has_loose_item = true;
    }
}
```
**Purpose:** Detect blank lines between list items by comparing row numbers.

**`last_para_range` (Line 157-163):**
```rust
if let Some(ref last_range) = last_para_range {
    if last_range.end.row != child_range.start.row {
        // if the last paragraph ends on a different line than the current item starts,
        // then the last item was loose, mark it
        has_loose_item = true;
    }
}
```
**Purpose:** Detect if the last paragraph ends on a different line than the next item starts.

## Analysis

### Key Observations

1. **Usage 1 & 3 only need `end.row`** - We're calling `source_info_to_range()` which computes BOTH start and end locations, but then immediately extracting only `r.end.row`. This is wasteful.

2. **Usage 2 needs full Range** - The `last_para_range` is stored and later compared with `child_range.start.row`, so we need the full Range for this case.

3. **`child_range` is already a `pandoc::location::Range`** - This comes from the `IntermediateListItem` and already has row information. We're comparing `quarto_source_map::SourceInfo` (via conversion) with `pandoc::location::Range`.

### The Inefficiency

For usages 1 & 3, we're:
1. Calling `map_offset(0, ctx)` to get start location (unused!)
2. Calling `map_offset(length(), ctx)` to get end location
3. Building a full Range with start and end Locations
4. Extracting only `.end.row`

We should instead:
1. Call `map_offset(length(), ctx)` to get end location
2. Extract `.location.row` directly

## Proposed Changes

### Step 1: Replace Usage 1 & 3 (only need end row)

**Current (lines 192-195 and 217-220):**
```rust
last_item_end_row = blocks.last().and_then(|b| {
    crate::pandoc::source_map_compat::source_info_to_range(get_block_source_info(b), context)
        .map(|r| r.end.row)
});
```

**New:**
```rust
last_item_end_row = blocks.last().and_then(|b| {
    let source_info = get_block_source_info(b);
    source_info.map_offset(source_info.length(), &context.source_context)
        .map(|mapped| mapped.location.row)
});
```

**Why better:**
- Only computes what we need (end row)
- One `map_offset()` call instead of two
- No allocation of temporary Range struct
- Direct and clear about intent

### Step 2: Simplify Usage 2 (needs full Range for comparison)

**Current (lines 205-206):**
```rust
last_para_range =
    crate::pandoc::source_map_compat::source_info_to_range(&para.source_info, context);
```

**Analysis:** This one is trickier because:
- We need the full Range to store in `last_para_range: Option<crate::pandoc::location::Range>`
- Later compared at line 158: `if last_range.end.row != child_range.start.row`

**Option A - Keep using source_info_to_range for this one:**
Keep this usage as-is, but inline the function since it's only used here.

**Option B - Refactor to only store end row:**
Change `last_para_range` to `last_para_end_row: Option<usize>` and adjust comparison logic.

**Recommended: Option B** - More consistent with the pattern

**New:**
```rust
// Change variable type at line 106
let mut last_para_end_row: Option<usize> = None;

// Update line 205-210
if blocks.len() == 1 {
    if let Some(Block::Paragraph(para)) = blocks.first() {
        last_para_end_row = para.source_info
            .map_offset(para.source_info.length(), &context.source_context)
            .map(|mapped| mapped.location.row);
    } else {
        last_para_end_row = None;
    }
} else {
    last_para_end_row = None;
}

// Update line 157-163
if let Some(last_para_end) = last_para_end_row {
    if last_para_end != child_range.start.row {
        has_loose_item = true;
    }
}
```

**Why better:**
- Clearer intent: we only care about the end row, not the full range
- No unnecessary Range construction
- Consistent with `last_item_end_row` pattern

### Step 3: Remove `source_info_to_range` and `source_info_to_range_or_fallback`

After Steps 1 & 2, these functions are unused and can be deleted from `source_map_compat.rs`.

## Implementation Steps

1. **Update Usage 1** (line 192-195) - Replace with direct `map_offset()` call
2. **Update Usage 3** (line 217-220) - Replace with direct `map_offset()` call
3. **Refactor Usage 2** (lines 105-210 and 157-163):
   - Change `last_para_range: Option<Range>` to `last_para_end_row: Option<usize>`
   - Update assignment to use `map_offset()`
   - Update comparison to use `last_para_end != child_range.start.row`
4. **Delete unused functions** from `source_map_compat.rs`:
   - `source_info_to_range`
   - `source_info_to_range_or_fallback`
5. **Run tests** to ensure list processing still works correctly
6. **Check for any other callers** (there shouldn't be any based on grep)

## Testing Strategy

The logic being tested is "loose vs tight lists" in Markdown. Tests should verify:
1. Lists with blank lines between items are loose
2. Lists with no blank lines are tight
3. Lists where the last paragraph of an item is on a different line than the next item starts are loose

Existing tests in `quarto-markdown-pandoc` should cover this. Run:
```bash
cargo test --package quarto-markdown-pandoc
```

## Benefits

1. **Performance:** Two fewer `map_offset()` calls per list item (we were calling it twice, now once)
2. **Clarity:** Code directly expresses intent (need end row) rather than hiding it behind a conversion function
3. **Simplicity:** Removes two helper functions that were doing more than necessary
4. **Consistency:** Both `last_item_end_row` and `last_para_end_row` use the same pattern

## Risks

**Low risk** - The change is straightforward:
- Existing comparison logic remains the same (comparing row numbers)
- Just extracting the row number more directly
- No change to the loose/tight list detection algorithm

**Potential issue:** If `map_offset()` returns `None` (mapping fails), we handle it gracefully with `.and_then()` and `.map()`, same as before.
