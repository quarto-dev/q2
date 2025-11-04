# Unicode Diacritics and Non-ASCII Character Support Investigation

Date: 2025-11-04
File: claude-notes/plans/2025-11-04-unicode-diacritics-support.md

## Purpose

Investigate and fix the grammar to support unicode diacritics and non-ascii characters in text. Currently, characters like "ô" in "Antônio" cause parse errors.

## Investigation Steps

### 1. Initial Reproduction ✅

**Objective**: Confirm the failure and capture error details

**Command**:
```bash
cargo run -p quarto-markdown-pandoc -- -i /Users/cscheid/today/diacritics.qmd
```

**Result**: Parse error at position 1:15 (the "ô" character)
```
Error: Parse error
   ╭─[/Users/cscheid/today/diacritics.qmd:1:15]
   │
 1 │ My name is Antônio.
   │               ┬
   │               ╰── unexpected character or token here
───╯
```

**Error Details**:
- Error location: Line 1, column 15
- Character: "ô" (U+00F4, UTF-8: 0xC3 0xB4, 2 bytes)
- The problematic construct: "Antônio"

### 2. Create Minimal Test Case ✅

**File**: `/Users/cscheid/today/diacritics.qmd`
```qmd
My name is Antônio.
```

**Verification**: ✅ Reproduces the error

### 3. Analyze Tree-Sitter Parse Tree ✅

**Command**:
```bash
cargo run -p quarto-markdown-pandoc -- -i /Users/cscheid/today/diacritics.qmd -v 2>&1 | tail -100
```

**Observations**:
- Parser successfully lexes "Ant" as `pandoc_str` (columns 11-14)
- At column 14, encounters "ô" character
- Lexes `_error` token (size 0)
- Skips 2 bytes (the UTF-8 encoded "ô": 0xC3 0xB4)
- Continues with "nio." as separate tokens

**Parse sequence**:
```
✓ "My"      → pandoc_str (col 0-2)
✓ " "       → _whitespace (col 2-3)
✓ "name"    → pandoc_str (col 3-7)
✓ " "       → _whitespace (col 7-8)
✓ "is"      → pandoc_str (col 8-10)
✓ " "       → _whitespace (col 10-11)
✓ "Ant"     → pandoc_str (col 11-14)
✗ "ô"       → _error (col 14-16, 2 bytes)
✓ "nio."    → continues after skipping
```

### 4. Verify Character Positions ✅

**Command**:
```bash
echo -n "My name is Antônio." | od -c
```

**Output**:
```
0000000    M   y       n   a   m   e       i   s       A   n   t   ô  **
0000020    n   i   o   .
```

The "ô" is a multi-byte UTF-8 character (2 bytes: 0xC3 0xB4), confirming the parser's column jump from 14 to 16.

### 5. Classify the Bug ✅

**Decision**: Grammar bug

**Reasoning**:
- The tree-sitter grammar does not recognize non-ASCII characters as valid `pandoc_str` content
- The grammar explicitly limits characters to ASCII range: `A-Za-z`
- This is a limitation in the grammar definition itself

**Classification**: Grammar bug in `tree-sitter-qmd`

### 6A. Grammar Investigation ✅

**Location**: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:559`

**Current Definition**:
```javascript
pandoc_str: $ => /(?:[\u{00A0}0-9A-Za-z%&()+-/]|\\.)(?:[\u{00A0}0-9A-Za-z!%&()+,./;?:-]|\\.|['][0-9A-Za-z])*/,
```

**Analysis**:
- First character class: `[\u{00A0}0-9A-Za-z%&()+-/]` - Only ASCII letters!
- Continuation character class: `[\u{00A0}0-9A-Za-z!%&()+,./;?:-]` - Only ASCII letters!
- Additional: `\\.` (escaped characters) and `['][0-9A-Za-z]` (apostrophe + ASCII)

**Root Cause**:
The regex only includes ASCII letters (`A-Za-z`) and doesn't include Unicode letter categories. Characters with diacritics (like ô, é, ñ, ü) are not recognized as valid text.

**Impact**:
- Any text with accented characters fails to parse
- Affects many languages: Spanish, Portuguese, French, German, etc.
- Also affects other Unicode scripts (Cyrillic, Greek, Arabic, CJK, etc.)

### 7. Fix Strategy

**Approach**: Extend the `pandoc_str` regex to include Unicode letter categories

**Options**:

1. **Option A: Add Unicode letter property escapes** (if supported by tree-sitter)
   ```javascript
   pandoc_str: $ => /(?:[\u{00A0}0-9A-Za-z\p{L}%&()+-/]|\\.)(?:[\u{00A0}0-9A-Za-z\p{L}!%&()+,./;?:-]|\\.|['][0-9A-Za-z\p{L}])*/u,
   ```
   - `\p{L}` matches any Unicode letter
   - Requires `/u` flag for Unicode support

2. **Option B: Add explicit Unicode ranges** (more conservative)
   ```javascript
   pandoc_str: $ => /(?:[\u{00A0}0-9A-Za-z\u{00C0}-\u{00FF}\u{0100}-\u{017F}%&()+-/]|\\.)(?:[\u{00A0}0-9A-Za-z\u{00C0}-\u{00FF}\u{0100}-\u{017F}!%&()+,./;?:-]|\\.|['][0-9A-Za-z\u{00C0}-\u{00FF}\u{0100}-\u{017F}])*/,
   ```
   - `\u{00C0}-\u{00FF}`: Latin-1 Supplement (includes á, é, í, ó, ú, ñ, ç, etc.)
   - `\u{0100}-\u{017F}`: Latin Extended-A (includes ā, ē, ī, ō, ū, etc.)
   - Can be extended with more ranges as needed

3. **Option C: Full Unicode letter support** (most comprehensive)
   Add broader Unicode ranges to support all common scripts:
   - Latin Extended: `\u{0100}-\u{024F}`
   - Cyrillic: `\u{0400}-\u{04FF}`
   - Greek: `\u{0370}-\u{03FF}`
   - etc.

**Recommendation**: Start with Option A (Unicode property escapes) if supported by tree-sitter's regex engine. If not supported, fall back to Option B with essential Latin ranges, then extend as needed.

**Testing Plan**:
1. Write tree-sitter test cases in `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/`
2. Test with various Unicode characters:
   - Latin with diacritics: "café", "niño", "Müller"
   - Latin Extended: "Māori", "Łódź"
   - Cyrillic: "Москва"
   - Greek: "Αθήνα"
   - CJK: "東京" (if applicable)
3. Verify test fails with current grammar
4. Fix grammar.js
5. Run `tree-sitter generate && tree-sitter build`
6. Run `tree-sitter test` to verify
7. Test with original failing file

### 8. Implementation Steps

**Steps**:
- [ ] Research tree-sitter regex capabilities (Unicode property escapes support)
- [ ] Write tree-sitter test cases for Unicode text
- [ ] Verify tests fail with current grammar
- [ ] Implement grammar fix (update `pandoc_str` regex)
- [ ] Rebuild grammar: `cd crates/tree-sitter-qmd/tree-sitter-markdown && tree-sitter generate && tree-sitter build`
- [ ] Run tree-sitter tests: `tree-sitter test`
- [ ] Test with original file: `cargo run -p quarto-markdown-pandoc -- -i /Users/cscheid/today/diacritics.qmd`
- [ ] Run full test suite: `cargo test`
- [ ] Document supported Unicode ranges in code comments

## Quick Commands Reference

```bash
# Test current behavior
cargo run -p quarto-markdown-pandoc -- -i /Users/cscheid/today/diacritics.qmd

# Test with verbose output
cargo run -p quarto-markdown-pandoc -- -i /Users/cscheid/today/diacritics.qmd -v 2>&1 | tail -100

# Check character encoding
echo -n "Antônio" | od -c
echo -n "Antônio" | xxd

# Grammar location
# File: crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:559

# Rebuild grammar (from tree-sitter-markdown directory)
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter generate && tree-sitter build && tree-sitter test

# Run tests
cargo test -p quarto-markdown-pandoc
```

## Notes

- This is a fundamental limitation affecting all non-ASCII text
- The fix should be conservative to avoid breaking existing behavior
- Need to verify that Pandoc also supports these characters in regular text
- Consider performance implications of broader Unicode ranges
- May need similar fixes for other node types beyond `pandoc_str`
