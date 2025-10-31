# Implementation Plan: pandoc_emph

**Date**: 2025-10-31
**Status**: Planning
**Parent Issue**: k-274 (Tree-sitter Grammar Refactoring)
**Context**: Implementing `pandoc_emph` node handler in the refactored tree-sitter grammar

## Problem Statement

The `pandoc_emph` node is not yet implemented in the refactored tree-sitter grammar processor. When the parser encounters emphasis text like `*hello*` or `_hello_`, it produces a `pandoc_emph` node in the syntax tree, but the processor doesn't know how to convert it to a Pandoc `Emph` inline element.

## Current Behavior

```bash
$ echo "*hello*" | cargo run --bin quarto-markdown-pandoc --
[ Para [] ]
```

**Tree structure:**
```
pandoc_emph: {Node pandoc_emph (0, 0) - (0, 7)}
  emphasis_delimiter: {Node emphasis_delimiter (0, 0) - (0, 1)}
  pandoc_str: {Node pandoc_str (0, 1) - (0, 6)}
  emphasis_delimiter: {Node emphasis_delimiter (0, 6) - (0, 7)}
```

**Warnings:**
```
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: emphasis_delimiter
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: pandoc_emph
```

## Expected Behavior

```bash
$ echo "*hello*" | pandoc -f markdown -t json
{"blocks":[{"t":"Para","c":[{"t":"Emph","c":[{"t":"Str","c":"hello"}]}]}]}
```

**Native format:**
```
[ Para [ Emph [ Str "hello" ] ] ]
```

## Technical Analysis

### Node Structure
- **Node name**: `pandoc_emph`
- **Children**:
  1. `emphasis_delimiter` (opening) - should be filtered out
  2. Content nodes (one or more inline elements)
  3. `emphasis_delimiter` (closing) - should be filtered out

### Supported Syntaxes
1. Asterisks: `*text*`
2. Underscores: `_text_`

### Content Complexity
The content can include:
- Single words: `*hello*` → one `pandoc_str`
- Multiple words: `*hello world*` → multiple `pandoc_str` nodes with `pandoc_space` between
- Nested formatting: `*hello **bold** world*` (will handle later when implementing `pandoc_strong`)

### Existing Infrastructure
The helper function `process_emphasis_inline` already exists in `treesitter_utils/text_helpers.rs` (lines 54-67):
- Filters out delimiter children
- Processes remaining children using a `native_inline` closure
- Builds the final Inline using a `build_inline` closure

The `emphasis_inline!` macro in `treesitter.rs` (lines 342-357) provides a convenient wrapper.

## Implementation Plan

### Step 1: Handle `emphasis_delimiter` marker node
**Location**: `treesitter.rs`, in the `native_visitor` match statement

Add case to return `IntermediateUnknown` for delimiter nodes (they're just markers):
```rust
"emphasis_delimiter" => {
    PandocNativeIntermediate::IntermediateUnknown(node_location(node))
}
```

**Test**: Verify the delimiter warnings disappear (but `pandoc_emph` warning remains)

### Step 2: Implement `pandoc_emph` handler
**Location**: `treesitter.rs`, in the `native_visitor` match statement

Use the existing `emphasis_inline!` macro:
```rust
"pandoc_emph" => emphasis_inline!(
    node,
    children,
    "emphasis_delimiter",
    native_inline,
    Emph,
    context
),
```

**Test**: Basic emphasis should now work

### Step 3: Write comprehensive tests
**Location**: `tests/test_treesitter_refactoring.rs`

#### Test 3a: Basic single-word emphasis (asterisk)
```rust
#[test]
fn test_pandoc_emph_basic_asterisk() {
    let input = "*hello*";
    let result = parse_qmd_to_pandoc_ast(input);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Str \"hello\""), "Should contain Str \"hello\": {}", result);
}
```

**Expected**: `[ Para [ Emph [ Str "hello" ] ] ]`

#### Test 3b: Basic single-word emphasis (underscore)
```rust
#[test]
fn test_pandoc_emph_basic_underscore() {
    let input = "_hello_";
    let result = parse_qmd_to_pandoc_ast(input);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Str \"hello\""), "Should contain Str \"hello\": {}", result);
}
```

**Expected**: `[ Para [ Emph [ Str "hello" ] ] ]`

#### Test 3c: Multi-word emphasis
```rust
#[test]
fn test_pandoc_emph_multiple_words() {
    let input = "*hello world*";
    let result = parse_qmd_to_pandoc_ast(input);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Str \"hello\""), "Should contain Str \"hello\": {}", result);
    assert!(result.contains("Str \"world\""), "Should contain Str \"world\": {}", result);
}
```

**Expected**: `[ Para [ Emph [ Str "hello" , Space , Str "world" ] ] ]`

#### Test 3d: Emphasis within text
```rust
#[test]
fn test_pandoc_emph_within_text() {
    let input = "before *hello* after";
    let result = parse_qmd_to_pandoc_ast(input);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Str \"before\""), "Should contain before: {}", result);
    assert!(result.contains("Str \"hello\""), "Should contain hello: {}", result);
    assert!(result.contains("Str \"after\""), "Should contain after: {}", result);
}
```

**Expected**: `[ Para [ Str "before" , Space , Emph [ Str "hello" ] , Space , Str "after" ] ]`

#### Test 3e: Multiple emphasis in one paragraph
```rust
#[test]
fn test_pandoc_emph_multiple() {
    let input = "*hello* and *world*";
    let result = parse_qmd_to_pandoc_ast(input);
    // Count occurrences of "Emph" - should appear twice
    let emph_count = result.matches("Emph").count();
    assert_eq!(emph_count, 2, "Should contain 2 Emph nodes: {}", result);
    assert!(result.contains("Str \"hello\""), "Should contain hello: {}", result);
    assert!(result.contains("Str \"world\""), "Should contain world: {}", result);
}
```

**Expected**: `[ Para [ Emph [ Str "hello" ] , Space , Str "and" , Space , Emph [ Str "world" ] ] ]`

#### Test 3f: Empty emphasis
```rust
#[test]
fn test_pandoc_emph_empty() {
    let input = "**";
    let result = parse_qmd_to_pandoc_ast(input);
    // This might not parse as emphasis at all - verify behavior
    // Document actual behavior in test
}
```

**Note**: Need to verify how the grammar handles this edge case

#### Test 3g: Emphasis with newline (soft break)
```rust
#[test]
fn test_pandoc_emph_with_softbreak() {
    let input = "*hello\nworld*";
    let result = parse_qmd_to_pandoc_ast(input);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("SoftBreak"), "Should contain SoftBreak: {}", result);
}
```

**Expected**: `[ Para [ Emph [ Str "hello" , SoftBreak , Str "world" ] ] ]`

### Step 4: Run tests and verify
```bash
cargo test --test test_treesitter_refactoring
```

All tests should pass.

### Step 5: Verify with verbose output
```bash
echo "*hello*" | cargo run --bin quarto-markdown-pandoc -- --verbose
```

Should show no warnings about unhandled `pandoc_emph` or `emphasis_delimiter` nodes.

### Step 6: Compare with Pandoc
```bash
echo "*hello*" | cargo run --bin quarto-markdown-pandoc -- -t json
echo "*hello*" | pandoc -f markdown -t json
```

Outputs should match (modulo formatting differences).

## Success Criteria

- [ ] No warnings about unhandled `pandoc_emph` nodes
- [ ] No warnings about unhandled `emphasis_delimiter` nodes
- [ ] All new tests pass
- [ ] Output matches Pandoc for basic emphasis cases
- [ ] Both `*` and `_` syntax work correctly
- [ ] Emphasis works within larger text contexts
- [ ] Multiple emphasis in one paragraph work correctly

## Edge Cases to Consider (Future Work)

1. **Nested emphasis and strong**: `*hello **bold** world*`
   - Will need `pandoc_strong` implemented first

2. **Mixed delimiters**: `*hello_` (should NOT produce emphasis)
   - Grammar should already handle this correctly

3. **Intra-word emphasis**: `hel*lo*`
   - Need to verify grammar behavior

4. **Emphasis at paragraph boundaries**: Multiple lines with emphasis

5. **Emphasis with other inline elements**: links, code spans, etc.
   - Will test as we implement more node types

## Files to Modify

1. **`crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`**
   - Add `emphasis_delimiter` case (around line 610-630 where other delimiters are)
   - Add `pandoc_emph` case (around line 573-579 where old emphasis code was commented)

2. **`crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs`**
   - Add 7 new test functions

## Dependencies

**Required** (already implemented):
- ✅ `pandoc_str` - for text content
- ✅ `pandoc_space` - for whitespace
- ✅ `pandoc_soft_break` - for newlines within emphasis
- ✅ `process_emphasis_inline` helper function
- ✅ `emphasis_inline!` macro

**Not required but will enable more tests later**:
- ❌ `pandoc_strong` - for nested formatting
- ❌ `pandoc_code_span` - for inline code in emphasis
- ❌ `inline_link` - for links in emphasis

## Notes

### Comparison with Old Implementation
The old (commented-out) implementation used:
```rust
"emphasis" => emphasis_inline!(
    node,
    children,
    "emphasis_delimiter",
    native_inline,
    Emph,
    context
),
```

The new implementation will be identical except:
- Old node name: `emphasis`
- New node name: `pandoc_emph`

### Grammar Notes
From verbose output analysis:
- The external scanner handles delimiter matching
- The grammar ensures delimiters are balanced
- Both `*` and `_` produce the same node type (`pandoc_emph`)
- Delimiter choice is not preserved in AST (correctly matches Pandoc behavior)

### Testing Strategy
Following the TDD workflow from project instructions:
1. ✅ Write the test
2. ✅ Run test, verify it fails
3. ✅ Implement the fix
4. ✅ Run test, verify it passes

## Timeline Estimate

- **Step 1** (delimiter handling): 5 minutes
- **Step 2** (pandoc_emph handler): 5 minutes
- **Step 3** (write tests): 20 minutes
- **Step 4** (run and debug): 10 minutes
- **Step 5-6** (verification): 5 minutes

**Total**: ~45 minutes

## Next Steps After Completion

According to the refactoring plan priority order:
1. ✅ `pandoc_str` - DONE
2. ✅ `pandoc_space` - DONE
3. ✅ `pandoc_soft_break` - DONE
4. 🔄 `pandoc_emph` - IN PROGRESS
5. ⏭️ `pandoc_strong` - NEXT (very similar to emph)
6. ⏭️ `pandoc_code_span` - after strong
