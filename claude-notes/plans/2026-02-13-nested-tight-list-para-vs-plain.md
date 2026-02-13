# Fix: Nested tight lists incorrectly marked as loose (Para instead of Plain)

**Beads issue:** bd-2gc9

## Overview

When a tight list item contains content followed by a nested sublist (no blank line between them), pampa incorrectly marks the entire list as "loose" and emits `Para` instead of `Plain`. Both Pandoc and CommonMark say this should be a tight list with `Plain`.

### Reproduction

Input:
```markdown
* foo
  * bar
* baz
```

**Pandoc** output: all items use `Plain` (correct — tight list)
**Pampa** output: outer items "foo" and "baz" use `Para` (wrong — loose list), inner "bar" uses `Plain`

### Root Cause

In `process_list()` at `crates/pampa/src/pandoc/treesitter.rs:183-192`:

```rust
// Check if this item has multiple blocks
// According to CommonMark/Pandoc, any item with multiple blocks makes the list loose
// (regardless of whether there are blank lines between the blocks)
if blocks.len() > 1
    && blocks.iter().any(|block| matches!(block, Block::Paragraph(_)))
{
    has_loose_item = true;
}
```

The comment is factually wrong. The [CommonMark spec (section 5.3)](https://spec.commonmark.org/0.31.2/#lists) says:

> A list is loose if any of its constituent list items are separated by blank lines, or if any of its constituent list items directly contain two block-level elements **with a blank line between them**.

The key phrase is "with a blank line between them." Having a nested sublist immediately following content text (no blank line) should NOT make the list loose.

### Trace for `* foo\n  * bar\n* baz`

1. Item 1 blocks = `[Paragraph("foo"), BulletList([Plain("bar")])]`
2. `blocks.len() > 1` = true (2 blocks)
3. `any(Paragraph)` = true
4. → `has_loose_item = true` ← **BUG**
5. All outer list items rendered as `Para` instead of `Plain`

## Work Items

### Phase 1: Tests

- [x] Add test case `test_tight_list_with_nested_sublist_beginning` — verified fails with Para instead of Plain
- [x] Add test case `test_tight_list_with_nested_sublist_middle` (3-item variant) — verified fails with Para instead of Plain
- [x] Add test case `test_loose_list_with_nested_sublist_has_blank_lines` — verified passes (loose list correctly uses Para)

### Phase 2: Fix

- [x] Fix the check at lines 183-192 in `process_list()` to only set `has_loose_item = true` when there's actually a blank line between consecutive blocks in the item
- [x] Update the incorrect comment with correct CommonMark spec reference

### Phase 3: Verification

- [x] Verify all new tests pass (3/3 pass)
- [x] Run full workspace test suite (`cargo nextest run --workspace`) — 6411 tests pass, 0 failures
- [x] Verify existing roundtrip tests still pass

## Actual Fix (implemented)

The initial approach of comparing converted block source positions didn't work because tree-sitter
paragraph nodes absorb blank lines via `block_continuation` children, making adjacent blocks appear
contiguous even when separated by blank lines.

The actual fix propagates blank-line information from tree-sitter level through `IntermediateListItem`:

1. **Added `has_blank_line_between_blocks: bool` to `IntermediateListItem`** in
   `pandocnativeintermediate.rs`

2. **Added `list_item_has_blank_line_between_blocks()` helper** in `treesitter.rs` — walks the
   tree-sitter `list_item_node`'s children to find `pandoc_paragraph` nodes followed by another
   block-level sibling, and checks if the paragraph contains a `block_continuation` that spans
   multiple rows. In tree-sitter's QMD grammar, a multi-row `block_continuation` indicates it
   absorbed a blank line.

3. **Modified the check in `process_list()`** to use `item_has_blank_line_between_blocks` instead
   of the unconditional `blocks.len() > 1 && any(Paragraph)` check.
