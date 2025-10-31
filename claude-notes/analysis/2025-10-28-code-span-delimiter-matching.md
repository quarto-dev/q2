# Code Span Delimiter Matching Issue

**Date:** 2025-10-28
**Status:** Documented, Workaround Available, Warning Rule Needed

## The Problem

The CommonMark spec requires that code spans be closed by **exactly** the same number of backticks used to open them, with the additional requirement that the content must not contain a substring of backticks of exactly that length.

**Example that should work but doesn't:**
```markdown
` ```foo`
```

**Expected behavior:** Code span containing ` ```foo` (three backticks followed by "foo")
**Actual behavior:** Code span containing `` `` (two backticks), followed by plain text "foo"

## Root Cause Analysis

### Grammar Structure

The inline grammar defines code spans as:
```javascript
code_span: $ => prec.right(seq(
    alias($._code_span_start, $.code_span_delimiter),
    alias(repeat(choice($._code_span_text_base, $._soft_line_break)), $.code_content),
    alias($._code_span_close, $.code_span_delimiter),
))
```

And `_code_span_text_base` allows any punctuation:
```javascript
_code_span_text_base: $ => prec.right(choice(
    $._word,
    common.punctuation_without($, []),  // Includes backticks!
    $._whitespace,
)),
```

### The Issue

When parsing `` ` ```foo` ``:

1. **Opening:** Scanner sees 1 backtick at position 0, stores `delimiter_length=1`
2. **Content parsing:** Grammar encounters backticks at positions 2, 3, 4
3. **Problem:** Tree-sitter calls the scanner at each position:
   - Position 2: Scanner counts 3 consecutive backticks, checks `3 == 1` (NO), returns false
   - Grammar consumes position 2 as punctuation, advances
   - Position 3: Scanner counts 2 consecutive backticks, checks `2 == 1` (NO), returns false
   - Grammar consumes position 3 as punctuation, advances
   - Position 4: Scanner counts 1 backtick, checks `1 == 1` (YES), matches as closer!

The grammar's ability to consume individual backticks as punctuation breaks the "exactly N delimiters" matching requirement.

### Why Excluding Backticks from `_code_span_text_base` Doesn't Work

Attempted fix:
```javascript
common.punctuation_without($, ['`'])  // Exclude backticks
```

**Result:** Parse errors, because when the scanner correctly rejects ``` as a 1-backtick closer, the grammar has no rule to consume those backticks.

### Scanner Logic

The scanner has two responsibilities:

1. **Finding openers:** When `valid_symbols[CODE_SPAN_START]` is true, scan ahead to verify a matching closer exists
2. **Matching closers:** When `valid_symbols[CODE_SPAN_CLOSE]` is true, check if current position has exactly the right number of delimiters

The problem is in the closer-matching logic (lines 159-170 of scanner.c):
```c
uint8_t level = 0;
while (lexer->lookahead == delimiter) {
    lexer->advance(lexer, false);
    level++;
}
if (level == *delimiter_length && valid_symbols[close_token]) {
    // Match as closer
}
```

This works correctly when called at the START of a backtick sequence, but fails when the grammar has already consumed some backticks and calls the scanner mid-sequence.

## Attempted Fixes

### Fix Attempt 1: Improve opener-finding lookahead
**Change:** Modified the scan-ahead logic to only match when exactly `level` delimiters are followed by a non-delimiter
**Result:** Correct for finding openers, but doesn't fix the closer-matching problem
**Location:** Lines 171-201 of scanner.c

### Fix Attempt 2: Verify closer is not followed by more delimiters
**Change:** Added check that `lexer->lookahead != delimiter` after counting delimiters
**Result:** Doesn't help because grammar has already fragmented the delimiter sequence
**Location:** Lines 165-178 of scanner.c (attempted but reverted)

### Fix Attempt 3: Exclude backticks from code span content
**Change:** Modified grammar to use `common.punctuation_without($, ['`'])`
**Result:** Parse errors - no rule to consume backticks that aren't valid closers
**Location:** grammar.js line 454 (attempted but reverted)

## Architectural Challenge

The fundamental issue is that **tree-sitter's lexer-parser separation makes it difficult to implement "exactly N delimiters" matching when individual delimiters are valid tokens**.

For this to work correctly, we would need:
1. Backticks to NOT be consumable as individual punctuation tokens inside code spans, BUT
2. Sequences of backticks that don't match the delimiter length to somehow be consumable as content

This creates a chicken-and-egg problem: the scanner can't tell the grammar "consume these N backticks as content" without having a grammar rule that accepts them.

## Workaround

Users can work around this limitation by using more backticks in the delimiter:

**Instead of:**
```markdown
` ```foo`
```

**Use:**
```markdown
```` ```foo````
```

This works because:
- Opens with 4 backticks
- The ``` sequence (3 backticks) doesn't match 4, so grammar can consume them individually
- Closes with 4 backticks at the end

## Recommendation

1. **Document this limitation** in user-facing documentation
2. **Create a qmd-syntax-helper rule** that detects this pattern and suggests the workaround
3. **Add to known limitations** in CLAUDE.md or similar developer documentation

## Detection Pattern for Helper Rule

The problematic pattern is:
- Code span opens with N backticks
- Content contains a sequence of M backticks where M > N
- The final backtick of that sequence can be misinterpreted as a closer

**Detection strategy:**
Parse the QMD file, find code spans, check if:
1. The code span's opening delimiter is shorter than expected based on content
2. The content appears truncated (missing expected closing delimiter)
3. There's a backtick sequence immediately after the code span

**Suggested fix message:**
```
Code span delimiter too short for content with backticks.
Current: ` ```foo`
Suggested: ```` ```foo````

When code span content contains N backticks in a row, use at least N+1 backticks as delimiters.
```

## Related Issues

The same issue affects:
- **Latex spans** (`$` delimiters) - manifests as parse errors
- Any other leaf delimiter construct using `parse_leaf_delimiter()` in the scanner

## References

- CommonMark spec: https://spec.commonmark.org/0.30/#code-spans
- Scanner implementation: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/src/scanner.c`
- Grammar definition: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`
- Test case: `/tmp/code-span-test.qmd`
