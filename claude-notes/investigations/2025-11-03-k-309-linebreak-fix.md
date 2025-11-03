# Fix for k-309: LineBreak vs SoftBreak Detection

## Date: 2025-11-03

## Problem
Text with two trailing spaces followed by a newline (hard line break in Markdown) was being parsed as `SoftBreak` instead of `LineBreak`.

Example:
```markdown
Line one
Line two
```

Should produce `LineBreak`, but was producing `SoftBreak`.

## Root Cause
The `pandoc_soft_break` handler in `treesitter.rs` was unconditionally creating `SoftBreak` nodes, without checking for the trailing spaces that indicate a hard line break.

## Investigation
1. Tree-sitter grammar represents both hard and soft line breaks as `pandoc_soft_break` nodes
2. The difference is in the content: hard breaks include 2+ spaces before the newline
3. The `pandoc_soft_break` node's byte range INCLUDES the trailing spaces

Example tree:
```
(pandoc_str [0, 5] - [0, 8])           # "one"
(pandoc_soft_break [0, 8] - [1, 0])    # "  \n" (2 spaces + newline)
```

## Solution
Modified the `pandoc_soft_break` handler to:
1. Get the node's byte range (start_byte to end_byte)
2. Count consecutive spaces from the start of the node
3. If 2+ spaces are found, create `LineBreak`
4. Otherwise, create `SoftBreak`

## Code Changes
File: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`

1. Added `LineBreak` to imports (line 48)
2. Modified `pandoc_soft_break` handler (lines 546-572):
   - Extract node byte range
   - Count leading spaces in the node
   - Create `LineBreak` if trailing_spaces >= 2

## Testing
```bash
# Test input
echo 'Line one
Line two' > /tmp/test.md

# Pandoc output:
# [Para [Str "Line", Space, Str "one", LineBreak, Str "Line", Space, Str "two"]]

# Our output (after fix):
# [Para [Str "Line", Space, Str "one", LineBreak, Str "Line", Space, Str "two"]]
```

✅ Output matches Pandoc exactly!

## Status
- Issue k-309: **CLOSED**
- Fix verified with test cases
- Note: test_html_writer and test_json_writer still have unrelated smart quotes failures
