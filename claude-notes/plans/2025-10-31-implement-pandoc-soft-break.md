# Implementation Plan: pandoc_soft_break Node Handler

**Date**: 2025-10-31
**Context**: Tree-sitter grammar refactoring work (k-274)
**Goal**: Implement handler for `pandoc_soft_break` to correctly process soft line breaks in paragraphs

## Current State Analysis

### Test Input
```markdown
hello
world
```
(Two words separated by a single newline - a soft break within a paragraph)

### Tree-sitter Output
```
(document [0, 0] - [1, 5]
  (section [0, 0] - [1, 5]
    (pandoc_paragraph [0, 0] - [1, 5]
      (pandoc_str [0, 0] - [0, 5])      // "hello"
      (pandoc_soft_break [0, 5] - [1, 0])  // <-- THE NODE WE NEED TO HANDLE
      (pandoc_str [1, 0] - [1, 5]))))   // "world"
```

**Key observations**:
- Node name: `pandoc_soft_break`
- Spans from end of first line to start of next line: `[0, 5] - [1, 0]`
- Appears between inline elements (pandoc_str nodes)

### Current quarto-markdown-pandoc Output
```
[ Para [Str "helloworld"] ]
```

**Problem**: Missing SoftBreak, words concatenated!

### Expected Output (from `pandoc -t native`)
```
[ Para [ Str "hello" , SoftBreak , Str "world" ] ]
```

### Verbose Mode Output
```
pandoc_soft_break: {Node pandoc_soft_break (0, 5) - (1, 0)}
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: pandoc_soft_break
```

Confirms the node is detected but not handled.

## Pandoc AST Definition

**Location**: `crates/quarto-markdown-pandoc/src/pandoc/inline.rs`

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoftBreak {
    pub source_info: quarto_source_map::SourceInfo,
}
```

Used in the `Inline` enum as:
```rust
pub enum Inline {
    // ...
    Space(Space),
    SoftBreak(SoftBreak),
    LineBreak(LineBreak),
    // ...
}
```

## Implementation Strategy

This is a **trivial** implementation, nearly identical to the `pandoc_space` handler we already implemented in k-275.

### Code to Add

**Location**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs` ~line 540 (after `pandoc_space`)

```rust
"pandoc_soft_break" => {
    PandocNativeIntermediate::IntermediateInline(Inline::SoftBreak(SoftBreak {
        source_info: node_source_info_with_context(node, context),
    }))
}
```

That's it! No complex logic needed because:
- SoftBreak has no text content (unlike Str)
- SoftBreak has no children (unlike emphasis)
- SoftBreak is just a marker with source location

## Test-Driven Development Steps

### 1. Write Test First
**Location**: `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs`

```rust
#[test]
fn test_soft_break() {
    let input = "hello\nworld";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should contain SoftBreak between the two words
    assert!(result.contains("SoftBreak"));
    assert!(result.contains("\"hello\""));
    assert!(result.contains("\"world\""));

    // Should NOT concatenate
    assert!(!result.contains("helloworld"));
}
```

### 2. Run Test (Expect Failure)
```bash
cargo test --test test_treesitter_refactoring test_soft_break
```

Expected: Test fails because SoftBreak is missing

### 3. Implement Handler
Add the handler code shown above to `treesitter.rs`

### 4. Run Test (Expect Success)
```bash
cargo test --test test_treesitter_refactoring test_soft_break
```

Expected: Test passes

### 5. Verify with Manual Testing
```bash
printf "hello\nworld" | cargo run --bin quarto-markdown-pandoc --
```

Expected output: `[ Para [ Str "hello" , SoftBreak , Str "world" ] ]`

### 6. Verify No Warnings
```bash
printf "hello\nworld" | cargo run --bin quarto-markdown-pandoc -- --verbose 2>&1 | grep MISSING
```

Expected: No output (no missing nodes)

### 7. Run Full Test Suite
```bash
cargo test --test test_treesitter_refactoring
```

Expected: All tests pass (including our 4 existing tests + new soft break test)

## Comparison with Similar Nodes

### Space Handler (Already Implemented)
```rust
"pandoc_space" => {
    PandocNativeIntermediate::IntermediateInline(Inline::Space(Space {
        source_info: node_source_info_with_context(node, context),
    }))
}
```

### SoftBreak Handler (To Implement)
```rust
"pandoc_soft_break" => {
    PandocNativeIntermediate::IntermediateInline(Inline::SoftBreak(SoftBreak {
        source_info: node_source_info_with_context(node, context),
    }))
}
```

**Difference**: Only the node name and type name change. Structure is identical.

## Additional Test Cases

Beyond the basic test, we should verify:

1. **Multiple soft breaks**:
   ```markdown
   line1
   line2
   line3
   ```
   Expected: `Str "line1", SoftBreak, Str "line2", SoftBreak, Str "line3"`

2. **Soft break with punctuation**:
   ```markdown
   Hello,
   world!
   ```
   Expected: `Str "Hello,", SoftBreak, Str "world!"`

3. **Empty lines create hard breaks** (not soft breaks):
   ```markdown
   para1

   para2
   ```
   Expected: Two separate Para blocks (not tested here, but good to verify it doesn't break)

## Files to Modify

1. **Test file** (create first): `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs`
   - Add `test_soft_break()` function

2. **Processor**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`
   - Add handler in `native_visitor` match statement (around line 540)

## Success Criteria

- ✅ Test written and initially fails
- ✅ Handler implemented (3 lines of code)
- ✅ Test passes
- ✅ Manual test produces correct output
- ✅ No MISSING node warnings in verbose mode
- ✅ All existing tests still pass
- ✅ Output matches `pandoc -t native` exactly

## Edge Cases & Considerations

### What is a Soft Break?
- A **soft break** is a single newline within a paragraph
- Depending on output format, may render as:
  - A space (HTML by default)
  - A line break (with `hardwraps` option)
  - Preserved in source formats

### Not the Same As:
- **Space** (`pandoc_space`): Actual space character in source
- **LineBreak** (`LineBreak`): Hard break created with two spaces + newline or backslash + newline
- **Paragraph break**: Double newline creates new Para block

### Tree-sitter Node Naming
The tree-sitter grammar uses `pandoc_soft_break` (with underscore as alias for an internal `_soft_line_break` node). This aligns with Pandoc's naming.

## Timeline Estimate

- Step 1 (Write test): 5 minutes
- Step 2 (Run test, verify failure): 2 minutes
- Step 3 (Implement handler): 2 minutes
- Steps 4-7 (Testing & verification): 5 minutes

**Total**: ~15 minutes

This is one of the simplest node types to implement!

## Related Nodes

After implementing this, we may also want to tackle:
- `_newline` - Hard newlines (paragraph breaks)
- `_soft_line_break` - (may be same as `pandoc_soft_break`)
- `LineBreak` - Explicit line breaks (two spaces + newline)

## Notes

- This is a **critical** node for proper paragraph formatting
- Without it, multi-line paragraphs become run-together text
- Very common in real-world markdown (most paragraphs have soft breaks)
- Simple implementation but high impact
