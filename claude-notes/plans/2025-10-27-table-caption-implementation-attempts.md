# Table Caption Implementation Attempts (k-185)

**Date**: 2025-10-27
**Issue**: k-185 - Table caption parsing fails without blank line before caption
**Status**: In progress - multiple approaches attempted

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

## Root Cause

The caption grammar rule originally required `$._blank_line` as first element. When no blank line precedes `: Caption`, the pipe table parser treats `:` as valid punctuation in cell contents, creating an extra table row instead of a caption.

## Tests Created ✅

Created 4 test files in `crates/quarto-markdown-pandoc/tests/snapshots/native/`:
1. `table-caption-no-blank-line.qmd` - Caption without blank line (currently fails)
2. `table-caption-with-blank-line.qmd` - Caption with blank line (works)
3. `table-fenced-div-no-blank.qmd` - Table + fenced div, no blank line
4. `table-fenced-div-with-blank.qmd` - Table + fenced div, with blank line

## Approach 1: Scanner + Grammar Modification (INCOMPLETE)

### Changes Made

**Grammar changes** (`grammar.js`):
1. Removed `$._blank_line` requirement from caption rule:
   ```javascript
   caption: $ => prec.right(seq(
       ':',  // No longer requires blank line
       optional(seq($._whitespace, alias($._caption_line, $.inline))),
       choice($._newline, $._eof),
   )),
   ```

2. Made caption part of pipe_table's closing sequence:
   ```javascript
   pipe_table: $ => prec.right(seq(
       $._pipe_table_start,
       alias($.pipe_table_row, $.pipe_table_header),
       $._newline,
       $.pipe_table_delimiter_row,
       repeat(seq($._pipe_table_newline, optional($.pipe_table_row))),
       choice(
           seq($._newline, optional($.caption)),
           $._eof
       ),
   )),
   ```

3. Prevented `pipe_table_row` from matching lines starting with `:`:
   ```javascript
   pipe_table_row: $ => seq(
       optional(seq(optional($._whitespace), '|')),
       choice(
           // ... rows with pipes ...
           // Row without starting pipe, but MUST NOT start with ':'
           prec(-1, seq(
               optional($._whitespace),
               alias($._pipe_table_cell_not_colon, $.pipe_table_cell),
               optional($._whitespace)
           ))
       ),
   ),

   _pipe_table_cell_not_colon: $ => prec.right(seq(
       choice(
           $._word,
           // ...
           common.punctuation_without($, ['|', ':']),  // Exclude ':' from first char
       ),
       repeat(choice(
           // ...
           common.punctuation_without($, ['|']),  // ':' OK after first char
       ))
   )),
   ```

**Scanner attempts** (`scanner.c`):

Tried multiple locations/approaches to check for `:` at line start:

1. **Attempt 1**: Check at line 1721-1730 (after recursive scan, in PIPE_TABLE_LINE_ENDING emission)
   - Checked `lexer->lookahead != ':'` before emitting
   - Issue: By this point, row already parsed

2. **Attempt 2**: Check before indentation processing (line 1678-1706)
   - Added early check for `:` after skipping whitespace
   - Issue: Can't rewind lexer after advancing through whitespace

3. **Attempt 3**: Emit LINE_ENDING instead of PIPE_TABLE_LINE_ENDING when detecting `:`
   - Issue: Multiple code paths, state management complex

### Results

- ✅ Tree-sitter tests pass (255/255)
- ✅ Grammar prevents `:` lines from being parsed as table rows
- ✅ Tables with blank line + caption work perfectly
- ❌ Tables without blank line + caption cause parse errors in caption text
- ❌ Scanner changes don't successfully close table and allow caption to parse

### Why It Didn't Work

The scanner's control flow is complex:
1. Newline consumption happens early
2. Indentation skipping happens
3. Recursive `scan()` called to check for paragraph-interrupting blocks
4. Multiple state flags control behavior (`STATE_MATCHING`, `STATE_WAS_SOFT_LINE_BREAK`, etc.)
5. By the time we check `lexer->lookahead`, we've already advanced past whitespace
6. The `repeat(seq($._pipe_table_newline, optional($.pipe_table_row)))` in pipe_table needs `_pipe_table_line_ending` to NOT be emitted to stop

The coordination between:
- Not emitting `PIPE_TABLE_LINE_ENDING`
- Properly closing the table
- Allowing caption to parse as part of table's closing sequence

proved more intricate than initially anticipated.

## Key Insights

1. **Grammar-only won't work**: Can't prevent `:` from matching as row content without scanner help
2. **Scanner-only won't work**: Grammar must also be modified to make caption part of table structure
3. **Both needed**: Combined approach required, but coordination is complex
4. **Parse tree shows**: When working, caption is separate block after table; when not working, `:` becomes table row content
5. **Fenced divs**: The scanner check for `:` successfully prevents `::` and `:::` from continuing tables (causes parse error as expected)

## Current State

**Grammar changes**: IN PLACE and working for blank-line case
**Scanner changes**: Multiple attempts, currently has incomplete/broken check
**Tests**: Created and verified they fail correctly

**What works**:
- Caption rule accepts lines without preceding blank line ✅
- pipe_table_row grammar rejects lines starting with `:` ✅
- Table + caption with blank line parses correctly ✅
- Tree-sitter test suite passes ✅

**What doesn't work**:
- Scanner doesn't successfully close table when `:` line follows ❌
- Caption without blank line causes parse error ❌

## File Changes

Modified files:
1. `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` - Grammar changes
2. `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` - Scanner changes (incomplete)
3. Test files created in `crates/quarto-markdown-pandoc/tests/snapshots/native/`

## Next Steps / Alternative Approaches

Potential approaches to try:

1. **More careful scanner analysis**:
   - Trace exact execution flow with debugging output
   - Understand all state transitions
   - Find the right place to check for `:` that doesn't break rewind

2. **Use external scanner token for caption start**:
   - Have scanner emit a special token when it sees `:` at line start
   - Grammar uses this token to close table and start caption
   - Avoids complex state management

3. **Make caption interrupt tables at grammar level**:
   - Add caption to `paragraph_interrupt_symbols`
   - Use precedence to make caption win over pipe_table_row
   - May be simpler than scanner approach

4. **Accept current limitation**:
   - Keep blank line requirement for captions
   - Document as intentional difference from Pandoc
   - Avoids complexity, but doesn't match Pandoc behavior

5. **Different grammar structure**:
   - Instead of `optional($.caption)` at end of pipe_table
   - Make caption a required part that can be empty?
   - Or use a wrapper that combines table + optional caption?

## Lessons Learned

- Tree-sitter external scanners are powerful but have complex state management
- Lexer can't easily rewind, so checks must happen at right time
- Combined grammar + scanner changes require careful coordination
- Test-driven development approach was valuable for verifying behavior
- Some features that seem simple may have deep implementation complexity

## References

- Original plan: `claude-notes/plans/2025-10-27-table-caption-blank-line-fix.md`
- Beads issue: k-185
- Tree-sitter docs: External scanners
- Scanner code: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
