# Block Handlers Implementation Plan

**Date**: 2025-10-31
**Beads Issue**: k-274 (tree-sitter grammar refactoring - block handlers phase)
**Context**: Inline handlers are complete. Need to implement remaining block-level handlers.

## Current Status

### Working Block Handlers ✅
- `document` - top-level document structure
- `section` - section hierarchy (headings create sections)
- `pandoc_paragraph` - paragraphs
- `atx_heading` - ATX-style headings (# Heading)
- `pandoc_block_quote` - block quotes (> quote)
- `pandoc_list` - lists (bullet and ordered)
- `list_item` - list items
- `pandoc_code_block` - fenced code blocks with backticks
- `pandoc_horizontal_rule` - horizontal rules (---)
- `inline_ref_def` - inline note definitions ([^note]: text)

### Working Inline Handlers ✅
(These were completed in earlier work)
- All basic inline formatting (emphasis, strong, code, etc.)
- Citations (@cite, [-@cite])
- Inline notes (^[note])
- Note references ([^ref])
- Quotes, superscript, subscript, strikeout
- Editorial marks (insert, delete, highlight, comment)
- Shortcodes ({{< shortcode >}})
- Links and images
- Math (inline and display)

### Missing Block Handlers ❌

**Critical - Currently Causing Crashes:**
1. **`pandoc_div`** - Fenced divs (`::: {.class}`)
   - Status: MISSING, causes crash with "Expected Block or Section, got IntermediateUnknown"
   - Helper: `process_fenced_div_block` EXISTS but not wired up
   - Line in treesitter.rs: 1099 (commented out)

2. **`note_definition_fenced_block`** - Fenced note definitions (`::: ^ref`)
   - Status: MISSING, produces warning "Unhandled node kind: note_definition_fenced_block"
   - Helper: `process_note_definition_fenced_block` EXISTS but not wired up
   - Line in treesitter.rs: 992-994 (commented out)

**Important - Tables:**
3. **`pipe_table`** and related - Pipe tables (`| Header |`)
   - Status: MISSING, likely produces warning or crash
   - Helpers: Complete set of pipe_table helpers EXISTS but not wired up
   - Lines in treesitter.rs: 1111-1118 (commented out)
   - Related nodes:
     - `pipe_table_header`
     - `pipe_table_row`
     - `pipe_table_cell`
     - `pipe_table_delimiter_row`
     - `pipe_table_delimiter_cell`
     - `caption`

### Explicitly Not Supported ⛔
(User confirmed these are not supported)
- `indented_code_block` - Code blocks indented by 4 spaces (NOT SUPPORTED)
- Tilde-fenced code blocks (only backticks supported)

## Implementation Priority

### Phase 1: Critical Block Handlers (HIGHEST PRIORITY)

**1. `pandoc_div` - Fenced Divs**
- **Why critical**: Currently causes crashes, blocking many documents
- **Complexity**: Medium - helper exists, needs to be wired up
- **Testing priority**: HIGH
- **Example input**:
  ```markdown
  ::: {.my-class #my-id}
  Div content here
  :::
  ```
- **Expected output**: `Div` block with attributes and content
- **Implementation steps**:
  1. Uncomment line 1099 in treesitter.rs
  2. Test with basic div (no attributes)
  3. Test with attributes
  4. Test with nested content (paragraphs, lists, etc.)
  5. Test with nested divs

**2. `note_definition_fenced_block` - Fenced Note Definitions**
- **Why critical**: User specifically mentioned this as a key feature
- **Complexity**: Low - helper exists and looks complete
- **Testing priority**: HIGH
- **Example input**:
  ```markdown
  ::: ^mynote
  This is a multi-block note definition.

  Second paragraph in the note.
  :::
  ```
- **Expected output**: `NoteDefinitionFencedBlock` with id and content blocks
- **Implementation steps**:
  1. Uncomment lines 992-994 in treesitter.rs
  2. Test with single-block note definition
  3. Test with multi-block note definition
  4. Test with complex content (lists, quotes, etc.)
  5. Verify integration with note references ([^mynote])

### Phase 2: Tables (MEDIUM PRIORITY)

**3. `pipe_table` - Pipe Tables**
- **Why important**: Common markdown feature
- **Complexity**: High - multiple sub-nodes, alignment handling, caption
- **Testing priority**: MEDIUM
- **Example input**:
  ```markdown
  | Header 1 | Header 2 |
  |----------|----------|
  | Cell 1   | Cell 2   |
  | Cell 3   | Cell 4   |

  : Table caption
  ```
- **Expected output**: `Table` block with headers, rows, alignment, caption
- **Implementation steps**:
  1. Uncomment line 1118 (`pipe_table`)
  2. Uncomment lines 1112-1113 (`pipe_table_header` and `pipe_table_row`)
  3. Uncomment line 1115 (`pipe_table_delimiter_row`)
  4. Uncomment line 1111 (`pipe_table_delimiter_cell`)
  5. Uncomment line 1116 (`pipe_table_cell`)
  6. Uncomment line 1117 (`caption`)
  7. Test basic 2x2 table
  8. Test with alignment markers
  9. Test with caption
  10. Test with inline formatting in cells
  11. Test edge cases (empty cells, single column, etc.)

## Testing Strategy

### For Each Block Handler:

**1. Create Failing Test First (TDD)**
   ```rust
   #[test]
   fn test_pandoc_div_basic() {
       let input = "::: {.my-class}\ncontent\n:::";
       let result = parse_qmd_to_json(input);

       // This should fail before implementation
       assert!(result.contains("\"t\":\"Div\""));
       assert!(result.contains("\"my-class\""));
   }
   ```

**2. Implement Handler**
   - Uncomment the match arm in treesitter.rs
   - Verify the helper function exists and is correct
   - Run verbose mode to understand tree structure

**3. Verify Test Passes**
   - Run `cargo test --test test_treesitter_refactoring test_<node_name>`
   - Check for warnings or errors

**4. Add Edge Case Tests**
   - Empty content
   - Nested structures
   - Multiple instances
   - With and without attributes
   - Integration with other nodes

### Test Organization

Add to `test_treesitter_refactoring.rs`:

```rust
// ============================================================================
// Div Block Tests
// ============================================================================

#[test]
fn test_pandoc_div_basic() { /* ... */ }

#[test]
fn test_pandoc_div_with_attributes() { /* ... */ }

#[test]
fn test_pandoc_div_nested() { /* ... */ }

#[test]
fn test_pandoc_div_with_complex_content() { /* ... */ }

// ============================================================================
// Note Definition Fenced Block Tests
// ============================================================================

#[test]
fn test_note_definition_fenced_block_basic() { /* ... */ }

#[test]
fn test_note_definition_fenced_block_multi_para() { /* ... */ }

#[test]
fn test_note_definition_fenced_block_complex_content() { /* ... */ }

// ============================================================================
// Pipe Table Tests
// ============================================================================

#[test]
fn test_pipe_table_basic() { /* ... */ }

#[test]
fn test_pipe_table_with_alignment() { /* ... */ }

#[test]
fn test_pipe_table_with_caption() { /* ... */ }

#[test]
fn test_pipe_table_with_formatting() { /* ... */ }
```

## Implementation Workflow

For each handler:

1. **Study the helper function**
   ```bash
   # Read the helper to understand what it expects
   cat crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/<handler>.rs
   ```

2. **Test with verbose mode**
   ```bash
   # Understand the tree structure
   cat /tmp/test.qmd | cargo run --bin quarto-markdown-pandoc -- -v
   ```

3. **Write failing test**
   ```rust
   // In test_treesitter_refactoring.rs
   #[test]
   fn test_<node_name>_basic() {
       let input = "...";
       let result = parse_qmd_to_json(input);
       assert!(...); // Will fail
   }
   ```

4. **Run test to verify it fails**
   ```bash
   cargo test --test test_treesitter_refactoring test_<node_name>_basic
   ```

5. **Uncomment handler in treesitter.rs**
   ```rust
   // Change this:
   // "pandoc_div" => process_fenced_div_block(buf, node, children, context),

   // To this:
   "pandoc_div" => process_fenced_div_block(buf, node, children, context),
   ```

6. **Run test to verify it passes**
   ```bash
   cargo test --test test_treesitter_refactoring test_<node_name>_basic
   ```

7. **Add edge case tests**

8. **Run full suite**
   ```bash
   cargo test --test test_treesitter_refactoring
   ```

## Special Considerations

### pandoc_div
- **Attribute handling**: Divs can have complex attributes ({#id .class key=value})
- **Nesting**: Divs can be nested arbitrarily deep
- **Special div types**: Some divs might have special semantics (e.g., callouts)
- **Note definition divs**: The ::: ^ref syntax is actually a special case of div

### note_definition_fenced_block
- **Relationship to note references**: Must work with [^ref] inline references
- **Multi-block content**: Can contain multiple paragraphs, lists, etc.
- **ID extraction**: The ^ref syntax needs special parsing (^ prefix stripped)
- **Difference from inline_ref_def**: This is for multi-block notes, inline_ref_def is for single-line

### pipe_table
- **Alignment**: Column alignment from delimiter row (`:---`, `:---:`, `---:`)
- **Caption**: Optional caption with `: Caption text` syntax
- **Cell formatting**: Cells can contain inline formatting
- **Escaping**: Pipes can be escaped with backslash
- **Edge cases**:
  - Empty cells
  - Single-column tables
  - Tables with no header
  - Misaligned columns

## Success Criteria

1. ✅ All three block handlers have match arms in treesitter.rs
2. ✅ No "[TOP-LEVEL MISSING NODE]" warnings for supported constructs
3. ✅ All basic tests pass for each handler
4. ✅ All edge case tests pass for each handler
5. ✅ Full test suite passes (135+ tests)
6. ✅ No regressions in existing functionality
7. ✅ Documentation updated (if needed)

## Risks and Mitigation

### Risk: Helper functions might not match current grammar
**Mitigation**: Test with verbose mode first, compare tree structure with helper expectations

### Risk: Attribute parsing might be incomplete
**Mitigation**: Test with various attribute combinations, check attribute helpers

### Risk: Table alignment might be complex
**Mitigation**: Start with simple tables, add alignment as second step

### Risk: Nested divs might cause issues
**Mitigation**: Test simple divs first, then add nesting tests

## Next Steps

1. ✅ Create this plan document
2. ⏭️ Implement `pandoc_div` handler
3. ⏭️ Write comprehensive tests for `pandoc_div`
4. ⏭️ Implement `note_definition_fenced_block` handler
5. ⏭️ Write comprehensive tests for `note_definition_fenced_block`
6. ⏭️ Implement `pipe_table` handlers
7. ⏭️ Write comprehensive tests for `pipe_table`
8. ⏭️ Run full test suite
9. ⏭️ Update k-274 status

## Notes

- All helper functions already exist and appear complete
- This is mainly a "wiring up" task rather than new implementation
- The commented-out code suggests this was working before the grammar refactoring
- Need to verify helpers still match the new grammar structure

## References

- Grammar file: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Main processor: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`
- Helper functions: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/`
- Test file: `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs`
