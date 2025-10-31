# Tree-sitter Grammar Refactoring Plan

**Date**: 2025-10-31
**Status**: In Progress
**Beads Issue**: k-274
**Context**: Major redesign of tree-sitter grammar - all node names changed and grammar now reports much more fine-grained syntax tree

## Problem Statement

The tree-sitter grammar for QMD has been completely redesigned:
- All node names have changed
- Grammar now provides much more fine-grained syntax tree
- All old processing code in `native_visitor` has been commented out
- Need to rewrite the processor to handle new nodes one at a time

## Test Isolation Strategy

### Question: Can `cargo test` run tests from a specific subdirectory?

**Answer**: Yes, using `cargo test --test <test_file_name>`

### Approach:
1. Create new integration test file: `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs`
2. Put ALL new tests for the refactoring work in this file
3. Run only these tests: `cargo test --test test_treesitter_refactoring`
4. This isolates our new tests from the existing (currently failing) test suite
5. Once refactoring is complete, we can integrate these tests back into the main suite

### Test File Structure:
```rust
// tests/test_treesitter_refactoring.rs
use quarto_markdown_pandoc::*;

#[test]
fn test_pandoc_str() {
    let input = "hello";
    // verify AST output
}

#[test]
fn test_pandoc_emph() {
    let input = "*hello*";
    // verify AST output
}

// ... more tests
```

## Refactoring Workflow

For each node type:

1. **Create minimal test document** - simplest possible example
2. **Run in verbose mode** - `echo "document" | cargo run --bin quarto-markdown-pandoc -- --verbose`
3. **Study tree structure** - understand node hierarchy and children
4. **Add node handler** - implement processing in `native_visitor` function
5. **Write test** - add test to `test_treesitter_refactoring.rs`
6. **Verify test passes** - run `cargo test --test test_treesitter_refactoring`
7. **Move to next node**

### Verification Commands:
```bash
# Run verbose to see tree structure
echo "test input" | cargo run --bin quarto-markdown-pandoc -- --verbose

# Run only refactoring tests
cargo test --test test_treesitter_refactoring

# Compare with pandoc output (when needed)
echo "test input" | pandoc -f markdown -t json
```

## Node Types by Category

### Category 1: Basic Text (Priority: CRITICAL)
Already working or next to implement:
- ✅ `document` - working
- ✅ `section` - working
- ✅ `pandoc_paragraph` - working
- ❌ `pandoc_str` - **NEXT TO IMPLEMENT**
- ❌ `pandoc_space` - derived from `_whitespace` token

### Category 2: Basic Inline Formatting (Priority: HIGH)
- `pandoc_emph` - emphasis with * or _
- `pandoc_strong` - strong emphasis with ** or __
- `pandoc_code_span` - inline code with backticks
- `backslash_escape` - escaped characters

### Category 3: Math (Priority: HIGH)
- `pandoc_math` - inline math $...$
- `pandoc_display_math` - display math $$...$$

### Category 4: Links and Images (Priority: HIGH)
- `pandoc_span` - [text](url) or [text]{attrs}
- `pandoc_image` - ![alt](url)
- `target` - the (url) part of links
- `inline_link` - full link construct (from inline grammar)
- `image` - full image construct (from inline grammar)

### Category 5: Advanced Inline (Priority: MEDIUM)
- `pandoc_superscript` - ^superscript^
- `pandoc_subscript` - ~subscript~
- `pandoc_strikeout` - ~~strikeout~~
- `pandoc_single_quote` - 'quoted'
- `pandoc_double_quote` - "quoted"

### Category 6: Editorial Marks (Priority: MEDIUM)
- `insert` - [++text++]
- `delete` - [--text--]
- `highlight` - [!!text!!]
- `edit_comment` - [>>text>>]

### Category 7: Citations and Notes (Priority: MEDIUM)
- `citation` - @citation_key or [@citation_key]
- `inline_note` - ^[note text]
- `note_reference` - [^note_id]

### Category 8: Shortcodes (Priority: MEDIUM)
- `shortcode` - {{< shortcode >}}
- `shortcode_escaped` - {{{< shortcode >}}}
- `shortcode_keyword_param`
- `shortcode_name`, `shortcode_string`, `shortcode_number`, `shortcode_boolean`

### Category 9: Block Structures (Priority: HIGH)
- `atx_heading` - # Heading
- `pandoc_block_quote` - > quote
- `pandoc_list` - bullet and ordered lists
- `list_item` - individual list items
- `pandoc_code_block` - fenced code blocks
- `pandoc_div` - ::: div
- `pandoc_horizontal_rule` - ---

### Category 10: Tables (Priority: LOW)
- `pipe_table`
- `pipe_table_header`
- `pipe_table_row`
- `pipe_table_cell`
- `pipe_table_delimiter_row`
- `pipe_table_delimiter_cell`

### Category 11: Attributes (Priority: HIGH - needed by many nodes)
- `attribute_specifier` - {#id .class key=value}
- `commonmark_specifier` - simplified attribute syntax
- `language_specifier` - for code blocks
- `raw_specifier` - <=format for raw blocks

### Category 12: Helper Nodes (ignore or handle specially)
Delimiter and marker nodes that don't need processing:
- `block_quote_marker`
- `list_marker_*`
- `code_span_delimiter`
- `emphasis_delimiter`
- `fenced_code_block_delimiter`
- Various other delimiter nodes

### Category 13: Metadata and Special (Priority: MEDIUM)
- `metadata` - YAML frontmatter
- `inline_ref_def` - reference definitions
- `note_definition_fenced_block` - ::: ^note

## Implementation Priority Order

### Phase 1: Core Text and Structure (Week 1)
1. ✅ Document structure (document, section) - DONE
2. ✅ Paragraphs (pandoc_paragraph) - DONE
3. **Current**: `pandoc_str` - basic text strings
4. `pandoc_space` - whitespace handling
5. `_newline` / `_soft_line_break` - line break handling

### Phase 2: Basic Formatting (Week 1-2)
6. `pandoc_emph` - emphasis
7. `pandoc_strong` - strong emphasis
8. `pandoc_code_span` - inline code
9. `backslash_escape` - escaped characters

### Phase 3: Structure and Headings (Week 2)
10. `atx_heading` - headings
11. `pandoc_horizontal_rule` - horizontal rules
12. Attributes (`attribute_specifier`) - needed by many nodes

### Phase 4: Links and Math (Week 2-3)
13. `pandoc_math` - inline math
14. `pandoc_display_math` - display math
15. `pandoc_span` / `inline_link` - links
16. `pandoc_image` / `image` - images

### Phase 5: Block Containers (Week 3)
17. `pandoc_block_quote` - block quotes
18. `pandoc_list` / `list_item` - lists
19. `pandoc_code_block` - code blocks
20. `pandoc_div` - divs

### Phase 6: Advanced Inline (Week 3-4)
21. `pandoc_superscript`, `pandoc_subscript`, `pandoc_strikeout`
22. `pandoc_single_quote`, `pandoc_double_quote`
23. Editorial marks (insert, delete, highlight, edit_comment)

### Phase 7: Advanced Features (Week 4)
24. `citation` - citations
25. `inline_note` - inline notes
26. `note_reference` - note references
27. `shortcode` - shortcodes

### Phase 8: Tables and Metadata (Week 4-5)
28. `pipe_table` and related - tables
29. `metadata` - YAML frontmatter
30. Special blocks (note definitions, etc.)

## Testing Strategy

### For Each Node:

1. **Create minimal test**:
   ```rust
   #[test]
   fn test_node_name_basic() {
       let input = "minimal example";
       let result = parse_and_convert(input);
       assert_eq!(result, expected_pandoc_ast);
   }
   ```

2. **Add edge cases**:
   ```rust
   #[test]
   fn test_node_name_empty() { /* ... */ }

   #[test]
   fn test_node_name_nested() { /* ... */ }
   ```

3. **Test interactions**:
   ```rust
   #[test]
   fn test_node_name_with_other_node() { /* ... */ }
   ```

### Test Document Organization:

Store test documents in `tests/refactoring/`:
```
tests/
  test_treesitter_refactoring.rs
  refactoring/
    01_pandoc_str.qmd
    02_pandoc_emph.qmd
    03_pandoc_strong.qmd
    ...
```

## Current Status

### Working:
- ✅ `document`
- ✅ `section`
- ✅ `pandoc_paragraph`

### In Progress:
- 🔄 `pandoc_str` - identified as missing node in "hello" test

### Not Yet Implemented:
- ❌ All other nodes (see lists above)

## Notes and Observations

### From "hello" test:
```
document: {Node document (0, 0) - (1, 0)}
  section: {Node section (0, 0) - (1, 0)}
    pandoc_paragraph: {Node pandoc_paragraph (0, 0) - (1, 0)}
      pandoc_str: {Node pandoc_str (0, 0) - (0, 5)}
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: pandoc_str
```

- Tree structure is clean and hierarchical
- `pandoc_str` is direct child of `pandoc_paragraph`
- Warning system helps identify missing nodes
- Each node has precise location information

### Key Files:
- `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` - block grammar
- `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js` - inline grammar
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs` - main processor
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/` - node processors

### Grammar Insights:
- Block grammar defines document structure and block-level elements
- Inline grammar defines inline elements within blocks
- Many nodes have attributes via `attribute_specifier`
- Emphasis and links have complex precedence rules
- External scanner handles delimiters (backticks, asterisks, etc.)

## Success Criteria

1. All node types have handlers in `native_visitor`
2. Each node type has at least 3 tests (basic, edge case, interaction)
3. All tests in `test_treesitter_refactoring.rs` pass
4. No "[TOP-LEVEL MISSING NODE]" warnings for valid QMD
5. Output matches expected Pandoc AST structure
6. All existing tests pass (re-enable after refactoring complete)

## Risks and Mitigation

### Risk: Existing tests will fail during refactoring
**Mitigation**: Use isolated test file, run only new tests

### Risk: Complex interactions between nodes
**Mitigation**: Build up complexity gradually, test interactions explicitly

### Risk: Grammar changes during refactoring
**Mitigation**: Lock grammar version, communicate changes via beads

### Risk: Token budget exhaustion
**Mitigation**: Work incrementally, commit frequently, create subtasks

## Next Steps

1. ✅ Create this plan document
2. ⏭️ Create beads issue and link to this plan
3. ⏭️ Create `test_treesitter_refactoring.rs`
4. ⏭️ Implement `pandoc_str` handler
5. ⏭️ Write tests for `pandoc_str`
6. ⏭️ Verify tests pass
7. ⏭️ Continue with `pandoc_space`
8. ⏭️ ... (work through priority list)

## References

- [Tree-sitter documentation](https://tree-sitter.github.io/tree-sitter/)
- [Pandoc AST documentation](https://hackage.haskell.org/package/pandoc-types)
- Grammar files in `crates/tree-sitter-qmd/`
- Existing processor utilities in `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/`
