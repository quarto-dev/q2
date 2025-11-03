# Fix for k-311: Nested Inline Formatting (Strikeout inside Subscript)

## Date: 2025-11-03

## Problem
The example `~he~~l~~lo~` should parse as subscript containing strikeout, but was producing:
```
Subscript [Str "he", RawInline (Format "quarto-internal-leftover") "~he~~l~~lo~", Str "lo"]
```

Instead of the correct output:
```
Subscript [Str "he", Strikeout [Str "l"], Str "lo"]
```

## Root Cause Analysis

### Step 1: Tree-sitter Grammar Check
The tree-sitter grammar was actually parsing correctly:
```
pandoc_subscript [0, 0] - [0, 11]
  subscript_delimiter [0, 0] - [0, 1]     # ~
  pandoc_str [0, 1] - [0, 3]              # "he"
  pandoc_strikeout [0, 3] - [0, 8]        # ~~l~~
    strikeout_delimiter [0, 3] - [0, 5]   # ~~
    pandoc_str [0, 5] - [0, 6]            # "l"
    strikeout_delimiter [0, 6] - [0, 8]   # ~~
  pandoc_str [0, 8] - [0, 10]             # "lo"
  subscript_delimiter [0, 10] - [0, 11]   # ~
```

So the grammar correctly recognizes nested structures.

### Step 2: Handler Investigation
The problem was in the Rust handler code, specifically in `process_native_inline()` in `treesitter.rs`.

When processing nested formatting:
1. `pandoc_subscript` calls `process_inline_with_delimiter_spaces`
2. This processes children and calls `native_inline` closure on each child
3. The nested `pandoc_strikeout` returns `IntermediateInlines` (plural)
4. But `process_native_inline` only handled `IntermediateInline` (singular)

The match statement in `process_native_inline` had these cases:
- `IntermediateInline` ✅
- `IntermediateBaseText` ✅
- `IntermediateAttr` ✅
- `IntermediateUnknown` ✅
- **`IntermediateInlines` - MISSING** ❌

When the nested strikeout returned `IntermediateInlines`, it fell through to the catch-all `other` case, which created a `RawInline` with "leftover" text.

## Solution
Added a case for `IntermediateInlines` in `process_native_inline()`:

```rust
PandocNativeIntermediate::IntermediateInlines(inlines) => {
    // If it's a single inline, just return it directly
    if inlines.len() == 1 {
        inlines.into_iter().next().unwrap()
    } else {
        // Multiple inlines need to be wrapped in a Span
        // This shouldn't normally happen in practice, but handle it gracefully
        use crate::pandoc::attr::{empty_attr, AttrSourceInfo};
        Inline::Span(crate::pandoc::inline::Span {
            attr: empty_attr(),
            attr_source: AttrSourceInfo {
                id: None,
                classes: Vec::new(),
                attributes: Vec::new(),
            },
            content: inlines,
            source_info: node_source_info_fn(),
        })
    }
}
```

The logic:
- If we have a single inline (common case), return it directly
- If we have multiple inlines (edge case), wrap them in a transparent Span

## Testing

### Test Input
```markdown
~he~~l~~lo~
```

### Expected (Pandoc)
```
[ Para [ Subscript [ Str "he" , Strikeout [ Str "l" ] , Str "lo" ] ] ]
```

### Actual (After Fix)
```
[ Para [Subscript [Str "he", Strikeout [Str "l"], Str "lo"]] ]
```

✅ Perfect match!

### Test Suite
```bash
cargo test --test test -- unit_test_corpus_matches_pandoc_commonmark
# test result: ok. 1 passed; 0 failed
```

## Files Changed
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`
  - Added `IntermediateInlines` case to `process_native_inline()` (lines 356-378)

## Status
- Issue k-311: **CLOSED**
- All tests pass ✅
- Nested inline formatting now works correctly
