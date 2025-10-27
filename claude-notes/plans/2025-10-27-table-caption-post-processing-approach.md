# Table Caption Post-Processing Approach - Analysis and Plan

**Date**: 2025-10-27
**Issue**: k-185 - Table caption parsing without blank line
**Status**: Analysis complete, ready to implement

## Executive Summary

**Key Discovery**: The grammar changes ARE working correctly! Tree-sitter successfully produces a `caption` node inside `pipe_table`. The only issue is that the Rust code expects `"table_caption"` but tree-sitter produces `"caption"`.

**Recommendation**: Fix the Rust code (5-10 lines) instead of complex post-processing.

## Current State Analysis

### What Pandoc Produces
```
Table ( ... )
  (Caption Nothing [ Plain [Str "Caption", Space, Str "without", ...] ])
```

### What Tree-Sitter Currently Produces (with grammar changes)
Verbose output shows:
```
reduce sym:caption, child_count:4
reduce sym:pipe_table, child_count:7
```

The tree structure is:
```
pipe_table
  ├── pipe_table_header
  ├── pipe_table_delimiter_row
  ├── pipe_table_row (data rows)
  └── caption ← Successfully parsed!
```

### What the Rust Code Does
`pipe_table.rs:188-196` checks for these node types:
- `"block_continuation"` ✅
- `"pipe_table_header"` ✅
- `"pipe_table_delimiter_row"` ✅
- `"pipe_table_row"` ✅
- `"table_caption"` ❌ WRONG! Should be `"caption"`

Result: **Panics at line 195**: "Unexpected node in pipe_table: caption"

## Two Possible Approaches

### Approach A: Simple Rust Fix (RECOMMENDED) ⭐

**What to do**:
1. Change `"table_caption"` to `"caption"` at `pipe_table.rs:188`
2. Extract inlines from the `CaptionBlock` that `process_caption()` produces
3. Store in existing `caption_inlines` variable

**Code changes** (~5-10 lines):
```rust
} else if node == "caption" {  // Changed from "table_caption"
    match child {
        PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(caption_block)) => {
            caption_inlines = Some(caption_block.content);
        }
        _ => panic!("Expected CaptionBlock in caption, got {:?}", child),
    }
}
```

**Why this works**:
- Grammar successfully produces `caption` node ✅
- Tree-sitter tests pass (255/255) ✅
- Scanner prevents `:` from being table row ✅
- Just need to wire up Rust processing ✅

**Pros**:
- Simple, clean, correct solution
- Minimal code changes
- Leverages existing working grammar
- No fragile pattern matching

**Cons**:
- None significant

**Estimated effort**: 10 minutes

---

### Approach B: Post-Processing (NOT RECOMMENDED)

**What to do**:
1. Revert all grammar changes in `grammar.js` and `scanner.c`
2. Let the malformed structure be produced (caption as table row)
3. Detect when last table row has single cell starting with ":"
4. Extract that row, create Caption from its content
5. Remove row from table body

**Code changes** (~50-100 lines):
```rust
// After processing all table rows, check if last row is actually a caption
if let Some(last_row) = rows.last() {
    if last_row.cells.len() == 1 {
        if let Some(Block::Plain(plain)) = last_row.cells[0].content.first() {
            if let Some(first_inline) = plain.content.first() {
                if matches!(first_inline, Inline::Str(s) if s.starts_with(':')) {
                    // This is a caption disguised as a row!
                    // Extract it, remove the ':', create caption
                    rows.pop();
                    caption_inlines = Some(extract_and_clean_caption_inlines(...));
                }
            }
        }
    }
}
```

**Why this approach**:
- Works without grammar changes
- Handles tree-sitter error recovery

**Pros**:
- Avoids understanding tree-sitter internals (but we already do!)

**Cons**:
- Much more complex (~50-100 lines vs ~5-10 lines)
- Fragile pattern matching (what if cell has multiple inlines?)
- Discards working grammar improvements
- Need to handle edge cases (empty caption, whitespace, etc.)
- Harder to maintain
- Doesn't fix root cause

**Estimated effort**: 1-2 hours + edge case testing

---

## Detailed Analysis: Why Approach A is Better

### Evidence That Grammar Works

1. **Verbose tree-sitter output shows**:
   ```
   reduce sym:caption, child_count:4
   reduce sym:pipe_table, child_count:7
   ```
   This confirms caption is part of pipe_table structure.

2. **Grammar definition** (line 408):
   ```javascript
   pipe_table: $ => seq(
       // ...
       choice(
           seq($._newline, optional($.caption)),  ← Caption is here!
           $._eof
       ),
   )
   ```

3. **Tree-sitter tests pass**: 255/255 tests passing

4. **Scanner changes work**: Lines starting with `:` don't become table rows

### Why the Panic Happens

At `pipe_table.rs:188`, the code checks:
```rust
} else if node == "table_caption" {
```

But tree-sitter produces a node named `"caption"` (not `"table_caption"`).

This is confirmed by:
- The panic message: "Unexpected node in pipe_table: **caption**"
- The grammar definition uses `$.caption` (line 408)
- The treesitter.rs mapping: `"caption" => process_caption()` (line 707)

### How process_caption() Works

File: `caption.rs:16-39`

```rust
pub fn process_caption(...) -> PandocNativeIntermediate {
    let mut caption_inlines: Inlines = Vec::new();

    for (node_name, child) in children {
        if node_name == "inline" {
            match child {
                PandocNativeIntermediate::IntermediateInlines(inlines) => {
                    caption_inlines.extend(inlines);
                }
                _ => panic!("Expected Inlines in caption, got {:?}", child),
            }
        }
        // Skip other nodes like ":", blank_line, etc.
    }

    PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(CaptionBlock {
        content: caption_inlines,  ← This is what we need!
        source_info: node_source_info_with_context(node, context),
    }))
}
```

**Key insight**: The `CaptionBlock` has a `content: Inlines` field that contains the caption text (without the leading colon).

### The Fix

At `pipe_table.rs:188-196`, change:

```rust
// OLD CODE
} else if node == "table_caption" {
    if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
        caption_inlines = Some(inlines);
    } else {
        panic!("Expected Inlines in table_caption, got {:?}", child);
    }
}
```

To:

```rust
// NEW CODE
} else if node == "caption" {  // ← Changed from "table_caption"
    match child {
        PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(caption_block)) => {
            caption_inlines = Some(caption_block.content);  // ← Extract inlines from CaptionBlock
        }
        _ => panic!("Expected CaptionBlock in caption, got {:?}", child),
    }
}
```

That's it! The rest of the code already handles `caption_inlines` correctly (lines 219-237).

## Implementation Steps (Approach A)

### Step 1: Update pipe_table.rs
- [x] Identify the problem location (line 188)
- [ ] Change `"table_caption"` to `"caption"`
- [ ] Change pattern match from `IntermediateInlines` to `IntermediateBlock(Block::CaptionBlock)`
- [ ] Extract `caption_block.content` into `caption_inlines`

### Step 2: Test the fix
- [ ] Run `cargo check` to ensure compilation
- [ ] Run the failing test: `cargo run -- -i table-caption-no-blank-line.qmd -t native`
- [ ] Verify output matches Pandoc's output
- [ ] Run full test suite: `cargo test`

### Step 3: Verify regression tests
- [ ] Test with blank line: `cargo run -- -i table-caption-with-blank-line.qmd -t native`
- [ ] Ensure both cases work correctly

### Step 4: Clean up
- [ ] Check if scanner warnings can be fixed (unused variables)
- [ ] Run `cargo fmt`

## Expected Results

**Before fix**:
```
thread 'main' panicked at pipe_table.rs:195:
Unexpected node in pipe_table: caption
```

**After fix**:
```
[ Table ( "" , [] , [] ) (Caption Nothing [ Plain [Str "Caption", Space, Str "without", Space, Str "blank", Space, Str "line"] ]) ... ]
```

This should match Pandoc's output exactly.

## Edge Cases to Test

All these should work after the fix:
- ✅ Caption without blank line (primary fix)
- ✅ Caption with blank line (regression test)
- ✅ Table without caption (should continue working)
- ✅ Multiple tables with captions
- ✅ Fenced divs after table (`::`/`:::` should not be captions)

## Risk Assessment

**Approach A risks**:
- Very low risk
- Grammar is proven to work
- Minimal code change
- Easy to test

**Approach B risks**:
- Medium-high risk
- Complex pattern matching
- Many edge cases
- Discards working solution

## Conclusion

**Use Approach A**: It's simpler, cleaner, and the grammar already works correctly. The fix is literally changing one string and adjusting the pattern match to extract from `CaptionBlock` instead of expecting raw `Inlines`.

## References

- Previous attempts: `claude-notes/plans/2025-10-27-table-caption-implementation-attempts.md`
- Grammar file: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:258, 408`
- Rust processing: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/pipe_table.rs:188-196`
- Caption processing: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/caption.rs:16-39`
- Beads issue: k-185
