# Investigation: Backtick Code Execution in Image/Link URLs

Date: 2025-11-20
Issue: k-369
Investigator: Claude Code

## Summary

Users are attempting to use inline code execution (backtick syntax) inside image and link URLs. This is not valid Quarto markdown syntax and produces a generic parse error.

## Corpus Search Results

Searched 1,939 .qmd files in external-sites corpus for patterns:
- `![](\`{python}` - image with inline code execution
- `[](\`{python}` - link with inline code execution
- `![](abc\`{python}` - image with text + inline code
- `[](abc\`{python}` - link with text + inline code

**Found**: 1 instance
- File: `external-sites/lino-galiana/python-datascientist/content/manipulation/04_api/_exo3_solution.qmd:61`
- Pattern: `![](\`{python} url_image\`)`

### Context from Found Instance

```python
# Line 58: Compute URL in code block
url_image = get_products_api(5449000000996, col = ["image_front_small_url"])["image_front_small_url"].iloc[0]

# Line 61: Attempt to use it with inline code execution
![](`{python} url_image`)
```

## Test Documents Created

Created 4 minimal test cases in `/tmp/kyoto-k369-tests/`:

1. **image-backtick-code.qmd**: `![](\`{python} url_variable\`)`
2. **link-backtick-code.qmd**: `[Click here](\`{python} url_variable\`)`
3. **image-mixed-backtick-code.qmd**: `![](https://example.com/\`{python} url_variable\`)`
4. **link-mixed-backtick-code.qmd**: `[Click here](https://example.com/\`{python} url_variable\`)`

## Current Parser Behavior

All 4 test cases produce identical error:

```json
{
  "kind": "error",
  "location": {"Original": {"start_offset": 48, "end_offset": 49, "file_id": 0}},
  "problem": {"content": "unexpected character or token here", "type": "markdown"},
  "title": "Parse error"
}
```

Error occurs at the `{` character immediately after the opening backtick.

**Tree-sitter behavior**:
- Parser lexes opening backtick: `pandoc_code_span_token2`
- Encounters `{` and enters error recovery
- Error is generic: "unexpected character or token here"

## Proposed Solution

Create a specific error diagnostic (Q-2-XX) that:

1. **Detects**: Backtick followed by `{language_name}` inside image/link destination
2. **Explains**: Inline code execution is not valid inside URLs
3. **Suggests**: "To dynamically compute URLs, produce an entire inline node (Image or Link) as the result of code execution"

### Detection Strategy

Need to identify the pattern:
- Context: Inside link/image destination (after `](` or `![](`)
- Pattern: Backtick followed by `{` with optional language name

### Error Message Draft

```
Error: Inline code execution not allowed in URLs

Inline code execution syntax (`{python} ...`) cannot be used inside
image or link destinations.

To dynamically compute URLs, your code block should produce an entire
inline node (Image or Link) as its result, not just the URL string.

Example:
  Instead of:  ![](`{python} url_variable`)
  Use:         `{python} Image("", url_variable, "")`

For OJS, you can use interpolation:
  ![]({url_variable})
```

## Next Steps

1. Determine appropriate Q-2-XX error code number
2. Implement detection in parser (likely in inline parsing)
3. Add error to quarto-error-reporting crate
4. Write tests with all 4 patterns
5. Verify on corpus file

## Files for Testing

- Real-world: `external-sites/lino-galiana/python-datascientist/content/manipulation/04_api/_exo3_solution.qmd`
- Test docs: `/tmp/kyoto-k369-tests/*.qmd`
