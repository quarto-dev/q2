# Fix: Recursive Braces in Code Block Language Attributes

**Date:** 2025-10-28
**Status:** Proposed

## Problem

Code blocks with recursively-nested braces in language attributes fail to parse:

```markdown
```{{r}}
cat("hi")
```
```

Currently only `{r}` and bare `r` work, but `{{r}}`, `{{{r}}}`, etc. fail with parse errors.

## Root Cause

The `language_attribute` rule in `crates/tree-sitter-qmd/common/common.js` (lines 137-141) only matches exactly one level of braces:

```javascript
language_attribute: $ => prec.dynamic(1, seq(
  "{",
  alias($.commonmark_name, $.language),
  "}"
)),
```

## Solution

Make `language_attribute` recursive, similar to how `_link_destination_parenthesis` handles nested parentheses (lines 65-72).

### Proposed Change

Replace the `language_attribute` rule with:

```javascript
language_attribute: $ => prec.dynamic(1, seq(
  "{",
  choice(
    alias($.commonmark_name, $.language),
    $.language_attribute
  ),
  "}"
)),
```

This allows:
- `{r}` - base case with `commonmark_name`
- `{{r}}` - recursive case: `{ language_attribute }` where inner = `{r}`
- `{{{r}}}` - recursive case nested twice
- And so on...

## Implementation Steps

1. **Modify grammar rule**
   - File: `crates/tree-sitter-qmd/common/common.js`
   - Lines: 137-141
   - Change: Add recursive `choice()` as shown above

2. **Rebuild block parser**
   ```bash
   cd crates/tree-sitter-qmd/tree-sitter-markdown
   tree-sitter generate
   tree-sitter build
   tree-sitter test
   ```

3. **Rebuild inline parser**
   ```bash
   cd crates/tree-sitter-qmd/tree-sitter-markdown-inline
   tree-sitter generate
   tree-sitter build
   tree-sitter test
   ```

4. **Verify with test file**
   ```bash
   cargo run --bin quarto-markdown-pandoc -- -i test-recursive-braces.qmd
   ```
   Should parse without errors.

5. **Clean up**
   - Remove `test-recursive-braces.qmd` if no longer needed
   - Consider adding tree-sitter test case if appropriate

## Testing

Test file created: `test-recursive-braces.qmd`

Contains test cases for:
- Bare identifier: ` ```r `
- Single braces: ` ```{r} `
- Double braces: ` ```{{r}} `
- Triple braces: ` ```{{{r}}} `
- Quadruple braces: ` ```{{{{r}}}} `

All should parse successfully after the fix.

## Notes

- This is a Quarto-specific feature, not supported by Pandoc
- The recursive pattern is already used elsewhere in the grammar (`_link_destination_parenthesis`)
- No changes needed to precedence levels
