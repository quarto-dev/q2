# Pipe Table Implementation Plan
**Date**: 2025-10-31
**Issue**: k-303
**Status**: Ready to implement

## Summary

Implement handlers for pipe table (`| Header |` style) syntax in the tree-sitter grammar processor. This is the last remaining block handler from the tree-sitter refactoring epic (k-274).

## Current State

### What's Commented Out (treesitter.rs lines 1117-1124)
```rust
// "pipe_table_delimiter_cell" => process_pipe_table_delimiter_cell(children, context),
// "pipe_table_header" | "pipe_table_row" => {
//     process_pipe_table_header_or_row(node, children, context)
// }
// "pipe_table_delimiter_row" => process_pipe_table_delimiter_row(children, context),
// "pipe_table_cell" => process_pipe_table_cell(node, children, context),
// "caption" => process_caption(node, children, context),
// "pipe_table" => process_pipe_table(node, children, context),
```

### Helper Functions Available
All helper functions already exist in `src/pandoc/treesitter_utils/pipe_table.rs`:
- ✅ `process_pipe_table_delimiter_cell()` - Extracts alignment from delimiter cells
- ✅ `process_pipe_table_header_or_row()` - Processes header and body rows
- ✅ `process_pipe_table_delimiter_row()` - Collects column alignments
- ✅ `process_pipe_table_cell()` - Processes individual cells with inline content
- ✅ `process_pipe_table()` - Main table assembly function

All functions are already imported (lines 37-40).

## Grammar Structure (Studied)

### Tree Structure for Basic Table
```
pipe_table
├── pipe_table_header (aliased pipe_table_row)
│   ├── | (marker)
│   ├── pipe_table_cell (contains inline nodes: pandoc_str, pandoc_space, etc.)
│   ├── | (marker)
│   ├── pipe_table_cell
│   └── | (marker)
├── pipe_table_delimiter_row
│   ├── | (marker)
│   ├── pipe_table_delimiter_cell
│   │   ├── pipe_table_align_left (optional :)
│   │   ├── - (repeated)
│   │   └── pipe_table_align_right (optional :)
│   ├── | (marker)
│   └── pipe_table_delimiter_cell
└── pipe_table_row (repeated)
    ├── | (marker)
    ├── pipe_table_cell
    └── | (marker)
```

### Alignment Markers
- **Left**: `:---` → `pipe_table_align_left` present, no `pipe_table_align_right`
- **Center**: `:---:` → Both `pipe_table_align_left` and `pipe_table_align_right` present
- **Right**: `---:` → Only `pipe_table_align_right` present
- **Default**: `---` → Neither present

### Cell Content
- `pipe_table_cell` contains inline nodes (pandoc_str, pandoc_space, etc.)
- The helper function `process_pipe_table_cell()` already:
  - Collects inline content
  - Trims trailing spaces (to match Pandoc behavior)
  - Wraps content in Plain block

## Test Examples Analyzed

### Basic 2x2 Table
```markdown
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |
| Cell 3   | Cell 4   |
```

**Tree-sitter Output**: ✅ Parses correctly with structure as documented above

**Pandoc Output**: ✅ Produces Table with TableHead, TableBody, 2 columns

### Alignment Table
```markdown
| Left | Center | Right |
|:-----|:------:|------:|
| L1   | C1     | R1    |
```

**Tree-sitter Output**: ✅ Correctly identifies alignment markers:
- Column 1: `pipe_table_align_left` only → AlignLeft
- Column 2: Both markers → AlignCenter
- Column 3: `pipe_table_align_right` only → AlignRight

**Pandoc Output**: ✅ Produces correct alignment in colspec

### Caption Support
**Note**: The tree-sitter grammar does NOT currently have a `caption` node defined for pipe tables. The helper function `process_pipe_table()` has code to handle captions (lines 188-194), but:
- No `caption:` rule found in `grammar.js`
- Test files with captions don't produce caption nodes
- **Action**: Comment out or skip the caption handler for now

## Implementation Steps (TDD)

### Phase 1: Basic Table (Priority 1)
1. ✅ **Study**: Grammar structure, helper code, test examples (DONE)
2. **Write test**: `test_pipe_table_basic()` - 2x2 table with headers
3. **Run test**: Verify it fails with IntermediateUnknown error
4. **Uncomment handlers**:
   - `pipe_table_delimiter_cell`
   - `pipe_table_header | pipe_table_row`
   - `pipe_table_delimiter_row`
   - `pipe_table_cell`
   - `pipe_table`
   - **Skip caption** (not in grammar)
5. **Run test**: Verify it passes
6. **Run full suite**: Ensure no regressions

### Phase 2: Alignment (Priority 1)
7. **Write test**: `test_pipe_table_alignment_left()` - Test `:---`
8. **Write test**: `test_pipe_table_alignment_center()` - Test `:---:`
9. **Write test**: `test_pipe_table_alignment_right()` - Test `---:`
10. **Write test**: `test_pipe_table_alignment_mixed()` - All three types
11. **Run tests**: Verify all alignment tests pass

### Phase 3: Edge Cases (Priority 2)
12. **Write test**: `test_pipe_table_empty_cells()` - Empty cells
13. **Write test**: `test_pipe_table_single_column()` - One column only
14. **Write test**: `test_pipe_table_formatted_cells()` - Bold, code, etc. in cells
15. **Write test**: `test_pipe_table_wide()` - Table with many columns (5+)
16. **Run tests**: Verify all edge case tests pass

### Phase 4: Cleanup
17. **Run full test suite**: Ensure all 166+ tests pass
18. **Format code**: `cargo fmt`
19. **Close issue**: `br close k-303`

## Expected Test Results

### Pandoc AST Structure
```
Table
  (attr: "", [], [])
  (caption: Caption Nothing [])
  (colspec: [(Alignment, ColWidth), ...])
  (head: TableHead with header row)
  (bodies: [TableBody with data rows])
  (foot: TableFoot empty)
```

### Native Format Checks
- Basic: `result.contains("Table")`
- Alignment: `result.contains("AlignLeft")`, `AlignCenter`, `AlignRight`
- Headers: Verify header row in TableHead
- Cells: Verify cell content matches input

## Special Considerations

1. **Caption Handler**: The `process_caption()` import exists but should remain commented out since grammar doesn't support captions yet
2. **Inline Content**: Cells can contain any inline markup (bold, code, links, etc.) - helper already handles this
3. **Empty Header Detection**: Helper code checks if header row is all empty cells and moves it to body if so (lines 200-218)
4. **Trailing Space Trimming**: Helper trims trailing spaces from cells to match Pandoc (lines 135-141)
5. **Block Continuation**: Helper skips `block_continuation` nodes (line 162)

## Files to Modify

- `src/pandoc/treesitter.rs` - Uncomment handlers (lines 1117-1124, skip caption)
- `tests/test_treesitter_refactoring.rs` - Add ~8-10 new tests

## Complexity Assessment

**Complexity**: HIGH (but helpers make it manageable)
- Grammar structure is complex (nested rows/cells, alignment markers)
- BUT: All helper functions exist and are well-tested
- Main task: Uncommenting handlers and writing comprehensive tests
- Estimated effort: 1-2 hours with TDD approach

## Success Criteria

- [ ] All basic table tests pass
- [ ] All alignment tests pass
- [ ] All edge case tests pass
- [ ] No regressions in existing 166 tests
- [ ] Can parse all common pipe table formats
- [ ] Output matches Pandoc for equivalent inputs
