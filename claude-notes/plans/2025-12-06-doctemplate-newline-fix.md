# quarto-doctemplate: Fix Excess Newlines in Template Evaluation

## Problem Summary

quarto-doctemplate produces extra newlines compared to Pandoc's doctemplates when evaluating templates with multiline `$if$`, `$for$`, `$else$`, `$endif$`, and `$endfor$` directives.

### Evidence

Running equivalence tests (`crates/quarto-doctemplate/tests/pandoc_equiv_tests.rs`):

| Template | Pandoc Output | quarto-doctemplate Output |
|----------|---------------|---------------------------|
| `before\n$if(show)$\ncontent\n$endif$\nafter\n` | `before\ncontent\nafter\n` | `before\n\ncontent\n\nafter\n` |
| `$if(items)$\nList:\n$for(items)$\n  - $it$\n$endfor$\n$endif$\n` | `List:\n  - one\n  - two\n  - three\n` | `\nList:\n\n  - one\n\n  - two\n\n  - three\n\n\n` |

### Root Cause

In Pandoc's doctemplates parser (`external-sources/doctemplates/src/Text/DocTemplates/Parser.hs`):

1. **Multiline Mode Detection**: When `$if(...)$` or `$for(...)$` is followed by a newline, the parser enters "multiline mode" and calls `skipEndline` to consume the newline.

2. **Balanced Newline Swallowing**: In multiline mode, newlines are also consumed after:
   - `$endif$`
   - `$endfor$`
   - `$else$`
   - `$sep$`

3. **Key Function** (`Parser.hs:199-205`):
```haskell
skipEndline = do
  pEndline
  pos <- P.lookAhead $ do
           P.skipMany (P.char ' ' <|> P.char '\t')
           P.getPosition
  P.updateState $ \st -> st{ firstNonspace = pos }
```

In quarto-doctemplate:
- The tree-sitter grammar treats all text (including newlines) as literals
- There's no concept of "multiline mode"
- Newlines after directives are preserved verbatim

## Proposed Fix

### Approach: Post-Parse AST Transformation

Rather than modifying the tree-sitter grammar (which has poor support for context-sensitive parsing), transform the AST after parsing to strip "swallowable" newlines.

### Algorithm

For each control directive (`Conditional`, `ForLoop`), detect and strip:

1. **Opening directive trailing newline**: If the first node in the body is a `Literal` starting with `\n`, and the directive appears on its own line, strip the leading newline.

2. **Closing directive trailing newline**: If there's a `Literal` node following the directive that starts with `\n`, and the directive appears on its own line, strip the leading newline.

3. **Else/Sep trailing newline**: Same logic for `$else$` and `$sep$` within their parent structures.

### Detection Criteria: "On Its Own Line"

A directive is "on its own line" when:
- Preceded only by whitespace on the current line (or at start of file)
- Followed immediately by newline or EOF

This matches Pandoc's behavior where inline directives like `before $if(show)$content$endif$ after` don't trigger newline swallowing.

### Implementation Location

Option A: **Parser Phase** (`parser.rs`)
- After tree-sitter parsing, before returning the `Template`
- Pro: Clean separation; AST represents "normalized" template
- Con: Need to track source positions carefully

Option B: **Evaluator Phase** (`evaluator.rs`)
- During `evaluate_node` for `Conditional`/`ForLoop`
- Pro: Doesn't modify AST representation
- Con: Mixes normalization with evaluation logic

**Recommendation**: Option A (Parser Phase) is cleaner and matches Pandoc's approach where swallowing happens at parse time.

### Implementation Steps

1. **Add helper function**: `normalize_multiline_directives(nodes: &mut Vec<TemplateNode>)`

2. **Detect multiline context**: For each `Conditional`/`ForLoop`:
   - Check if previous sibling `Literal` ends with only whitespace on last line
   - Check if next sibling `Literal` starts with newline
   - If both true, mark as "multiline"

3. **Strip newlines**:
   - If multiline, strip leading newline from first body literal
   - Strip trailing newline from last body literal before `$endif$`/`$endfor$`
   - Handle `else` branch similarly

4. **Recursive application**: Apply to nested conditionals/loops

### Edge Cases

- **Inline directives**: `$if(x)$yes$endif$` - no newlines to strip
- **Mixed content**: `text $if(x)$\ncontent\n$endif$ more` - don't strip newlines
- **Empty branches**: `$if(x)$$endif$` - nothing to strip
- **Nested directives**: Each level handles its own newlines

## Testing Strategy

1. **Keep existing tests passing**: Current tests document inline behavior which should remain unchanged

2. **Add Pandoc equivalence tests**: Compare output against `pandoc --template` for:
   - Multiline conditionals
   - Multiline loops
   - Nested structures
   - Mixed inline/multiline

3. **Boundary tests**: Edge cases from above

## Files to Modify

- `crates/quarto-doctemplate/src/parser.rs` - Add normalization pass
- `crates/quarto-doctemplate/tests/pandoc_equiv_tests.rs` - Convert to proper assertions

## Related Work

- Pandoc implementation: `external-sources/doctemplates/src/Text/DocTemplates/Parser.hs`
- Tree-sitter grammar: `crates/tree-sitter-doctemplate/`

## Scope of This Fix

This fix addresses two issues:

1. **Multiline directive newline swallowing** - The main problem described above
2. **Variable final newline stripping** - Pandoc's `removeFinalNl` strips trailing newlines from resolved values

### Feature Independence Analysis

Before implementing, we verified that these fixes won't complicate future implementation of `$^$` (nesting) or `$~$` (breaking spaces):

| Feature | Operates On | When | Interaction |
|---------|-------------|------|-------------|
| Final newline stripping | Resolved values (SimpleVal) | Variable resolution | None with others |
| Nesting (`$^$`) | Template content | Rendering with column state | None with stripping |
| Breaking spaces (`$~$`) | Literal text | Parsing | Stripping ignores BreakingSpace |

**Key insight**: In Pandoc's `removeFinalNl`, `BreakingSpace` nodes pass through unchanged:

```haskell
removeFinalNl DL.NewLine        = mempty
removeFinalNl DL.CarriageReturn = mempty
removeFinalNl (DL.Concat d1 d2) = d1 <> removeFinalNl d2
removeFinalNl x                 = x  -- BreakingSpace passes through!
```

This is intentional: in breaking space mode, "newlines" become `BreakingSpace` for text reflow, and shouldn't be stripped.

**Conclusion**: These features are orthogonal:
- Variable final newline stripping operates on VALUES at resolution time
- Nesting wraps TEMPLATE CONTENT at render time with column tracking
- Breaking spaces convert LITERALS at parse time

Implementing the current fixes will not complicate future `$^$` or `$~$` work.

## Other Pandoc Features (Future Work)

1. **Automatic Nesting** (`handleNesting` in Parser.hs):
   - Variables alone on a line get auto-wrapped in `Nested`
   - Multi-line values are indented to match variable column
   - quarto-doctemplate has `$^$` directive but it's marked TODO

2. **Breaking Spaces** (`$~$` toggle):
   - Partially parsed but not fully implemented in evaluator

These are left for future work and are orthogonal to the current fixes.
