# Fix Plan: Keycap Emoji Support (k-368)

**Date**: 2025-11-20
**Issue**: k-368 - Fix parser handling of multi-byte emoji characters
**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`

## Problem Analysis

### Current State

The grammar has emoji support via this regex (line 61):
```javascript
const EMOJI_REGEX = "(\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*)";
```

This correctly handles:
- Regular emojis: 🍎 🤗 👍 (in `\p{Extended_Pictographic}`)
- Skin tone modifiers: 👍🏽 (with `\p{Emoji_Modifier}`)
- Variation selectors: 🗓️ (with `\uFE0F`)
- ZWJ sequences: 👨‍👩‍👧‍👦 (with `\u200D`)

### What Fails

**Keycap emojis** like 1️⃣ 2️⃣ 3️⃣ #️⃣ *️⃣ fail because:

1. Base character is ASCII (`0-9`, `#`, `*`) - NOT in `\p{Extended_Pictographic}`
2. Structure: `[0-9#*] + U+FE0F + U+20E3`
   - Base: ASCII character
   - U+FE0F: Variation Selector-16 (optional but usually present)
   - U+20E3: Combining Enclosing Keycap (required)

**Example byte structure of "1️⃣"**:
```
31        - '1' (U+0031)
ef b8 8f  - U+FE0F (Variation Selector-16)
e2 83 a3  - U+20E3 (Combining Enclosing Keycap)
```

### Parse Behavior

With current grammar:
```
Input:  1️⃣ First item
Parse:  (ERROR
          (pandoc_str "1")    <- Only the '1' is recognized
          (shortcode_name)    <- U+FE0F+U+20E3 confuse the parser
          ...)
```

With working emoji (🍎):
```
Input:  🍎 Apple
Parse:  (pandoc_paragraph
          (pandoc_str "🍎")   <- Entire emoji recognized correctly
          ...)
```

## Solution

Add support for keycap emoji sequence as a separate pattern.

### Keycap Emoji Pattern

According to Unicode TR51 (Unicode Emoji):
- Valid base characters: `0 1 2 3 4 5 6 7 8 9 # *` (12 characters total)
- Sequence: `base + [U+FE0F] + U+20E3`
- U+FE0F is technically optional but nearly always present in practice

**Regex**: `[0-9#*]\uFE0F?\u20E3`

This is:
- **Precise**: Only matches the 12 valid keycap emoji sequences
- **Safe**: Doesn't add broad Unicode categories
- **Complete**: Covers all keycap emojis (0️⃣-9️⃣, #️⃣, *️⃣)

### Implementation

**Location**: `grammar.js` lines 60-66

**Change**:
```javascript
// BEFORE:
const EMOJI_REGEX = "(\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*)";

const PANDOC_REGEX_STR =
    regexOr(
        "\\\\.",
        EMOJI_REGEX,        // <- Only one emoji type
        ...
```

```javascript
// AFTER:
// Keycap emojis: 0-9, #, * + optional U+FE0F + U+20E3
const KEYCAP_EMOJI_REGEX = "([0-9#*]\\uFE0F?\\u20E3)";

// Regular emojis with Extended_Pictographic property
const EMOJI_REGEX = "(\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*)";

const PANDOC_REGEX_STR =
    regexOr(
        "\\\\.",
        KEYCAP_EMOJI_REGEX, // <- Add keycap emojis FIRST (more specific)
        EMOJI_REGEX,        // <- Then regular emojis
        ...
```

**Order matters**: Keycap regex must come BEFORE general emoji regex to match first.

## Testing Strategy

### 1. Add Test Case to qmd.txt

Add after existing emoji test (around line 760):

```
================================================================================
Keycap Emojis
================================================================================
0️⃣ 1️⃣ 2️⃣ 3️⃣ 4️⃣ 5️⃣ 6️⃣ 7️⃣ 8️⃣ 9️⃣ #️⃣ *️⃣
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str)
          (pandoc_space)
          (pandoc_str))))
```

### 2. Rebuild and Test

```bash
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter generate
tree-sitter build
tree-sitter test
```

### 3. Verify with test files

```bash
tree-sitter parse test-emoji-ts.md
tree-sitter parse test-all-emojis.md
```

### 4. Test in qmd parser

```bash
cargo run --bin quarto-markdown-pandoc -- -i test-emoji-ts.md
```

### 5. Re-run corpus validation

Expected impact:
- ~29 errors across 3+ files should be fixed
- Files should move from uncoded-errors to clean

## Safety Considerations

### Why This is Safe

1. **Minimal scope**: Only adds 12 specific character sequences
2. **No broad Unicode categories**: Uses explicit character class `[0-9#*]`
3. **Follows Unicode standard**: Matches TR51 keycap emoji definition
4. **Tested pattern**: Keycap emojis are well-established (since Unicode 6.0)

### What We're NOT Doing

- ❌ Adding `\p{Emoji}` (too broad, includes many non-pictographic characters)
- ❌ Adding all combining characters (dangerous)
- ❌ Adding arbitrary digit+modifier combinations
- ✅ Adding only the 12 Unicode-defined keycap sequences

### Regex Breakdown

```javascript
[0-9#*]    // Exactly 12 characters: 0123456789#*
\uFE0F?    // Optional Variation Selector-16 (usually present)
\u20E3     // Required Combining Enclosing Keycap
```

Matches exactly: 0️⃣ 1️⃣ 2️⃣ 3️⃣ 4️⃣ 5️⃣ 6️⃣ 7️⃣ 8️⃣ 9️⃣ #️⃣ *️⃣

And their variants without U+FE0F (rare but valid): 0⃣ 1⃣ 2⃣ etc.

## Implementation Steps

1. ✅ **DONE**: Investigate and understand the problem
2. **TODO**: Make grammar.js changes
3. **TODO**: Add test case to qmd.txt
4. **TODO**: Run `tree-sitter generate && tree-sitter build`
5. **TODO**: Run `tree-sitter test` and verify new test passes
6. **TODO**: Test with actual corpus files
7. **TODO**: Run full corpus validation
8. **TODO**: Verify no regressions (all existing tests still pass)

## Expected Results

- **Before**: 18 uncoded errors
- **After**: ~9 uncoded errors (50% reduction)
- **Files fixed**: ~3 files with 29+ errors
- **Test coverage**: Keycap emojis now tested in grammar

## References

- Unicode TR51: https://unicode.org/reports/tr51/
- Keycap Emoji Sequences: https://unicode.org/emoji/charts/emoji-sequences.html#keycap
- Current emoji test: `tree-sitter-markdown/test/corpus/qmd.txt` line 738
