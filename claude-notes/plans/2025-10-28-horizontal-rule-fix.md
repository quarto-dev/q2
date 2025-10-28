# Fix: Parser fails to recognize horizontal rules (---) in qmd files

**Issue**: k-268
**Date**: 2025-10-28

## Problem Analysis

### Current Behavior
The parser incorrectly treats horizontal rules as YAML metadata blocks when they appear with blank lines around them. For example:

```markdown
First paragraph.

---

Second paragraph.

---

Third paragraph.
```

This is being parsed as:
- paragraph
- **minus_metadata** (lines 2-7)
- paragraph

When it should be:
- paragraph
- **thematic_break**
- paragraph
- **thematic_break**
- paragraph

### Root Cause

Located in `tree-sitter-markdown/src/scanner.c`, function `parse_minus()` (lines 1101-1150).

The logic for detecting `MINUS_METADATA` currently:
1. Checks if we have exactly 3 minuses with no whitespace between them
2. Checks if there's a line end after the minuses
3. **Scans forward** looking for another line with exactly 3 minuses
4. If found, emits `MINUS_METADATA` token

The problem: **It doesn't check whether there's a blank line immediately after the opening `---`**.

According to the specification:
- **Metadata blocks**: Content immediately follows the opening `---` (single newline)
  ```
  ---
  title: Test
  ---
  ```

- **Horizontal rules**: Blank lines surround the `---` (double newline before/after)
  ```
  paragraph

  ---

  paragraph
  ```

### Key Insight

The distinction between metadata and horizontal rule should be:
- **Single newline** after `---` → Start looking for metadata closing delimiter
- **Double newline** after `---` → This is a horizontal rule, don't scan for closing delimiter

## Proposed Fix

### Location
File: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
Function: `parse_minus()` (around lines 1101-1150)

### Changes Required

In the metadata detection section (starting at line 1101), add a check for blank line:

**Current code (lines 1101-1112)**:
```c
if (minus_count == 3 && (!minus_after_whitespace) && line_end &&
    valid_symbols[MINUS_METADATA]) {
    for (;;) {
        // advance over newline
        if (lexer->lookahead == '\r') {
            advance(s, lexer);
            if (lexer->lookahead == '\n') {
                advance(s, lexer);
            }
        } else {
            advance(s, lexer);
        }
```

**Proposed change**: After advancing over the first newline, check if the next character is another newline (blank line). If so, this is NOT metadata, it's a horizontal rule that should be handled by the `THEMATIC_BREAK` branch above.

```c
if (minus_count == 3 && (!minus_after_whitespace) && line_end &&
    valid_symbols[MINUS_METADATA]) {
    // Save position before advancing
    TSLexer saved_lexer_state = *lexer;

    // advance over newline
    if (lexer->lookahead == '\r') {
        advance(s, lexer);
        if (lexer->lookahead == '\n') {
            advance(s, lexer);
        }
    } else {
        advance(s, lexer);
    }

    // Check if next line is blank (indicating horizontal rule, not metadata)
    if (lexer->lookahead == '\r' || lexer->lookahead == '\n') {
        // This is a horizontal rule surrounded by blank lines, not metadata
        // Don't process as metadata; let THEMATIC_BREAK handle it
        // Note: We can't restore lexer position, so we just skip the metadata logic
        // and return false to let other handlers try
        return false;
    }

    // Continue with normal metadata parsing...
    for (;;) {
```

### Alternative Approach

Actually, looking at the code flow more carefully, the `THEMATIC_BREAK` check happens **before** the `MINUS_METADATA` check (lines 1067-1073 vs 1101-1150). The issue is that `THEMATIC_BREAK` is being successfully matched, but then the metadata scanner is **also** being tried and is greedily consuming more input.

This suggests the fix should be simpler: **Just add the blank line check at the start of the metadata section** to prevent it from even trying to match when there's a blank line after.

## Implementation Plan

1. **Write test first** (TDD requirement from CLAUDE.md):
   - Create test in `tree-sitter-markdown/test/corpus/`
   - Test cases:
     a. Horizontal rules with blank lines (should parse as `thematic_break`)
     b. Actual metadata blocks (should still parse as `minus_metadata`)
     c. Edge cases (BOF, EOF, mixed scenarios)
   - Run `tree-sitter test` to verify tests fail

2. **Implement the fix**:
   - Modify `parse_minus()` in `scanner.c`
   - Add blank line check before metadata scanning

3. **Rebuild and test**:
   - Run `tree-sitter generate` in `tree-sitter-markdown/`
   - Run `tree-sitter build` in `tree-sitter-markdown/`
   - Run `tree-sitter test` to verify all tests pass

4. **Integration test**:
   - Test with `quarto-markdown-pandoc` on the original `horizontal-rule.qmd` fixture
   - Verify correct parsing

5. **Update beads**:
   - Close k-268

## Test Cases to Add

```
================================================================================
Horizontal rules with blank lines
================================================================================

First paragraph.

---

Second paragraph.

---

Third paragraph.

--------------------------------------------------------------------------------

(document
  (section
    (paragraph (inline))
    (thematic_break)
    (paragraph (inline))
    (thematic_break)
    (paragraph (inline))))

================================================================================
YAML metadata block (should still work)
================================================================================

---
title: Test
author: Someone
---

Content paragraph.

--------------------------------------------------------------------------------

(document
  (minus_metadata)
  (section
    (paragraph (inline))))
```

## Notes

- The fix must maintain backward compatibility with existing metadata parsing
- The key differentiator is: blank line after opening `---` = horizontal rule
- This aligns with CommonMark spec for thematic breaks
- User requirement allows horizontal rules anywhere except BOF/EOF initially (though current code seems to handle BOF already)
