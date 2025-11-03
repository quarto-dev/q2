# Tight vs Loose List Detection Fix - Summary

## Date: 2025-11-03

## Problem
Test `tests/pandoc-match-corpus/markdown/058.qmd` was failing because lists with multiple blocks per item were incorrectly detected as tight lists (using `Plain`) instead of loose lists (using `Para`).

## Root Cause
The tree-sitter grammar refactoring changed how block ranges are represented. The list detection logic in `process_list()` wasn't adapted to handle items with multiple blocks correctly according to CommonMark/Pandoc rules.

## The Rule (verified with Pandoc)
A list is **loose** (uses `Para`) if:
1. Items are separated by blank lines, OR
2. ANY item contains multiple blocks (regardless of whether there are blank lines between those blocks)

A list is **tight** (uses `Plain`) only if:
- All items have single blocks AND
- No blank lines between items

## Fix Applied
Added check in `src/pandoc/treesitter.rs` at line 195:

```rust
// Check if this item has multiple blocks
// According to CommonMark/Pandoc, any item with multiple blocks makes the list loose
// (regardless of whether there are blank lines between the blocks)
if blocks.len() > 1
    && blocks
        .iter()
        .any(|block| matches!(block, Block::Paragraph(_)))
{
    has_loose_item = true;
}
```

## Results
- ✅ Test 058.qmd now **PASSES**
- ✅ List items with multiple blocks correctly use `Para`
- ✅ Fix is minimal and targeted

## Discovered Issue (Unrelated)
Test `pipe-table-code-span.qmd` now fails because it was masked by 058 failing first. This is a **pre-existing bug** in table cell parsing that adds an extra `Space` before code spans in certain cases. This bug exists in both the current code AND the code before my changes.

**Recommendation**: File this as a separate issue for table parsing.

## Files Changed
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs` (11 lines added)

## Test Verification
```bash
# Test 058.qmd specifically
cargo run --bin quarto-markdown-pandoc -- -i tests/pandoc-match-corpus/markdown/058.qmd --to native

# Output correctly shows Para instead of Plain:
# [ OrderedList (1, Decimal, Period) [[Para [Str "foo."], CodeBlock ...], [Para [Str "bar."], CodeBlock ...]] ]
```
