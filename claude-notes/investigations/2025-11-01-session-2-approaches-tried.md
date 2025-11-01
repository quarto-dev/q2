# List Ending Bug - Session 2: Attempted Fixes

**Date**: 2025-11-01
**Issue**: k-315
**Previous Investigation**: claude-notes/investigations/2025-11-01-list-ending-bug.md

## Session Summary

Continued investigation from previous session. Tested Pandoc's actual behavior and attempted multiple implementation approaches. All approaches faced fundamental limitations with tree-sitter's lexer API.

## Pandoc Behavior Confirmation

Created test files and confirmed Pandoc's expected behavior:

```bash
# Test 1: `- a\n\nb`
pandoc output: [ BulletList [ [ Plain [ Str "a" ] ] ] , Para [ Str "b" ] ]
→ List ends, 'b' is separate paragraph

# Test 2: `- a\n\n  b` (2 spaces before b)
pandoc output: [ BulletList [ [ Para [ Str "a" ] , Para [ Str "b" ] ] ] ]
→ List continues, two paragraphs in one item

# Test 3: `- a\n\n- b`
pandoc output: [ BulletList [ [ Para [ Str "a" ] ] , [ Para [ Str "b" ] ] ] ]
→ One list with two items (not two separate lists)
```

**Key Insight**: The decision of whether a list continues after a blank line depends on what comes AFTER the blank line - requires lookahead.

## Approaches Attempted

### Approach 1: Lookahead Helper Function

**Strategy**: Create `list_continues_after_blank_line()` helper that:
1. Advances past blank lines
2. Checks if next line is indented enough or starts with list marker
3. Returns true/false to indicate if list continues

**Code Location**: Added before `match()` function (~60 lines)

**Problem**: Fundamental catch-22 with tree-sitter lexer:
- To determine if list continues, must look at content after blank lines
- Looking ahead requires advancing the lexer (no peek function)
- If list shouldn't continue, need blank line to be seen as paragraph interrupt by recursive `scan()`
- But we've already consumed the blank lines, so recursive `scan()` sees post-blank content instead
- This causes SOFT_LINE_ENDING to be returned when LINE_ENDING should be

**Result**: Complete parse failures with ERROR nodes

**Test Results**: N/A (parser produced errors)

### Approach 2: Context-Aware Paragraph Interrupts

**Strategy**: Modify where BLANK_LINE_START is recognized (lines 2393-2400):
1. During simulation, check if we're inside a list
2. If yes, don't return BLANK_LINE_START (blank lines not interrupts in lists during simulation)
3. This allows SOFT_LINE_ENDING to be set
4. List matching logic then handles continuation based on actual indentation

**Code Changes**:
```c
// In scan() at line 2395
if (valid_symbols[BLANK_LINE_START]) {
    if (s->simulate && s->open_blocks.size > 0) {
        // Check if any open block is a list item
        bool in_list = false;
        for (int i = 0; i < s->open_blocks.size; i++) {
            Block b = s->open_blocks.items[i];
            if (b >= LIST_ITEM && b <= LIST_ITEM_MAX_INDENTATION) {
                in_list = true;
                break;
            }
        }
        if (in_list) {
            break;  // Don't return BLANK_LINE_START
        }
    }
    lexer->result_symbol = BLANK_LINE_START;
    return true;
}

// In match() for LIST_ITEM at line 717
if ((lexer->lookahead == '\n' || lexer->lookahead == '\r') && !s->simulate) {
    s->indentation = 0;
    return true;
}
```

**Test Results**:
- **Total**: 355 passing (↑2), 26 failing (↓2)
- **Test 5** (list should end): ✗ Still failing
- **Test 6** (multi-paragraph item): ✗ Still failing
- **Test 7** (indented blank + content): ✗ NOW failing (was passing)
- **Test 8** (blank + list marker): ✗ Still failing

**Analysis**: Slightly better overall (2 more tests pass), but target tests still fail and introduced a regression in test 7.

## Root Cause Analysis

The fundamental problem is an **architectural limitation** in tree-sitter's lexer API:

1. **No Lookahead**: Tree-sitter provides no way to look ahead at upcoming characters without consuming them
2. **Ambiguity Requires Lookahead**: Blank lines in lists are ambiguous - their meaning depends on what follows
3. **Simulation Limitations**: Even during simulation (which is meant for lookahead), consuming characters affects the recursive `scan()` call that checks for paragraph interrupts

**The Catch-22**:
- Need to look ahead past blank lines to determine list continuation
- Looking ahead requires consuming (no peek API)
- If list shouldn't continue, need blank line visible to recursive scan as interrupt
- But we've consumed it, so it's not visible
- Can't "unconsume" or restore lexer state

## Comparison with Block Quotes

Block quotes work correctly because they don't have this ambiguity:

```markdown
> a

b
```

Block quote matching only succeeds when it finds `>`. No `>` on the blank line → match fails → block quote ends. No lookahead needed!

Lists are different:
- Blank lines CAN continue lists (multi-paragraph items)
- But they CAN also end lists (when followed by non-indented content)
- The distinction requires looking past the blank line

## Possible Solutions

### Option 1: Grammar-Level Changes

Instead of handling this in the scanner, modify the grammar to treat list structures differently. Allow blank lines to always be part of lists initially, then handle the "should this list have ended?" question in post-processing.

**Pros**: Avoids lexer lookahead problem
**Cons**: Major grammar changes, post-processing complexity

### Option 2: Stateful Blank Line Tracking

Add scanner state to track "expecting indented continuation" after blank lines in lists. Use this state to determine matching behavior on subsequent lines.

**Pros**: Stays within scanner
**Cons**: Complex state management, may still have edge cases

### Option 3: Accept Limitations

Document that the qmd parser handles multi-paragraph list items differently than Pandoc. Users must use explicit markers or indentation consistently.

**Pros**: No code changes
**Cons**: Incompatibility with Pandoc

### Option 4: Multi-Pass Parsing

Parse in multiple passes: first pass creates potentially-too-long lists, second pass identifies where they should have ended and splits them.

**Pros**: Can make correct decisions with full context
**Cons**: Performance impact, complexity

## Next Steps

1. **Discuss with maintainer**: This may require architectural decisions about acceptable trade-offs
2. **Consider grammar redesign**: May need to fundamentally restructure how lists are parsed
3. **Study other parsers**: How do other CommonMark parsers handle this?
4. **Prototype Option 2**: Stateful tracking might be feasible with careful design

## Files Modified

- `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`:
  - Lines 717-722: Modified LIST_ITEM blank line matching
  - Lines 2395-2418: Added context-aware BLANK_LINE_START logic

## Test Files Created

- `test-pandoc-1.md`: Test case 1
- `test-pandoc-2.md`: Test case 2
- `test-pandoc-3.md`: Test case 3
- `crates/tree-sitter-qmd/tree-sitter-markdown/test-simple.md`: Minimal test case

## Current Status

The bug remains unfixed. Multiple approaches attempted, all blocked by tree-sitter's lack of non-consuming lookahead. This appears to be a fundamental architectural challenge requiring either:
- Grammar-level restructuring, or
- Acceptance of behavioral differences from Pandoc, or
- A creative solution not yet discovered

**Recommendation**: Pause implementation attempts and consult with experienced tree-sitter developers or study how other markdown parsers solve this.
