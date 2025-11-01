# List Item Block Ending Bug Investigation

**Date**: 2025-11-01
**Issue**: k-315
**Status**: In Progress

## Problem Statement

List items never properly "end" after blank lines followed by non-indented content. The minimal example:

```markdown
- a

b
```

**Expected**: List ends after blank line, `b` is a separate paragraph
**Actual**: List item continues to include `b`

This contrasts with block quotes, which work correctly:

```markdown
> a

b
```

Block quote ends properly and `b` is a separate paragraph.

## Investigation Process

### 1. Initial Test Case Creation

Added test case to `test/corpus/list.txt`:

```
================================================================================
5 (list should end after blank line when followed by non-indented content)
================================================================================
- a

b
--------------------------------------------------------------------------------
(document
  (section
    (pandoc_list
      (list_item
        (list_marker_minus)
        (pandoc_paragraph
          (pandoc_str))))
    (pandoc_paragraph
      (pandoc_str))))
```

**Result**: Test fails - list item incorrectly contains both paragraphs.

### 2. Comparative Analysis

Created minimal test files and compared parse trees:

**Block Quote** (CORRECT):
```
(pandoc_block_quote [0, 0] - [1, 0]
  ...
  (pandoc_paragraph [0, 2] - [1, 0]
    (pandoc_str [0, 2] - [0, 3])))
(pandoc_paragraph [2, 0] - [2, 1]
  (pandoc_str [2, 0] - [2, 1])))
```

**List** (WRONG):
```
(pandoc_list [0, 0] - [2, 1]
  (list_item [0, 0] - [2, 1]
    ...
    (pandoc_paragraph [0, 2] - [1, 0]
      (pandoc_str [0, 2] - [0, 3])
      (block_continuation [1, 0] - [1, 0]))
    (pandoc_paragraph [2, 0] - [2, 1]
      (pandoc_str [2, 0] - [2, 1]))))
```

The list_item extends to [2, 1] and incorrectly contains both paragraphs.

### 3. Scanner Code Analysis

Located the issue in `src/scanner.c`, in the `match()` function around line 690-720:

**LIST_ITEM matching logic** (PROBLEMATIC):
```c
case LIST_ITEM:
    // ... indentation checking ...
    if (s->indentation >= list_item_indentation(block)) {
        s->indentation -= list_item_indentation(block);
        return true;
    }
    // PROBLEM: Unconditionally match blank lines
    if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
        s->indentation = 0;
        return true;  // Always returns true for blank lines!
    }
    break;
```

**BLOCK_QUOTE matching logic** (CORRECT):
```c
case BLOCK_QUOTE:
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        s->indentation += advance(s, lexer);
    }
    if (lexer->lookahead == '>') {
        advance(s, lexer);
        // ... handle optional space after '>' ...
        return true;
    }
    break;  // No '>' marker means no match
```

Block quotes only match when they find the `>` marker. There's no special case for blank lines.

### 4. Understanding the State Machine

The scanner uses a complex state machine with these key components:

- **STATE_MATCHING**: Currently trying to match open blocks
- **STATE_WAS_SOFT_LINE_BREAK**: Previous line break was soft (within same block)
- **simulate**: Lookahead mode to determine soft vs hard line breaks

**Key Code Flow** (lines 2506-2577):

1. When in STATE_MATCHING, try to match all open blocks
2. If match fails and NOT STATE_WAS_SOFT_LINE_BREAK, return BLOCK_CLOSE
3. If match fails and IS STATE_WAS_SOFT_LINE_BREAK, don't close (continue)

**The Problem**:
- Line 717-720: Blank lines always match, setting `partial_success = true`
- This prevents BLOCK_CLOSE from being returned
- Even when followed by non-indented content, the list stays open

### 5. Comprehensive Test Cases

Added more test cases to understand the complete behavior:

```markdown
# Test 5: List should end after blank line + non-indented content
- a

b
→ Expected: Two separate blocks (list, paragraph)
→ Status: ✓ PASSES with fix

# Test 6: List should continue with blank line + indented content
- a

  b
→ Expected: One list with two paragraphs in single item
→ Status: ✗ FAILS - list closes incorrectly

# Test 7: List continues with indented blank line + indented content
- a

  b
→ Expected: One list with two paragraphs
→ Status: ✓ PASSES

# Test 8: Blank line + list marker should stay in same list
- a

- b
→ Expected: One list with two items
→ Status: ✗ FAILS - parses as two separate lists
```

### 6. Fix Attempts

#### Attempt 1: Remove blank line matching entirely
```c
// Removed lines 717-720 completely
```
**Result**: Test 5 passes, but 26 other tests fail (including GFM examples with tabs/indented continuations)

#### Attempt 2: Only match blank lines during simulation
```c
if ((lexer->lookahead == '\n' || lexer->lookahead == '\r') && s->simulate) {
    s->indentation = 0;
    return true;
}
```
**Result**: Test 5 passes, test 6 fails (multi-paragraph list items broken)

#### Attempt 3: Match blank lines during simulation OR after soft line break
```c
if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    if (s->simulate || (s->state & STATE_WAS_SOFT_LINE_BREAK)) {
        s->indentation = 0;
        return true;
    }
}
```
**Result**: Same as Attempt 2 - test 6 still fails

## Root Cause Analysis

The core issue is more subtle than initially thought:

### The Simulation Problem

When the scanner encounters a line break, it simulates ahead to determine if it should be a SOFT_LINE_ENDING or LINE_ENDING (lines 2604-2648):

```c
s->simulate = true;
// Try to match all open blocks with upcoming content
while (s->matched < s->open_blocks.size) {
    if (match(s, lexer, s->open_blocks.items[s->matched])) {
        s->matched++;
        one_will_be_matched = true;
    } else {
        break;
    }
}

// Then recursively scan with paragraph_interrupt_symbols
const bool *symbols = paragraph_interrupt_symbols;
if (!scan(s, lexer, symbols)) {
    // No paragraph interrupt found
    if (one_will_be_matched) {
        s->state |= STATE_MATCHING;
    }
    lexer->result_symbol = SOFT_LINE_ENDING;
    s->state |= STATE_WAS_SOFT_LINE_BREAK;
    return true;
}
```

### The Issue

In `paragraph_interrupt_symbols` (line 402), `BLANK_LINE_START` is set to `true`:

```c
static const bool paragraph_interrupt_symbols[] = {
    // ...
    true,  // BLANK_LINE_START (line 402)
    // ...
};
```

This means:
1. During simulation after `- a\n`, blank line matches the list item
2. Then recursive `scan()` is called with `paragraph_interrupt_symbols`
3. The blank line is recognized as BLANK_LINE_START (a paragraph interrupt)
4. Because an interrupt was found, SOFT_LINE_ENDING is NOT returned
5. STATE_WAS_SOFT_LINE_BREAK never gets set
6. Later, when trying to match the blank line during actual matching, even if we check for STATE_WAS_SOFT_LINE_BREAK, it won't be set!

### Why Block Quotes Work

Block quotes don't have the blank line special case. They only match when they find `>`:

```c
case BLOCK_QUOTE:
    // ... skip whitespace ...
    if (lexer->lookahead == '>') {
        return true;
    }
    break;  // Blank lines just fall through and don't match
```

So for `> a\n\nb`:
1. Simulation tries to match blank line against block quote
2. No `>` found, match fails
3. No matching means block quote doesn't continue
4. BLOCK_CLOSE is returned properly

## Technical Details

### Key Scanner State Variables

- `s->state`: Bit flags including STATE_MATCHING, STATE_WAS_SOFT_LINE_BREAK
- `s->simulate`: Boolean, true when doing lookahead
- `s->matched`: Count of how many blocks have been matched
- `s->open_blocks`: Stack of currently open block types
- `s->indentation`: Current indentation level

### Code Locations

- `src/scanner.c:674-741` - `match()` function with LIST_ITEM logic
- `src/scanner.c:2506-2577` - STATE_MATCHING block matching logic
- `src/scanner.c:2579-2649` - Line ending logic with simulation
- `src/scanner.c:280-330` - `display_math_paragraph_interrupt_symbols`
- `src/scanner.c:372-422` - `paragraph_interrupt_symbols`

### List Item Indentation

```c
static uint8_t list_item_indentation(Block block) {
    return (uint8_t)(block - LIST_ITEM + 2);
}
```

List items require continuation content to be indented by at least 2 spaces (for `-`/`*`/`+`) or more for numbered lists.

## Current Status

**Passing Tests**:
- Test 5: Blank line + non-indented content correctly closes list ✓
- Test 7: Indented blank line + indented content continues list ✓
- Test 1-4: Original list tests still pass ✓

**Failing Tests**:
- Test 6: Blank line + indented content should continue but closes ✗
- Test 8: Blank line + list marker should be one list, creates two ✗
- Various GFM spec tests (exact count TBD) ✗

**Overall Test Suite**: ~25 failures out of ~365 tests

## Possible Solutions

### Option 1: Change Paragraph Interrupt Symbols for Lists

Modify `paragraph_interrupt_symbols` to NOT treat blank lines as interrupts when inside a list. This would allow SOFT_LINE_ENDING to be set.

**Complexity**: Would need context-awareness in symbol selection.

### Option 2: Look-Ahead Past Blank Lines

During blank line matching, look ahead to see what follows:
- If non-indented non-marker content → don't match
- If indented content → match
- If list marker → match (for multi-item lists)

**Complexity**: Requires additional lookahead logic in `match()`.

### Option 3: Two-Phase Matching

Separate the "can this line continue the block?" from "does this specific line match the block marker?". Blank lines would be "can continue" but not "does match".

**Complexity**: Significant refactoring of match semantics.

### Option 4: State-Based Blank Line Handling

Track whether we're in a "between paragraphs" state within a list item. Only allow blank lines to continue in this specific state.

**Complexity**: Additional state management.

## Recommended Next Steps

1. **Further Investigation**: Instrument the simulation phase to understand exactly when and why STATE_WAS_SOFT_LINE_BREAK should be set for multi-paragraph list items

2. **Study Pandoc Behavior**: Test these cases in actual Pandoc to confirm expected behavior:
   - `- a\n\n  b` (blank + indented)
   - `- a\n\n- b` (blank + marker)
   - Various indentation levels

3. **Explore Option 2**: Look-ahead past blank lines seems most surgical
   - Minimal changes to existing logic
   - Directly addresses the ambiguity
   - Similar to how block quotes work (looking for specific markers)

4. **Consider Lazy Continuation**: Pandoc's markdown has "lazy continuation" rules where indentation requirements are relaxed in some contexts. This may be relevant.

## References

- Original issue: k-315
- Test file: `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/list.txt`
- Scanner implementation: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`

## Debug Artifacts

Debug output has been added to scanner.c (controlled by `#define SCAN_DEBUG 1`):
- Match function shows indentation checks and decisions
- State matching shows block matching attempts
- Simulation phase shows lookahead results

Test files created:
- `/Users/cscheid/repos/github/cscheid/kyoto/test-list.md` - Basic failing case
- `/Users/cscheid/repos/github/cscheid/kyoto/test-list-6.md` - Multi-paragraph case
- `/Users/cscheid/repos/github/cscheid/kyoto/test-blockquote.md` - Working comparison
