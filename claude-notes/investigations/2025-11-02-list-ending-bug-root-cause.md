# List Ending Bug - Root Cause Analysis

**Date**: 2025-11-02
**Issue**: k-315
**Previous Sessions**:
- claude-notes/investigations/2025-11-01-list-ending-bug.md
- claude-notes/investigations/2025-11-01-session-2-approaches-tried.md

## Executive Summary

**BUG FOUND!** The issue is in `scanner.c` in the `match()` function for `LIST_ITEM` blocks (lines 578-581). List items incorrectly return `true` when encountering a newline, causing blank lines to always match as list continuations.

## The Discovery

### Minimal Test Cases

User provided two minimal test cases showing the discrepancy:

**Block quote (CORRECT behavior):**
```markdown
> a

b
```

Tree output:
```
(document
  (section
    (pandoc_block_quote [0, 0] - [1, 0]
      ...
      (pandoc_paragraph (pandoc_str)))  # 'a' paragraph
    (pandoc_paragraph (pandoc_str))))    # 'b' paragraph - OUTSIDE block quote
```

**List (INCORRECT behavior):**
```markdown
* a

b
```

Tree output:
```
(document
  (section
    (pandoc_list [0, 0] - [3, 0]
      (list_item [0, 0] - [3, 0]
        ...
        (pandoc_paragraph
          (pandoc_str)
          (block_continuation [1, 0] - [1, 0]))  # <-- SUSPICIOUS!
        (pandoc_paragraph (pandoc_str))))))       # 'b' paragraph - INSIDE list item
```

### Key Observation

The list case has a `block_continuation` node at `[1, 0] - [1, 0]` (the blank line position) inside the first paragraph. This signals that the list should continue, causing 'b' to be included in the list item.

## Root Cause Analysis

### How block_continuation Works

From `grammar.js` comments (lines 707-715):
> "After every newline (`$._line_ending`) we try to match as many open blocks as possible. For example if the last line was part of a block quote we look for a `>` at the beginning of the next line. We emit a `$.block_continuation` for each matched block."

The `block_continuation` token is emitted by the scanner when an open block successfully matches on the next line after a newline.

### The match() Function

**scanner.c lines 549-602**: The `match()` function determines whether an open block continues on the current line.

#### LIST_ITEM matching (lines 551-582):

```c
case LIST_ITEM:
case LIST_ITEM_1_INDENTATION:
// ... (all indentation cases)
    while (s->indentation < list_item_indentation(block)) {
        if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            s->indentation += advance(s, lexer);
        } else {
            break;
        }
    }
    if (s->indentation >= list_item_indentation(block)) {
        s->indentation -= list_item_indentation(block);
        return true;
    }
    if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {  // <-- LINE 578
        s->indentation = 0;
        return true;  // <-- LINE 580: THE BUG!
    }
    break;
```

#### BLOCK_QUOTE matching (lines 583-595):

```c
case BLOCK_QUOTE:
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        s->indentation += advance(s, lexer);
    }
    if (lexer->lookahead == '>') {
        advance(s, lexer);
        s->indentation = 0;
        if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            s->indentation += advance(s, lexer) - 1;
        }
        return true;
    }
    break;  // <-- Falls through to return false if no '>' found
```

### The Critical Difference

**LIST_ITEM**: Lines 578-581 return `true` when `lookahead` is a newline (`\n` or `\r`). This means **blank lines ALWAYS match list item continuations!**

**BLOCK_QUOTE**: Only returns `true` if it finds a `>` marker. If there's no `>` marker (e.g., on a blank line), it breaks out and returns `false` at line 601.

## Execution Flow for `* a\n\nb`

1. Parser sees `* a\n` and creates a list item (pushed to `open_blocks`)
2. After the newline, scanner enters matching mode (line 2282 in scan())
3. Scanner calls `match(s, lexer, LIST_ITEM)` to check if list continues
4. At this point, lexer is positioned at the blank line
5. The `while` loop (lines 567-573) processes any indentation (none in this case)
6. Check at line 574 fails (indentation is 0, not >= required indentation)
7. Check at line 578: `lexer->lookahead == '\n'` is TRUE (blank line)
8. **BUG**: Returns `true` at line 580
9. Back in scan() at line 2289, match succeeded: `partial_success = true`
10. Scanner emits `BLOCK_CONTINUATION` at line 2303
11. Grammar interprets this as list continuing
12. The 'b' paragraph gets parsed inside the list item

## Why This Is Wrong

According to CommonMark/Pandoc:
- `* a\n\nb` should END the list (b is separate paragraph)
- `* a\n\n  b` should CONTINUE the list (b is indented, part of item)

The blank line alone should NOT cause continuation. The decision depends on what comes AFTER the blank line.

However, the current code treats **any** blank line in a list as a continuation marker, which is incorrect.

## The Fix

**Remove the special case for newlines in LIST_ITEM matching.**

Lines 578-581 should be deleted:

```c
// REMOVE THESE LINES:
if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    s->indentation = 0;
    return true;
}
```

After this fix:
- If a line has sufficient indentation → match succeeds (returns true at line 576)
- If a line lacks indentation → match fails (breaks at line 582, returns false at line 601)
- Blank lines will be treated like any other line: they must meet the indentation requirement

## Expected Behavior After Fix

For `* a\n\nb`:
1. After first newline, list item is open
2. At blank line: `match()` checks indentation
3. Indentation is 0, less than required → returns `false`
4. Scanner emits `BLOCK_CLOSE` instead of `BLOCK_CONTINUATION`
5. List item closes
6. 'b' paragraph is parsed outside the list

## Questions Answered

**Q: Why was this code there in the first place?**
A: Likely an attempt to handle multi-paragraph list items. However, it's too permissive - it matches ALL blank lines, not just those followed by properly indented content.

**Q: Won't this break multi-paragraph list items?**
A: No! The case `* a\n\n  b` (with indentation on the 'b' line) will still work:
- At the blank line: match fails (no indentation)
- List doesn't close yet (lazy continuation logic)
- At the '  b' line: match succeeds (has indentation) → continues list

**Q: How do we know block quotes don't have the same issue?**
A: Block quotes require an explicit `>` marker to continue. They don't have a "blank lines always match" rule. That's why the minimal test case for block quotes works correctly.

## Implementation Notes

**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
**Lines to delete**: 578-581
**After fix**: Rebuild parser with `tree-sitter generate && tree-sitter build`
**Test**: Run `tree-sitter test` to verify no regressions

## Files Modified

- `crates/tree-sitter-qmd/tree-sitter-markdown/test-blockquote.md` (created for testing)
- `crates/tree-sitter-qmd/tree-sitter-markdown/test-list.md` (created for testing)

## Next Steps

1. Create tree-sitter test case
2. Verify test fails
3. Remove lines 578-581 from scanner.c
4. Rebuild parser
5. Verify test passes
6. Run full test suite
7. Update issue k-315
