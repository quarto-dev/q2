# Tight vs Loose List Detection Bug Analysis

## Date: 2025-11-03

## Test Case: 058.qmd

```markdown
1.  foo.

    ```
    huh
    ```

2.  bar.

    ```
    huh
    ```
```

## Expected Behavior (Pandoc)
- List should be **LOOSE**
- List items should contain `Para` blocks (not `Plain`)

## Actual Behavior (Our Code)
- List is **TIGHT**
- List items contain `Plain` blocks

## CommonMark Specification

A list is **loose** if:
1. Items are separated by blank lines, OR
2. Any item contains multiple block-level elements with blank line(s) between them

This test satisfies **BOTH** conditions:
- **Condition 1**: Line 5 is blank (between items)
- **Condition 2**: Line 1 is blank (between paragraph and code block within item 1)

## Tree-Sitter Parse Structure

```
(list_item [0, 0] - [6, 0]
  (list_marker_dot [0, 0] - [0, 4])
  (pandoc_paragraph [0, 4] - [2, 4]          ← Ends at line 2, col 4
    (pandoc_str [0, 4] - [0, 8])             ← "foo."
    (block_continuation [1, 0] - [2, 4]))    ← Blank line included in paragraph
  (pandoc_code_block [2, 4] - [6, 0]))       ← Starts at line 2, ends at line 6

(list_item [6, 0] - [11, 0]                  ← Starts at line 6
  (list_marker_dot [6, 0] - [6, 4])
  (pandoc_paragraph [6, 4] - [8, 4]
    (pandoc_str [6, 4] - [6, 8])
    (block_continuation [7, 0] - [8, 4]))
  (pandoc_code_block [8, 4] - [11, 0]))
```

## Key Observations

1. **Each list item has 2 blocks**: paragraph + code block (`blocks.len() == 2`)
2. **Paragraph range includes blank line**: [0, 4] - [2, 4] includes the blank line via `block_continuation`
3. **Items are adjacent in tree**: First item ends at [6, 0], second starts at [6, 0]
4. **Blank line 5 is "between" them but not explicitly represented**

## Code Analysis: process_list() in treesitter.rs

### Issue 1: Multiple blocks reset last_para_end_row (Lines 208-213)

```rust
} else {
    // if the item has multiple blocks (but not multiple paragraphs,
    // which would have been caught above), we need to reset the
    // last_para_end_row since this item can't participate in loose detection
    last_para_end_row = None;
}
```

**Problem**: When `blocks.len() > 1`, this resets `last_para_end_row = None`, preventing the
next item from checking if there was a blank line after the previous item's paragraph.

### Issue 2: No check for blank lines WITHIN items

The code only checks:
- If item has > 1 paragraph (line 167-179)
- If there's a blank line between items (line 159-164)

It does NOT check if there's a blank line between a paragraph and other blocks within the same item.

### Issue 3: Between-items check may not work correctly (Lines 159-164)

```rust
// Check if there's a blank line between the last item and this item
if let Some(last_end) = last_item_end_row {
    if child_range.start.row > last_end {
        // There's at least one blank line between items
        has_loose_item = true;
    }
}
```

With the new tree-sitter grammar:
- `last_item_end_row` for item 1 = row where code block ends
- Code block range is [2, 4] - [6, 0]
- `map_offset(source_info.length())` maps to end position
- If code block ends at [6, 0], then `last_item_end_row` = 5 (last actual line) or 6 (exclusive end)?
- `child_range.start.row` for item 2 = 6
- Check: `6 > ?` depends on how we calculate last_item_end_row

## Root Cause Hypothesis

The tree-sitter grammar was refactored and now:
1. **Blank lines are included in block ranges** (via block_continuation)
2. **Item ranges extend to next item start** (no gap between items in tree)
3. **The loose detection logic assumes the old grammar structure** where:
   - Blank lines were represented differently
   - Block ranges didn't include trailing whitespace
   - There were gaps between items in the tree

## Fix Strategy

Need to detect loose lists by checking if:
1. Any item has multiple blocks AND has blank line(s) between them
2. Items are separated by blank lines (need to fix the detection logic)

### Approach 1: Check blank lines within items
- When `blocks.len() > 1`, check if there's a blank line between first block and second block
- Use source map to check if paragraph end row < next block start row - 1

### Approach 2: Fix between-items detection
- Correctly calculate the last actual content row (not the exclusive end row)
- Account for how tree-sitter now includes trailing content

### Approach 3: Simpler heuristic
- If any item has multiple blocks, mark as loose (might be too aggressive)
- Check the CommonMark spec to see if this is actually correct

## Next Steps

1. Write failing test
2. Add debug output to trace actual values
3. Implement fix based on analysis
4. Verify test passes
5. Run full test suite
