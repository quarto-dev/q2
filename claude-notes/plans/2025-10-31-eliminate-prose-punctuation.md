# Grammar Issue: emphasis_delimiter Captures Adjacent Whitespace

**Date**: 2025-10-31
**Discovered During**: Implementation of `pandoc_emph` handler
**Status**: Documented - Needs Grammar Fix

## Problem

The tree-sitter grammar's `emphasis_delimiter` nodes capture adjacent whitespace, preventing proper `Space` node generation.

**Test Case**: `"x *y* z"`

**Current Output**: `[ Para [Str "x", Emph [Str "y"], Str "z"] ]`  
**Expected Output**: `[ Para [ Str "x" , Space , Emph [ Str "y" ] , Space , Str "z" ] ]`

**Root Cause**: Delimiter byte ranges include surrounding spaces:
- `emphasis_delimiter: (0, 1) - (0, 3)` = `" *"` (includes leading space)
- `emphasis_delimiter: (0, 4) - (0, 6)` = `"* "` (includes trailing space)

## Impact

- Semantic meaning preserved (emphasis works correctly)
- Space information lost in AST
- Output doesn't match Pandoc's structure
- May affect roundtripping

## Fix Location

This requires a **grammar fix** in `tree-sitter-markdown-inline/grammar.js`, not in the Rust processor.

## Related

May affect other delimiter-based constructs: `pandoc_strong`, `code_span_delimiter`, `strikeout_delimiter`, etc.
