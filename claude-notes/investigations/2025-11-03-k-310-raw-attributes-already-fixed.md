# k-310: Raw Attribute Validation - Already Fixed

## Date: 2025-11-03

## Issue Description
k-310 claimed that `'# Hello {=world}'` should fail to parse (raw attributes not allowed in QMD) but currently passes.

## Investigation Results
**The issue is already resolved!** Raw attributes ARE being properly rejected.

### Test Results

#### ✅ Headers with raw attributes - REJECTED (correct)
```bash
echo '# Hello {=world}' | quarto-markdown-pandoc
# Error: unexpected character or token here (at =)
```

#### ✅ Spans with raw attributes - REJECTED (correct)
```bash
echo '[text]{=html}' | quarto-markdown-pandoc
# Error: unexpected character or token here (at =)
```

#### ✅ Code blocks with raw attributes - ALLOWED (correct)
```bash
echo '```{=html}
<div>test</div>
```' | quarto-markdown-pandoc
# Output: [ RawBlock (Format "html") "<div>test</div>" ]
```

This is correct because `{=format}` is the standard Pandoc syntax for raw blocks.

#### ✅ Regular attributes - ALLOWED (correct)
```bash
echo '# Hello {.class}' | quarto-markdown-pandoc
# Output: [ Header 1 ( "hello" , ["class"] , [] ) [Str "Hello"] ]
```

### Current Behavior Summary

| Context | Raw Attribute `{=format}` | Status | Correct? |
|---------|---------------------------|--------|----------|
| Headers | REJECTED | Error | ✅ |
| Spans | REJECTED | Error | ✅ |
| Code Blocks | ALLOWED | RawBlock | ✅ |
| Regular attrs `{.class}` | ALLOWED | Works | ✅ |

### Test Status
The test `test_disallowed_in_qmd_fails` **PASSES**:
```bash
cargo test test_disallowed_in_qmd_fails
# test result: ok. 1 passed; 0 failed
```

This test verifies that files in:
- `tests/pandoc-differences/disallowed-in-qmd/*.qmd`
- `tests/invalid-syntax/*.qmd`

...correctly fail to parse.

## Conclusion
k-310 can be **CLOSED** as the validation is already working correctly. Raw attributes are:
1. Properly rejected on headers and spans (parse error)
2. Properly allowed on code blocks (for RawBlock support)
3. Regular attributes continue to work

The issue was likely created based on an old state of the code, or the tree-sitter refactoring fixed it as a side effect.

## Recommendation
Close k-310 with note: "Already fixed - raw attribute validation working correctly."
