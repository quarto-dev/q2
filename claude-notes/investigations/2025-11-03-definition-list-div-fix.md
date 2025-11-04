# Definition List Div Handling Investigation

**Date:** 2025-11-03
**Issue:** k-319
**Status:** Resolved

## Summary

After the tree-sitter grammar overhaul, definition list tests were failing. Investigation revealed that the definition list transformation code was working correctly, but test files needed to be updated to conform to a new grammar requirement.

## Root Cause

The new tree-sitter grammar requires a **blank line before the closing `:::`** of a fenced div when the div contains a bullet list. This is due to how the external scanner handles `$._close_block` tokens when lists are involved.

### Grammar Analysis

In `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` line 684-692:

```javascript
pandoc_div: $ => seq(
  $._fenced_div_start,
  $._whitespace,
  choice(alias($._commonmark_naked_value, $.info_string),
         alias($._pandoc_attr_specifier, $.attribute_specifier)),
  $._newline,
  repeat($._block),
  optional(seq($._fenced_div_end, $._close_block, choice($._newline, $._eof))),
  $._block_close,
),
```

The closing fence sequence (`$._fenced_div_end, $._close_block, ...`) requires the `$._close_block` token to be emitted. When a list is the last block before the closing fence, the external scanner cannot emit `$._close_block` without a blank line to properly close the list context.

## Definition List Processing Code

The transformation logic in `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs` is working correctly:

1. **Lines 180-223**: `is_valid_definition_list_div()` - Validates that:
   - Div has "definition-list" class
   - Contains exactly one BulletList
   - Each list item has exactly 2 blocks (term and definitions)
   - First block is Plain or Paragraph (the term)
   - Second block is BulletList (the definitions)

2. **Lines 229-260**: `transform_definition_list_div()` - Transforms validated divs:
   - Extracts term inlines from first block
   - Extracts definition blocks from nested bullet list
   - Creates DefinitionList with (term, definitions) tuples
   - Preserves source location from original div

3. **Lines 393-400**: Filter application:
   ```rust
   .with_div(|div| {
       if is_valid_definition_list_div(&div) {
           FilterResult(vec![transform_definition_list_div(div)], false)
       } else {
           Unchanged(div)
       }
   })
   ```

## Testing

### Without blank line (fails):
```markdown
::: {.definition-list}
* term 1
  - definition 1
:::
```
Error: `Parse error: unexpected character or token here` at closing `:::`

### With blank line (works):
```markdown
::: {.definition-list}
* term 1
  - definition 1

:::
```
Output: `[ DefinitionList [([Str "term", Space, Str "1"], [[Plain [Str "definition", Space, Str "1"]]])] ]`

## Solution

Update all definition-list test files to add a blank line before the closing `:::`:

- `tests/snapshots/native/definition-list-basic.qmd`
- `tests/snapshots/native/definition-list-complex-term.qmd`
- `tests/snapshots/native/definition-list-invalid-extra-blocks.qmd`
- `tests/snapshots/native/definition-list-invalid-no-nested-list.qmd`
- `tests/snapshots/native/definition-list-multiple-defs.qmd`

Note: `definition-list-invalid-empty-div.qmd` already has correct structure.

## Expected Output

After fixes, all definition list tests should pass and produce correct DefinitionList Pandoc AST nodes with proper term/definition structure.
