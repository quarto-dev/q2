# Plan: Convert Naked HTML Elements to Warnings with Auto-fix

**Date**: 2025-11-12
**Goal**: Change naked HTML element handling from hard error to warning with automatic conversion to RawInline nodes

## Background

Currently, `<b>hello world</b>` causes a hard error (Q-2-6) and prevents document processing. We want to:
1. Issue a warning instead of an error
2. Automatically convert HTML elements to `RawInline` nodes with format="html"
3. Allow the document to process successfully

## Design Decision: Option A (Convert During Tree-sitter Processing)

Following the pattern in `html_comment.rs`, we'll convert HTML elements to `RawInline` nodes during the tree-sitter traversal, similar to how HTML comments are already handled.

## Implementation Plan

### Step 1: Add New Warning Code to Error Catalog

**File**: `crates/quarto-error-reporting/error_catalog.json`

Add new entry:
```json
"Q-2-9": {
  "subsystem": "markdown",
  "title": "HTML Element Auto-converted",
  "message_template": "HTML elements are automatically converted to raw HTML inlines.",
  "docs_url": "https://quarto.org/docs/errors/Q-2-9",
  "since_version": "99.9.9"
}
```

**Note**: Keep Q-2-6 unchanged in case we want to revert to hard error behavior.

### Step 2: Modify HTML Element Handler in treesitter.rs

**File**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`
**Location**: Lines 1056-1079 (the `"html_element"` case)

**Current behavior**:
- Creates error message
- Returns `IntermediateUnknown`

**New behavior**:
- Extract exact text from node
- Create warning with code Q-2-9
- Create `RawInline` with format="html"
- Return `IntermediateInline(Inline::RawInline(...))`

**Key implementation details**:
1. Use `node.utf8_text(input_bytes).unwrap().to_string()` for exact text (no whitespace stripping)
2. Use `node_source_info_with_context(node, context)` for source location
3. Use `DiagnosticMessageBuilder::warning()` instead of `::error()`
4. Add warning message explaining auto-conversion
5. Keep the helpful hint about using backtick syntax explicitly

**Code structure** (similar to `html_comment.rs`):
```rust
"html_element" => {
    let text = node.utf8_text(input_bytes).unwrap().to_string();

    let msg = DiagnosticMessageBuilder::warning("HTML element converted to raw HTML")
        .with_code("Q-2-9")
        .with_location(node_source_info_with_context(node, context))
        .add_info("HTML elements are automatically converted to `RawInline` nodes with format 'html'")
        .add_hint("To be explicit, use: `<element>`{=html}")
        .build();
    error_collector.add(msg);

    PandocNativeIntermediate::IntermediateInline(Inline::RawInline(RawInline {
        format: "html".to_string(),
        text,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

### Step 3: Write Tests

**File**: `crates/quarto-markdown-pandoc/tests/test_warnings.rs`

Create comprehensive test suite:

1. **Test: HTML element produces warning, not error**
   - Input: `<b>hello world</b>`
   - Assert: Process succeeds (no exit code 1)
   - Assert: Warnings contain Q-2-9
   - Assert: No errors in collector

2. **Test: Output contains correct RawInline nodes**
   - Input: `<b>hello world</b>`
   - Assert: AST contains two RawInline nodes
   - Assert: First has text=`"<b>"` and format=`"html"`
   - Assert: Second has text=`"</b>"` and format=`"html"`

3. **Test: Multiple HTML elements**
   - Input: `<i>italic</i> and <b>bold</b>`
   - Assert: All four HTML elements converted
   - Assert: Four warnings issued

4. **Test: Source locations are accurate**
   - Input: `hello <b>world</b>`
   - Assert: Warning locations point to exact HTML element positions
   - Assert: RawInline source_info is accurate

5. **Test: Block-level HTML elements**
   - Input: `<div>content</div>`
   - Assert: Both elements converted to RawInline
   - Assert: Warnings issued

### Step 4: Verify Behavior

Run manual tests:
```bash
# Should succeed with warnings
echo '<b>hello world</b>' | cargo run -- -f qmd -t json

# Should produce valid JSON output
echo '<b>hello world</b>' | cargo run -- -f qmd -t json 2>/dev/null | jq .

# Compare with explicit syntax
echo '`<b>hello world</b>`{=html}' | cargo run -- -f qmd -t json
```

## Test-Driven Development Checklist

Following the CLAUDE.md instructions:

- [ ] Step 3.1: Write test for warning instead of error
- [ ] Step 3.1: Run test, verify it fails with current error behavior
- [ ] Step 1: Add Q-2-9 to error catalog
- [ ] Step 2: Implement HTML element conversion in treesitter.rs
- [ ] Step 3.1: Run test, verify it now passes
- [ ] Step 3.2: Write test for RawInline output structure
- [ ] Step 3.2: Run test, verify it passes
- [ ] Step 3.3-5: Write remaining tests
- [ ] Step 3.3-5: Run all tests, verify they pass
- [ ] Step 4: Run manual verification
- [ ] Run full test suite: `cargo test`

## Success Criteria

1. ✅ `<b>hello world</b>` produces warnings, not errors
2. ✅ Document processes successfully (no exit code 1)
3. ✅ Output contains RawInline nodes with format="html"
4. ✅ All existing tests still pass
5. ✅ New tests verify warning behavior and AST structure

## Future Considerations

- **Revert option**: Q-2-6 remains in catalog in case we want to switch back to hard error
- **Post-AST linting**: If we want more sophisticated linting (e.g., the definition list TODOs in postprocess.rs), we can later add a separate linting infrastructure
- **Block elements**: Current plan treats block-level HTML (like `<div>`) the same as inline elements, both becoming RawInline nodes
