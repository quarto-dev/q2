# Plan: Fix Table Caption Parsing Without Blank Line (k-185)

**Date**: 2025-10-27
**Issue**: k-185 - Table caption parsing fails without blank line before caption
**Status**: Planning

## Problem Statement

Table captions using the colon syntax (`: Caption text`) require a blank line before the caption line, but Pandoc doesn't require this.

**Working case**:
```markdown
| Header |
|--------|
| Data   |

: Caption text
```

**Failing case**:
```markdown
| Header |
|--------|
| Data   |
: Caption text
```

In the failing case, the caption line is incorrectly parsed as a table row containing `": Caption text"` instead of being attached as the table's caption.

## Root Cause

### Grammar Level
The caption rule in `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:257-265` hardcodes a blank line requirement:

```javascript
caption: $ => prec.right(seq(
    $._blank_line,        // ← PROBLEM: hardcoded requirement
    ':',
    optional(seq(
        $._whitespace,
        alias($._caption_line, $.inline)
    )),
    choice($._newline, $._eof),
)),
```

### Parser Behavior
When no blank line precedes `: Caption text`:
1. Pipe table remains active (hasn't been closed)
2. The `:` is treated as valid punctuation in pipe table cell contents
3. The entire line becomes a pipe_table_row with cell content `": Caption text"`
4. The table gets an empty caption and an extra data row

### Key Insight: No Ambiguity in QMD
Unlike CommonMark/Pandoc which supports definition lists using `: ` at line start, **quarto-markdown does NOT support definition lists**. Therefore, `: ` at the beginning of a line (after accounting for indentation/block markers) is **ONLY** used for captions.

This means there is **zero ambiguity** - we can safely recognize caption syntax without requiring a blank line.

## Solution Approach

### Two-Part Fix

#### Part 1: Remove Blank Line Requirement from Grammar
Modify the caption rule to make the blank line optional or remove it entirely:

```javascript
caption: $ => prec.right(seq(
    ':',  // ← Remove $._blank_line requirement
    optional(seq(
        $._whitespace,
        alias($._caption_line, $.inline)
    )),
    choice($._newline, $._eof),
)),
```

#### Part 2: Teach Pipe Table to Terminate Before Captions
Modify the external scanner (`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`) to recognize caption lines as table terminators.

When the external scanner is deciding whether to emit `$._pipe_table_line_ending` (which continues the table), it should check:
- Is the current line starting with a single `:` (not `::` or `:::`) followed by space/text?
- If YES: Do NOT emit `$._pipe_table_line_ending`
- This causes the pipe table's `repeat(seq($._pipe_table_newline, optional($.pipe_table_row)))` to stop
- The table closes naturally
- Parser returns to document context where the caption rule can match

### Why This Works
1. The external scanner controls pipe table continuation via `$._pipe_table_line_ending`
2. By not emitting this token when we see a caption line, the table naturally closes
3. The caption rule can then match in document context
4. No ambiguity because `: ` **only means caption** in qmd (no definition lists)

## Implementation Plan

### Step 1: Examine External Scanner
**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`

Tasks:
- [ ] Read and understand the external scanner logic
- [ ] Find where `_pipe_table_line_ending` token emission is decided
- [ ] Understand the lookahead mechanism and how to check the next line's start
- [ ] Identify how to distinguish `:` (caption) from `::` or `:::` (fenced divs)

### Step 2: Write Failing Tests FIRST
**File**: Create test files in appropriate location (likely `crates/quarto-markdown-pandoc/tests/`)

Tasks:
- [ ] Create test: table with caption, no blank line - should produce table with caption
- [ ] Create test: table with caption, blank line - should continue to work (regression test)
- [ ] Create test: table followed by `::` fenced div - should not be confused with caption
- [ ] Create test: table in blockquote with caption - caption should work
- [ ] Run tests and verify they fail with expected errors

### Step 3: Modify External Scanner
**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`

Tasks:
- [ ] Add helper function to check if a line starts with caption syntax (`: ` but not `::` or `:::`)
- [ ] Modify pipe table line ending logic to:
  - Look ahead at the next line
  - If it starts with caption syntax, do NOT emit `$._pipe_table_line_ending`
  - Let the table close naturally
- [ ] Handle edge cases:
  - Indented contexts (block quotes, nested lists)
  - Empty caption lines (just `:`)
  - Caption with only whitespace after `:`

### Step 4: Modify Grammar (if needed)
**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`

Options to consider:
1. **Remove blank line requirement entirely** - caption becomes `seq(':', optional(seq($._whitespace, alias($._caption_line, $.inline))), choice($._newline, $._eof))`
2. **Make blank line optional** - caption becomes `seq(optional($._blank_line), ':', ...)`

Decision: Start with option 1 (remove entirely) since external scanner will handle table termination.

Tasks:
- [ ] Modify caption rule in grammar.js
- [ ] Ensure caption remains in `_block_not_section` choice

### Step 5: Rebuild Parser
**Location**: `crates/tree-sitter-qmd/tree-sitter-markdown/`

Tasks:
- [ ] Run `tree-sitter generate` to regenerate parser from modified grammar
- [ ] Run `tree-sitter build` to compile the parser
- [ ] Run `tree-sitter test` to ensure existing tree-sitter tests still pass
- [ ] Fix any tree-sitter test failures (only modify tests we just added, not existing ones)

### Step 6: Run Rust Tests
**Location**: `crates/quarto-markdown-pandoc/`

Tasks:
- [ ] Run `cargo check` to ensure changes compile
- [ ] Run `cargo test` to run full test suite
- [ ] Verify our new tests now pass
- [ ] Ensure all existing tests still pass (no regressions)
- [ ] If any tests fail, debug and fix

### Step 7: Manual Testing with -v Flag
**Tasks**:
- [ ] Run `cargo run --bin quarto-markdown-pandoc -- -i test-caption-no-blank.qmd -v`
- [ ] Verify tree-sitter parse trace shows caption as separate block (not table row)
- [ ] Run without `-v` and verify final AST has caption attached to table
- [ ] Compare with Pandoc output: `pandoc -t native -i test-caption-no-blank.qmd`

### Step 8: Edge Case Testing
Create and test edge cases:
- [ ] Table with caption in block quote
- [ ] Table with caption in nested list
- [ ] Multiple tables with captions in sequence
- [ ] Table followed by fenced div (`::`/`:::`) - should not be confused
- [ ] Caption with attributes: `: Caption {#tbl-id}`
- [ ] Empty caption: just `:`
- [ ] Caption with leading whitespace after colon: `:   Caption`

### Step 9: Cleanup
- [ ] Remove temporary test files (test-caption-with-blank.qmd, test-caption-no-blank.qmd)
- [ ] Run `cargo fmt` on modified Rust files
- [ ] Update beads issue k-185 with findings

## Files to Modify

1. **Grammar**: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
   - Modify caption rule to remove blank line requirement

2. **External Scanner**: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
   - Add caption line detection
   - Modify pipe table line ending logic to terminate before captions

3. **Tests**: Create new test files for caption parsing scenarios

## Success Criteria

- [ ] Tables with captions work WITHOUT blank line before caption
- [ ] Tables with captions work WITH blank line before caption (no regression)
- [ ] Caption is properly attached to table in final Pandoc AST
- [ ] All existing tests continue to pass
- [ ] New tests for caption scenarios all pass
- [ ] Tree-sitter tests pass
- [ ] Manual comparison with Pandoc shows matching behavior
- [ ] Edge cases handled correctly

## Risks and Considerations

### Risk: Breaking Fenced Divs
Fenced divs use `::` and `:::` at line start. We must ensure:
- Single `:` triggers caption detection
- Double `::` and triple `:::` do NOT trigger caption detection
- Test this explicitly

### Risk: Indented Contexts
Captions in block quotes or lists need special handling:
- The "line start" check must account for indentation markers
- External scanner already has block context tracking - leverage this

### Risk: Tree-Sitter Test Failures
Modifying the grammar may break existing tree-sitter tests:
- Only modify tests we explicitly add for this feature
- Do NOT change existing tests unless absolutely necessary
- If stuck, report back to user

### Risk: Postprocessing Assumptions
The postprocessing code (postprocess.rs:687-799) assumes:
- Caption comes after the table as a separate block
- This should still work, but verify the source location combining works correctly

## Notes

- The postprocessing code already handles caption attachment correctly
- The fix is primarily in the parser layer (grammar + external scanner)
- The design simplification (no definition lists) makes this fix clean and unambiguous
- Follow TDD: write tests first, verify they fail, then implement, then verify they pass

## Related Issues

- k-185: Table caption parsing fails without blank line before caption (this issue)

## References

- Pandoc manual on table captions
- Tree-sitter documentation on external scanners
- Existing caption parsing code in postprocess.rs
