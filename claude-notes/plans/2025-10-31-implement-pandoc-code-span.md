# Implement pandoc_code_span (Inline Code)

**Date**: 2025-10-31
**Epic**: k-274 (Tree-sitter Grammar Refactoring)
**Phase**: 2 (Basic Formatting)
**Priority**: HIGH

## Summary

Implement the `pandoc_code_span` node handler for inline code with backticks. This is item #8 in Phase 2 of the refactoring plan.

## Background

Inline code is represented with backticks: `` `code` ``

Tree-sitter structure:
```
pandoc_code_span: {Node pandoc_code_span (0, 0) - (0, 6)}
  code_span_delimiter: {Node code_span_delimiter (0, 0) - (0, 1)}    # opening backtick(s)
  content: {Node content (0, 1) - (0, 5)}                            # the code text
  code_span_delimiter: {Node code_span_delimiter (0, 5) - (0, 6)}    # closing backtick(s)
  attribute_specifier: {Node attribute_specifier (0, 6) - (0, 17)}   # OPTIONAL attributes
```

Pandoc output format:
```json
{
  "t": "Code",
  "c": [
    ["id", ["class1", "class2"], [["key", "value"]]],  // attributes
    "code text"                                         // content
  ]
}
```

## Key Observations

1. **No Space Injection**: Unlike emphasis constructs (emph, strong, etc.), code spans do NOT require Space node injection. The delimiters (backticks) don't capture surrounding spaces.

2. **Content Extraction**: The `content` child node contains the raw code text. We need to extract the text from this node.

3. **Attributes Optional**: Code spans can have optional attributes like `` `code`{.language} `` which produce attributes in the Pandoc AST.

4. **Multiple Backticks**: Code spans can use multiple backticks (e.g., ``` `` code `` ```) to allow literal backticks inside.

5. **Space Preservation**: Spaces within code spans are preserved exactly as-is.

## Test Cases Needed

### Basic Functionality
1. ✅ Single word: `` `code` `` → `Code [["", [], []], "code"]`
2. ✅ With spaces: `` `code with spaces` `` → `Code [["", [], []], "code with spaces"]`
3. ✅ No spaces around: `x\`y\`z` → `[Str "x", Code [..., "y"], Str "z"]`
4. ✅ Within text: `test \`code\` here` → `[Str "test", Space, Code [...], Space, Str "here"]`

### Attributes
5. ✅ With class: `` `code`{.language} `` → `Code [["", ["language"], []], "code"]`
6. ✅ With id: `` `code`{#myid} `` → `Code [["myid", [], []], "code"]`
7. ✅ With key-value: `` `code`{key=value} `` → `Code [["", [], [["key", "value"]]], "code"]`

### Edge Cases
8. ✅ Multiple backticks: ``` `` code with ` backtick `` ``` → proper content extraction
9. ✅ Empty code span: `` ` ` `` → `Code [["", [], []], " "]`
10. ✅ Multiple in paragraph: `` `foo` and `bar` ``

### Comparison with Pandoc
All outputs must exactly match Pandoc's behavior for these inputs.

## Implementation Steps

### Step 1: Write Tests (TDD Approach)
Add tests to `tests/test_treesitter_refactoring.rs`:
- `test_pandoc_code_span_basic()`
- `test_pandoc_code_span_with_spaces()`
- `test_pandoc_code_span_no_spaces_around()`
- `test_pandoc_code_span_within_text()`
- `test_pandoc_code_span_with_class()`
- `test_pandoc_code_span_multiple()`

### Step 2: Verify Tests Fail
Run: `cargo test --test test_treesitter_refactoring test_pandoc_code_span`

Expected: All tests fail because `pandoc_code_span` is not implemented

### Step 3: Implement Handler
In `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`, add handler in `native_visitor` function:

```rust
"pandoc_code_span" => {
    // Extract code content from the 'content' child node
    let mut code_text = String::new();
    let mut attr = Attr::default(); // (id, classes, key-value pairs)

    for (node_name, child) in &children {
        match node_name.as_str() {
            "content" => {
                // Extract text from content node
                if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                    code_text = std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                        .unwrap()
                        .to_string();
                }
            }
            "attribute_specifier" => {
                // Process attributes if present
                if let PandocNativeIntermediate::IntermediateAttr(attrs) = child {
                    attr = attrs.clone();
                }
            }
            "code_span_delimiter" => {
                // Ignore delimiters
            }
            _ => {}
        }
    }

    // Create Code inline
    PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
        attr,
        content: code_text,
        source_info: node_source_info_with_context(node, context),
    }))
}

"code_span_delimiter" => {
    // Marker node, no processing needed
    PandocNativeIntermediate::IntermediateUnknown(node_location(node))
}

"content" => {
    // This is a generic node name used in multiple contexts
    // For code_span, it will be handled by the parent
    // Return the text range so parent can extract it
    PandocNativeIntermediate::IntermediateUnknown(node_location(node))
}
```

### Step 4: Handle Code Inline Type
Check if `Inline::Code` already exists in the AST. If not, add it to `crates/quarto-markdown-pandoc/src/pandoc/inline.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub attr: Attr,
    pub content: String,
    pub source_info: quarto_source_map::SourceInfo,
}
```

And add to the `Inline` enum:
```rust
pub enum Inline {
    // ... existing variants
    Code(Code),
    // ... more variants
}
```

### Step 5: Run Tests
Run: `cargo test --test test_treesitter_refactoring test_pandoc_code_span`

Expected: All tests pass

### Step 6: Verbose Mode Verification
Test with verbose mode to ensure no MISSING warnings:
```bash
echo "\`code\`" | cargo run --bin quarto-markdown-pandoc -- --verbose 2>&1 | grep MISSING
```

Expected: No MISSING warnings for `pandoc_code_span`, `code_span_delimiter`, or `content` (in code_span context)

### Step 7: Compare with Pandoc
For each test case, verify output exactly matches Pandoc:
```bash
echo "test \`code\` here" | pandoc -f markdown -t json
echo "test \`code\` here" | cargo run --bin quarto-markdown-pandoc -- -t json
```

## Dependencies

- ✅ `pandoc_str` - already implemented
- ✅ `pandoc_space` - already implemented
- ✅ `pandoc_paragraph` - already implemented
- ⚠️ `attribute_specifier` - may need implementation for attributes support

If `attribute_specifier` is not yet fully implemented, we can:
1. Start with basic code spans (no attributes)
2. Add attribute support later

## Success Criteria

1. ✅ All tests pass
2. ✅ Output exactly matches Pandoc for all test cases
3. ✅ No MISSING warnings in verbose mode
4. ✅ Code properly handles optional attributes
5. ✅ Multiple backtick delimiters work correctly

## Estimated Time

**2-3 hours** including:
- Writing comprehensive tests (45 min)
- Implementation (60 min)
- Testing and verification (30 min)
- Handling edge cases (15 min)

## References

- Tree-sitter grammar: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`
- Existing processor: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/code_span.rs` (old implementation, may need updating)
- Pandoc documentation: [Code spans](https://pandoc.org/MANUAL.html#verbatim)

## Notes

- Code spans are simpler than emphasis constructs because they don't need Space injection
- The main complexity is in attribute handling
- Need to check if the existing `Code` inline type supports all needed fields
