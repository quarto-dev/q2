# Debugging Div + Math Parse Error

## Problem
The parser fails on this document:
```markdown
::: {}

$$hello$$

:::
```

But these parse fine individually:
- `:::  {}\n\n:::`
- `$$hello$$`

## Plan

1. ✅ Create plan
2. Reproduce the error with the failing document
3. Verify the two working documents parse correctly
4. Run parser with `-v` flag to get detailed tree-sitter output and concrete syntax tree
5. Analyze the CST to identify where parsing goes wrong
6. Locate the bug in the parser code (likely in tree-sitter grammar or block/inline interaction)
7. Write a test that fails with the current behavior
8. Fix the bug
9. Verify the test passes

## Notes

- The issue appears to be an interaction between divs with attributes and display math
- Since both components work individually, likely an issue with whitespace handling or state management between block elements
- Use `cargo run -- -v` to get verbose tree-sitter output

## Root Cause Analysis

Looking at scanner.c:1508-1518:

The DISPLAY_MATH_STATE_TRACK_MARKER check happens BEFORE block continuation matching (line 1660+).

When inside a fenced div block:
1. Parser is in STATE_MATCHING mode (trying to match block continuations)
2. At line 3 of the document (`$$hello$$`), scanner sees `$`
3. The check at line 1508: `if (!s->simulate && lexer->lookahead == '$' && !inside_fenced_code && valid_symbols[DISPLAY_MATH_STATE_TRACK_MARKER])`
4. Since we're NOT simulating and valid_symbols includes DISPLAY_MATH_STATE_TRACK_MARKER, it consumes the `$$`
5. This happens BEFORE the block continuation check, causing the parser to fail

**The fix**: When in STATE_MATCHING mode (i.e., `s->state & STATE_MATCHING`), we should NOT process DISPLAY_MATH_STATE_TRACK_MARKER early. We should let the block continuation logic run first.
