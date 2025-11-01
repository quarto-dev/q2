# Fix List Item Block Ending Detection

**Issue**: k-315
**Date**: 2025-11-01

## Problem

List items never "end" properly after blank lines. Example:
- Input: `- a\n\nb`
- Expected: list item ends, `b` is a separate paragraph
- Actual: list item continues to include `b`

Block quotes work correctly with the same pattern: `> a\n\nb`

Suspected issue in `scanner.c` block ending logic.

## Debugging Strategy

1. Write a test case that demonstrates the failure
2. Use tree-sitter parse in debug mode to examine the parse tree
3. Compare block quote behavior (working) vs list behavior (broken)
4. Instrument scanner.c with additional debug prints if needed
5. Identify the logic difference
6. Fix the scanner logic
7. Verify test passes

## Key Files

- `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` - main scanner logic
- `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` - grammar definition
- `crates/tree-sitter-qmd/tree-sitter-markdown/test/` - test directory
