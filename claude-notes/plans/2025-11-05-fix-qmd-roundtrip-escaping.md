# Plan: Fix QMD Roundtrip Escaping Bug

**Date**: 2025-11-05
**Issue**: Escaped punctuation characters lose their backslash escapes during qmd roundtripping

## Problem Summary

When parsing qmd text like `\$3.14`, the parser correctly processes the escape sequence and produces an AST with a `Str` node containing `$3.14`. However, when writing this AST back to qmd format, the writer doesn't re-escape the dollar sign, outputting `$3.14` instead of `\$3.14`.

This breaks roundtripping: `input.qmd → AST → output.qmd` produces different qmd text.

## Root Cause Analysis

### Parser Behavior (Correct)

**File**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/backslash_escape.rs`

The `process_backslash_escape` function correctly removes the backslash:
- Input text: `\$`
- Processing: Removes leading `\`
- AST result: `Str` node with text `$`

### Writer Behavior (Buggy)

**File**: `crates/quarto-markdown-pandoc/src/writers/qmd.rs:763-781`

The `escape_markdown` function only escapes 4 characters:
```rust
fn escape_markdown(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '>' => result.push_str("\\>"),
            '<' => result.push_str("\\<"),
            '#' => result.push_str("\\#"),
            _ => result.push(ch),
        }
    }
    result
}
```

### Escapable Characters According to Grammar

**File**: `crates/tree-sitter-qmd/common/common.js:16-18`

The tree-sitter grammar defines 30 escapable ASCII punctuation characters:
```javascript
const PUNCTUATION_CHARACTERS_ARRAY = [
    '!', '"', '#', '$', '%', '&', "'", '(', ')', '*', '+', ',', '-', '.', '/', ':', ';',
    '=', '?', '@', '[', '\\', ']', '^', '_', '`', '{', '|', '}', '~'
];
```

**Missing from writer**: `!`, `"`, `$`, `%`, `&`, `'`, `(`, `)`, `*`, `+`, `,`, `-`, `.`, `/`, `:`, `;`, `=`, `?`, `@`, `[`, `]`, `^`, `_`, `` ` ``, `{`, `|`, `}`, `~`

## Affected Characters - Test Results

Testing confirms multiple characters are affected:

| Input | Expected Output | Actual Output | Status |
|-------|----------------|---------------|---------|
| `\$3.14` | `\$3.14` | `$3.14` | ❌ BROKEN |
| `\*test\*` | `\*test\*` | `*test*` | ❌ BROKEN |
| `\_underscore\_` | `\_underscore\_` | `_underscore_` | ❌ BROKEN |
| `\[bracket\]` | `\[bracket\]` | `[bracket]` | ❌ BROKEN |
| `\`backtick\`` | `\`backtick\`` | `` `backtick` `` | ❌ BROKEN |

## Pandoc's Approach

Pandoc uses a defensive escaping strategy - it escapes punctuation characters liberally to ensure safe roundtripping:

```bash
$ echo 'test $ test' | pandoc -f markdown -t markdown
test \$ test

$ echo 'test * test' | pandoc -f markdown -t markdown
test \* test

$ echo 'test # test' | pandoc -f markdown -t markdown
test \# test
```

## Solution Strategy

### Option 1: Escape All Punctuation (Recommended)

**Pros**:
- Guarantees correct roundtripping
- Matches Pandoc's defensive approach
- Simple to implement
- No context needed

**Cons**:
- May escape more than strictly necessary
- Output might look "over-escaped" to humans

### Option 2: Context-Aware Escaping

**Pros**:
- Minimal escaping
- "Cleaner" looking output

**Cons**:
- Much more complex implementation
- Need to track context (position in line, surrounding chars, etc.)
- Error-prone
- May still have edge cases

**Recommendation**: Go with Option 1 for reliability and simplicity.

## Implementation Plan

### 1. Update `escape_markdown` function

**File**: `crates/quarto-markdown-pandoc/src/writers/qmd.rs`

Replace the `escape_markdown` function to escape all punctuation characters defined in the grammar:

```rust
fn escape_markdown(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            // All ASCII punctuation characters that can be escaped per the grammar
            '!' | '"' | '#' | '$' | '%' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' |
            '-' | '.' | '/' | ':' | ';' | '=' | '?' | '@' | '[' | '\\' | ']' | '^' |
            '_' | '`' | '{' | '|' | '}' | '~' => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    result
}
```

### 2. Add Comprehensive Tests

**File**: `crates/quarto-markdown-pandoc/tests/test_json_roundtrip.rs` or create new test file

Following TDD principles:
1. **FIRST**: Write tests that verify all punctuation characters roundtrip correctly
2. **SECOND**: Run tests to confirm they fail with current implementation
3. **THIRD**: Implement the fix in `escape_markdown`
4. **FOURTH**: Run tests to confirm they now pass

Test cases should include:
- Individual escaped punctuation: `\$`, `\*`, `\_`, etc.
- Multiple escapes in one line: `\$3.14 and \*not\* \$5`
- Escaped characters in various contexts (paragraphs, headers, lists, etc.)
- Edge case: escaped backslash itself `\\`
- Verify existing tests still pass

### 3. Consider Special Cases

Some characters may need special handling:

**a) Escaped backticks in code context**
- Already handled by `write_code` function (line 821)
- Should not be affected by our changes

**b) Characters in raw/verbatim contexts**
- Raw blocks/inlines should bypass escaping
- Verify this is already correct

**c) Math delimiters**
- Dollar signs in math are handled separately
- Should not be affected by changes to `escape_markdown`

### 4. Regression Testing

- Run full test suite: `cargo test`
- Check that no existing tests break
- Verify roundtrip tests in `tests/roundtrip_tests/qmd-json-qmd/` still pass

## Alternative: Smarter Escaping (Future Enhancement)

If "over-escaping" becomes a concern, we could later implement context-aware escaping:

**Characters that ALWAYS need escaping**:
- `\` (backslash)
- `<` (raw HTML, autolink)
- `>` (blockquote at line start)

**Characters that need conditional escaping**:
- `#` at line start (heading)
- `*`, `_` when paired (emphasis)
- `[` when followed by link syntax
- `$` when paired (math)
- etc.

However, this adds significant complexity and should only be pursued if the simpler solution proves problematic.

## Testing Checklist

- [ ] Write failing test for `\$` character
- [ ] Write failing tests for other affected punctuation
- [ ] Confirm tests fail with current code
- [ ] Implement fix in `escape_markdown`
- [ ] Confirm new tests pass
- [ ] Run full test suite and ensure no regressions
- [ ] Test with real-world qmd documents
- [ ] Compare output with Pandoc for consistency

## Related Issues

This may also affect:
- Other writer functions that handle text (check for similar patterns)
- Error messages that quote source text (should show escaped form)
- Documentation examples (may need updates)

## Files to Modify

1. `crates/quarto-markdown-pandoc/src/writers/qmd.rs` (primary fix)
2. `crates/quarto-markdown-pandoc/tests/test_json_roundtrip.rs` (or new test file)
3. Potentially add tests to `tests/roundtrip_tests/qmd-json-qmd/`

## Success Criteria

- All 30 escapable punctuation characters roundtrip correctly
- Existing tests continue to pass
- New tests demonstrate the fix works
- Behavior is consistent with Pandoc where possible
