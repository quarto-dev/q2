# Block Handlers Implementation Plan - ACCURATE Assessment

**Date**: 2025-10-31
**Beads Issue**: k-274 (tree-sitter grammar refactoring - block handlers phase)
**Status**: Most block handlers are NOT implemented

## Critical Reality Check

I previously claimed many block handlers were working. This was **completely wrong**.

### Actually Implemented ✅ (Only 5 block-level handlers)

1. **`document`** - Top-level document container
2. **`section`** - Section hierarchy created by headings
3. **`pandoc_paragraph`** - Basic paragraphs
4. **`atx_heading`** - ATX-style headings (# Heading)
5. **`inline_ref_def`** - Single-line note definitions ([^note]: text)

**Everything else for blocks is missing or commented out.**

### All Missing Block Handlers ❌ (7 major block types)

According to the grammar (`_block_not_section` in grammar.js):

1. **`pandoc_block_quote`** - Block quotes (> quote)
   - Status: Commented out (line 1098)
   - Helper: `process_block_quote` exists
   - Impact: **CRASHES** on any block quote

2. **`pandoc_list`** - Lists (bullet and ordered)
   - Status: Commented out (line 1093)
   - Helper: `process_list` exists
   - Also needs: `list_item` handler
   - Impact: **CRASHES** on any list

3. **`pandoc_code_block`** - Fenced code blocks (```code```)
   - Status: Commented out (line 943 as "fenced_code_block")
   - Helper: `process_fenced_code_block` exists
   - Impact: **CRASHES** on any fenced code block

4. **`pandoc_horizontal_rule`** - Horizontal rules (---)
   - Status: Commented out (line 1101 as "thematic_break")
   - Helper: `process_thematic_break` exists
   - Impact: **CRASHES** on horizontal rules

5. **`pandoc_div`** - Fenced divs (::: {.class})
   - Status: Commented out (line 1099 as "fenced_div_block")
   - Helper: `process_fenced_div_block` exists
   - Impact: **CRASHES** on any div

6. **`note_definition_fenced_block`** - Multi-block note defs (::: ^ref)
   - Status: Commented out (line 992-994)
   - Helper: `process_note_definition_fenced_block` exists
   - Impact: **Warning**, likely crashes

7. **`pipe_table`** - Pipe tables (| Header |)
   - Status: Commented out (line 1118)
   - Helpers: Complete suite exists (table, row, cell, delimiter, caption)
   - Impact: **CRASHES** on any table

### Inline Handlers Status ✅

Most inline handlers ARE working:
- Text: str, space, soft_break
- Formatting: emphasis, strong, strikeout, super/subscript
- Editorial: insert, delete, highlight, edit_comment
- Code: code_span (inline code)
- Math: inline and display math
- Citations: @cite, [-@cite]
- Notes: inline notes ^[note], note references [^ref]
- Quotes: single and double quotes
- Links/Images: spans and images
- Shortcodes: {{< shortcode >}}
- Attributes: full attribute support

## Implementation Statistics

- **Uncommented handlers**: 70
- **Commented handlers**: 58
- **Block handlers working**: 5 out of 12 (42%)
- **Block handlers missing**: 7 out of 12 (58%)

## Why Everything is Crashing

When the parser encounters an unhandled block node, it returns `IntermediateUnknown`. The postprocessing step expects `IntermediateBlock` or `IntermediateSection` for block contexts, so it panics with:

```
Expected Block or Section, got IntermediateUnknown
```

This means **any document with a block quote, list, code block, div, table, or horizontal rule will crash**.

## Implementation Priority

All 7 missing block handlers are **CRITICAL** because they all cause crashes. We need to implement them in dependency order:

### Phase 1: Basic Containers (No dependencies)

**1. `pandoc_block_quote`** - Block Quotes
- **Priority**: CRITICAL
- **Complexity**: Low
- **Dependencies**: None (just contains other blocks)
- **Example**:
  ```markdown
  > This is a quote
  > Second line
  ```
- **Helper**: `process_block_quote` exists
- **Test files**: Should exist in old test suite

**2. `pandoc_horizontal_rule`** - Horizontal Rules
- **Priority**: CRITICAL (but simple)
- **Complexity**: Very Low (no children, just marker)
- **Dependencies**: None
- **Example**:
  ```markdown
  ---
  ```
- **Helper**: `process_thematic_break` exists
- **Implementation**: Line 1101, uncomment

**3. `pandoc_code_block`** - Fenced Code Blocks
- **Priority**: CRITICAL
- **Complexity**: Medium (language, attributes, content)
- **Dependencies**: Attribute handling (already working)
- **Example**:
  ```markdown
  ```python
  print("hello")
  ```
  ```
- **Helper**: `process_fenced_code_block` exists
- **Note**: Only backtick fences (no tildes), no indented code blocks

### Phase 2: Lists (Recursive structure)

**4. `pandoc_list` + `list_item`** - Lists
- **Priority**: CRITICAL
- **Complexity**: High (ordered/unordered, nesting, list attributes)
- **Dependencies**: None, but recursive (lists can contain lists)
- **Examples**:
  ```markdown
  - Bullet item
  - Another item
    - Nested item

  1. Ordered item
  2. Another item
  ```
- **Helpers**: `process_list` and `process_list_item` exist
- **Special handling**: List marker parsing (already has code for this)

### Phase 3: Divs (Containers with attributes)

**5. `pandoc_div`** - Fenced Divs
- **Priority**: CRITICAL
- **Complexity**: Medium (attributes, nested content)
- **Dependencies**: Attribute handling (working)
- **Example**:
  ```markdown
  ::: {.callout-note}
  This is a div
  :::
  ```
- **Helper**: `process_fenced_div_block` exists
- **Special case**: This is the base for note_definition_fenced_block

**6. `note_definition_fenced_block`** - Fenced Note Definitions
- **Priority**: HIGH (user-requested feature)
- **Complexity**: Medium (similar to div, extracts ^id)
- **Dependencies**: Same structure as div
- **Example**:
  ```markdown
  ::: ^mynote
  Multi-block note content.

  Second paragraph.
  :::
  ```
- **Helper**: `process_note_definition_fenced_block` exists
- **Special handling**: Extracts ^ref from div syntax

### Phase 4: Tables (Complex structure)

**7. `pipe_table`** - Pipe Tables
- **Priority**: MEDIUM (complex, can defer)
- **Complexity**: Very High (alignment, caption, multiple sub-nodes)
- **Dependencies**: None, but complex structure
- **Example**:
  ```markdown
  | Header 1 | Header 2 |
  |----------|----------|
  | Cell 1   | Cell 2   |

  : Table caption
  ```
- **Helpers**: Complete suite exists:
  - `process_pipe_table`
  - `process_pipe_table_header_or_row` (lines 1112-1113)
  - `process_pipe_table_delimiter_row` (line 1115)
  - `process_pipe_table_delimiter_cell` (line 1111)
  - `process_pipe_table_cell` (line 1116)
  - `process_caption` (line 1117)
- **Sub-tasks**: Need to uncomment 6 different handlers

## Implementation Strategy

### For Each Handler - TDD Approach

**1. Write Failing Test FIRST**
```rust
#[test]
fn test_block_quote_basic() {
    let input = "> quote";
    let result = parse_qmd_to_json(input);

    // This WILL crash before implementation
    assert!(result.contains("\"t\":\"BlockQuote\""));
}
```

**2. Run Test to See Crash**
```bash
cargo test --test test_treesitter_refactoring test_block_quote_basic
# Should see: "Expected Block or Section, got IntermediateUnknown"
```

**3. Uncomment Handler**
Find the commented line in treesitter.rs and uncomment it:
```rust
// Before:
// "pandoc_block_quote" => process_block_quote(buf, node, children, context),

// After:
"pandoc_block_quote" => process_block_quote(buf, node, children, context),
```

**4. Run Test Again - Should Pass**
```bash
cargo test --test test_treesitter_refactoring test_block_quote_basic
```

**5. Add Edge Case Tests**
- Empty content
- Nested structures
- Multiple instances
- Integration tests

### Verification Commands

For each handler implementation:

```bash
# Test with verbose to see tree structure
echo "test input" | cargo run --bin quarto-markdown-pandoc -- -v

# Run specific test
cargo test --test test_treesitter_refactoring test_<handler>

# Run all refactoring tests
cargo test --test test_treesitter_refactoring

# Test actual document
cat /tmp/test.qmd | cargo run --bin quarto-markdown-pandoc --
```

## Handler Implementation Checklist

For each of the 7 handlers:

- [ ] **pandoc_block_quote**
  - [ ] Write failing test (basic)
  - [ ] Uncomment handler (line 1098)
  - [ ] Verify test passes
  - [ ] Add tests: empty, nested, multi-line
  - [ ] Test with other blocks inside

- [ ] **pandoc_horizontal_rule**
  - [ ] Write failing test
  - [ ] Uncomment handler (line 1101)
  - [ ] Verify test passes
  - [ ] Add tests: multiple, in context

- [ ] **pandoc_code_block**
  - [ ] Write failing test (no language)
  - [ ] Uncomment handler (line 943)
  - [ ] Verify test passes
  - [ ] Add tests: with language, with attrs, empty, multi-line

- [ ] **pandoc_list** and **list_item**
  - [ ] Write failing test (bullet list)
  - [ ] Uncomment both handlers (lines 1093-1094)
  - [ ] Verify test passes
  - [ ] Add tests: ordered, nested, tight/loose, mixed

- [ ] **pandoc_div**
  - [ ] Write failing test (no attrs)
  - [ ] Uncomment handler (line 1099)
  - [ ] Verify test passes
  - [ ] Add tests: with attrs, nested, complex content

- [ ] **note_definition_fenced_block**
  - [ ] Write failing test (basic)
  - [ ] Uncomment handler (line 992-994)
  - [ ] Verify test passes
  - [ ] Add tests: multi-block, complex content, with references

- [ ] **pipe_table**
  - [ ] Write failing test (basic 2x2)
  - [ ] Uncomment ALL 6 handlers (lines 1111-1118)
  - [ ] Verify test passes
  - [ ] Add tests: alignment, caption, formatting, edge cases

## Expected Timeline

Given that helpers exist and are presumably working:

- **Phase 1** (Basic Containers): ~2-3 hours
  - Block quote: 30 min
  - Horizontal rule: 15 min
  - Code block: 1-1.5 hours

- **Phase 2** (Lists): ~2-3 hours
  - Basic lists: 1 hour
  - Nested lists: 1 hour
  - Edge cases: 1 hour

- **Phase 3** (Divs): ~2-3 hours
  - Basic div: 1 hour
  - Note definition: 1 hour
  - Edge cases: 1 hour

- **Phase 4** (Tables): ~3-4 hours
  - Basic table: 1 hour
  - Alignment/caption: 1 hour
  - Edge cases: 1-2 hours

**Total**: ~9-13 hours of focused work

## Success Criteria

1. ✅ All 7 block handlers uncommented in treesitter.rs
2. ✅ No "[TOP-LEVEL MISSING NODE]" warnings for supported blocks
3. ✅ No "Expected Block or Section, got IntermediateUnknown" crashes
4. ✅ Each handler has 5+ tests (basic + edge cases)
5. ✅ All 135+ existing inline tests still pass
6. ✅ All new block tests pass
7. ✅ Can parse comprehensive test document with all block types

## Risks

1. **Helper functions may not match new grammar**
   - Mitigation: Test with verbose mode first, compare tree structure

2. **Attribute handling may be incomplete**
   - Mitigation: Attributes already work for other nodes, should be fine

3. **List nesting may be complex**
   - Mitigation: Start with simple non-nested lists, add nesting incrementally

4. **Table implementation is very complex**
   - Mitigation: Save for last, break into sub-tasks

## Next Steps

1. ✅ Create this accurate plan
2. ⏭️ Implement `pandoc_block_quote` (simplest, no dependencies)
3. ⏭️ Implement `pandoc_horizontal_rule` (trivial)
4. ⏭️ Implement `pandoc_code_block`
5. ⏭️ Implement `pandoc_list` + `list_item`
6. ⏭️ Implement `pandoc_div`
7. ⏭️ Implement `note_definition_fenced_block`
8. ⏭️ Implement `pipe_table` and all sub-handlers
9. ⏭️ Run full test suite
10. ⏭️ Close k-274

## Notes

- This is primarily "wiring up" work - the hard implementation is done
- All helpers exist in `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/`
- The commented code suggests this worked before the grammar refactoring
- Need to verify helpers still match new grammar node structure
- TDD approach is critical - write failing tests first
