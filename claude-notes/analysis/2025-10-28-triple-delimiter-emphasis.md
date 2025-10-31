# Triple Delimiter Emphasis/Strong Emphasis Issue

**Date:** 2025-10-28
**Status:** Documented, Workaround Available, Helper Rule Recommended

## The Problem

Markdown syntax `***bold italics***` (triple star or triple underscore) should parse as nested emphasis and strong emphasis but currently fails with parse errors.

**Examples that fail:**
```markdown
***bold italics***  (with stars)
___bold italics___  (with underscores)
```

**Expected behavior:** `Strong[Emph[text]]` (strong emphasis containing emphasis)
**Actual behavior:** Parse error

**Pandoc's output:**
```
[ Para [ Strong [ Emph [ Str "bold" , Space , Str "italics" ] ] ] ]
```

## Root Cause Analysis

### Scanner Behavior

The scanner in `scanner.c` (lines 340-366) handles star delimiters:

```c
static bool parse_star(TSLexer *lexer, const bool *valid_symbols) {
    lexer->advance(lexer, false);  // Consume first *
    if (lexer->lookahead == '*') {  // Check for second *
        lexer->advance(lexer, false);  // Consume second *

        // Emit ** as STRONG_EMPHASIS_OPEN or STRONG_EMPHASIS_CLOSE
        if (valid_symbols[STRONG_EMPHASIS_CLOSE_STAR]) {
            lexer->result_symbol = STRONG_EMPHASIS_CLOSE_STAR;
            return true;
        }
        if (valid_symbols[STRONG_EMPHASIS_OPEN_STAR]) {
            lexer->result_symbol = STRONG_EMPHASIS_OPEN_STAR;
            return true;
        }
        return false;
    }

    // Emit * as EMPHASIS_OPEN or EMPHASIS_CLOSE
    if (valid_symbols[EMPHASIS_CLOSE_STAR]) {
        lexer->result_symbol = EMPHASIS_CLOSE_STAR;
        return true;
    }
    if (valid_symbols[EMPHASIS_OPEN_STAR]) {
        lexer->result_symbol = EMPHASIS_OPEN_STAR;
        return true;
    }
    return false;
}
```

**Key observations:**
1. Scanner is greedy - it consumes `**` as a single token when possible
2. It checks CLOSE before OPEN
3. Each scanner call emits only ONE token (`**` or `*`), never both

### What Happens with `***bold italics***`

When parsing `***bold italics***`:

1. **Position 0** (`***`):
   - Scanner sees `*`, looks ahead, sees another `*`, consumes both
   - Emits `STRONG_EMPHASIS_OPEN_STAR` (for `**`)
   - Leaves one `*` at position 2

2. **Position 2** (`*bold`):
   - Grammar expects `_inline_no_star` content
   - Scanner sees `*`, looks ahead, sees `b` (not another `*`)
   - Could emit `EMPHASIS_OPEN_STAR`...
   - But scanner/grammar somehow can't make this work

3. **Position 15** (`***`):
   - Scanner sees `*`, looks ahead, sees another `*`, consumes both
   - Emits `STRONG_EMPHASIS_CLOSE_STAR` (for `**`)
   - Leaves one `*` at position 17

**Result:** We have:
- `**` (strong open) at 0-2
- `*` (orphaned) at 2-3
- text content
- `**` (strong close) at 15-17
- `*` (orphaned) at 17-18

The grammar can't assemble this into a valid tree because the orphaned `*` delimiters don't match up.

### Tree-Sitter Parse Output

```
(ERROR [0, 0] - [1, 0]
  (emphasis_delimiter [0, 0] - [0, 2])   <- **
  (emphasis_delimiter [0, 2] - [0, 3])   <- *
  (text_base [0, 3] - [0, 7])            <- "bold"
  (text_base [0, 7] - [0, 8])            <- " "
  (text_base [0, 8] - [0, 15])           <- "italics"
  (emphasis_delimiter [0, 15] - [0, 17]) <- **
  (emphasis_delimiter [0, 17] - [0, 18]) <- *
)
```

All tokens are recognized, but they can't be assembled into valid `emphasis` and `strong_emphasis` nodes.

### Grammar Structure

From `grammar.js` lines 508-511:

```javascript
grammar.rules['_emphasis_star' + suffix_link] = $ =>
    prec.dynamic(PRECEDENCE_LEVEL_EMPHASIS, seq(
        alias($._emphasis_open_star, $.emphasis_delimiter),
        optional($._last_token_punctuation),
        $['_inline_no_star' + suffix_link],
        alias($._emphasis_close_star, $.emphasis_delimiter)
    ));

grammar.rules['_strong_emphasis_star' + suffix_link] = $ =>
    prec.dynamic(2 * PRECEDENCE_LEVEL_EMPHASIS, seq(
        alias($._strong_emphasis_open_star, $.emphasis_delimiter),
        $['_inline_no_star' + suffix_link],
        alias($._strong_emphasis_close_star, $.emphasis_delimiter)
    ));
```

Both rules require `_inline_no_star` as content, which can recursively contain `_emphasis_star`. This SHOULD allow nesting, but the scanner's greedy `**` consumption prevents the correct token sequence from being emitted.

### Why the Grammar Can't Parse It

The grammar expects one of these token sequences:

**Option 1** (Strong containing Emphasis):
```
STRONG_EMPHASIS_OPEN (**)
  EMPHASIS_OPEN (*)
    content
  EMPHASIS_CLOSE (*)
STRONG_EMPHASIS_CLOSE (**)
```

**Option 2** (Emphasis containing Strong):
```
EMPHASIS_OPEN (*)
  STRONG_EMPHASIS_OPEN (**)
    content
  STRONG_EMPHASIS_CLOSE (**)
EMPHASIS_CLOSE (*)
```

But the scanner emits:
```
STRONG_EMPHASIS_OPEN (**) at position 0
* at position 2 (can't be matched as opener - grammar state is wrong)
content
STRONG_EMPHASIS_CLOSE (**) at position 15
* at position 17 (can't be matched as closer - no matching opener)
```

The problem is that after emitting `STRONG_EMPHASIS_OPEN`, the grammar is looking for `_inline_no_star` content. The single `*` at position 2 should trigger `EMPHASIS_OPEN_STAR`, but either:
1. `valid_symbols[EMPHASIS_OPEN_STAR]` is false at that position, OR
2. The scanner is called but returns false for some reason

## Why Fixing This Is Hard

### Challenge 1: Scanner Token Atomicity

Tree-sitter scanners emit one token per call. For `***` to work, we'd need:
- First call at position 0: emit `**` (STRONG_OPEN)
- Second call at position 2: emit `*` (EMPHASIS_OPEN)
- ...
- Third call after content: emit `*` (EMPHASIS_CLOSE)
- Fourth call: emit `**` (STRONG_CLOSE)

But the scanner doesn't maintain enough state to know "I already emitted part of this `***` sequence, so next time emit the remainder".

### Challenge 2: Greedy Matching

The scanner's greedy behavior (preferring `**` over `*`) is correct for most cases but problematic for `***`. We'd need context-sensitive logic:
- At start of `***`: emit `**`, then `*`
- At end of `***` when inside strong+emphasis: emit `*`, then `**`

### Challenge 3: Valid Symbols

The grammar controls what tokens are valid via `valid_symbols`. Even if the scanner wanted to emit `EMPHASIS_OPEN` at position 2, it can only do so if the grammar allows it. The interaction between grammar rules and scanner logic would need careful coordination.

## Workaround

Users can achieve bold italics by mixing delimiters:

**Instead of:**
```markdown
***bold italics***
```

**Use:**
```markdown
**_bold italics_**   (works!)
_**bold italics**_   (also works!)
```

Both parse correctly as `Strong[Emph[text]]`.

**Verification:**
```bash
$ echo '**_bold italics_**' | pandoc -f markdown -t native
[ Para [ Strong [ Emph [ Str "bold" , Space , Str "italics" ] ] ] ]
```

## Comparison to Code Span Issue

This issue is similar to the code span delimiter problem:
- Both involve delimiter sequences where individual delimiters can be consumed separately
- Both have architectural challenges in tree-sitter's lexer-parser separation
- Both have simple workarounds (use different delimiter counts/types)
- Both warrant user-facing documentation and helper warnings

## Recommendation

1. **Document this limitation** in user-facing docs
2. **Create qmd-syntax-helper rule** to detect `***` or `___` and suggest mixed delimiters
3. **Add to known limitations** documentation
4. **Do not attempt to fix** - the complexity outweighs the benefit given the easy workaround

## Detection Pattern for Helper Rule

Detect:
- Three or more consecutive `*` or `_` characters that appear to be emphasis delimiters
- Pattern: `***text***` or `___text___` where text doesn't contain the delimiter

Suggest:
```
Triple delimiter emphasis/strong emphasis not supported.
Current: ***bold italics***
Suggested: **_bold italics_**  or  _**bold italics**_

Use mixed delimiters (* and _) for combined bold and italic formatting.
```

## Related Test Cases

**Works:**
- `*italic*` ✓
- `**bold**` ✓
- `**_bold italic_**` ✓
- `_**bold italic**_` ✓
- `___bold italic___` if parsed as strong then emphasis (but fails currently)

**Fails:**
- `***bold italic***` ✗ (parse error)
- `___bold italic___` ✗ (unclosed emphasis error)

## References

- Scanner implementation: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/src/scanner.c` (lines 340-366)
- Grammar generation: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js` (lines 508-511)
- CommonMark spec on emphasis: https://spec.commonmark.org/0.30/#emphasis-and-strong-emphasis
- Test file: `/tmp/emphasis-test.qmd`
