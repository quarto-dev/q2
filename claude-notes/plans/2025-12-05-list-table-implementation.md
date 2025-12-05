# List-Table Implementation Plan

**Beads Issue:** k-mapj
**Created:** 2025-12-05
**Status:** Pending Review

## Overview

Implement bidirectional transformations between list-table div syntax and Pandoc Table AST:
1. **Reader direction (postprocess.rs):** Convert `.list-table` divs to `Table` blocks
2. **Writer direction (qmd.rs):** Convert complex tables to list-table div syntax

The key insight is that pipe tables have limitations (no multi-line cells, no row/col spans), so we need list-table format for complex tables.

## Background: List-Table Structure

A list-table div has the following structure:
```markdown
::: {.list-table header-rows="1" aligns="l,c,r" widths="1,2,1"}
Caption text (optional, any blocks before the bullet list)

* - Header 1
  - Header 2
  - Header 3
* - Cell 1
  - Cell 2
  - Cell 3
:::
```

Each row is a list item in the outer bullet list, containing an inner bullet list of cells.

Cell attributes (colspan, rowspan, alignment) are encoded via an empty span at the start:
```markdown
* - []{colspan="2"} Cell spanning 2 columns
```

## Part 1: Reader Direction (list-table div to Table)

### Location
`src/pandoc/treesitter_utils/postprocess.rs`

### Transformation Logic

Following the existing pattern of `is_valid_definition_list_div` and `transform_definition_list_div`:

#### 1.1 Validation: `validate_list_table_div(div: &Div) -> ListTableValidation`

Return an enum indicating:
- `Valid` - div is a valid list-table, can be transformed
- `NotListTable` - div doesn't have `list-table` class (not an error, just skip)
- `Invalid { reason: String, location: SourceInfo }` - div has `list-table` class but malformed structure

The `Invalid` variant includes `SourceInfo` to enable precise error location reporting.

Validation checks:
- Div has `list-table` class
- Contains blocks where the last block is a BulletList
- Preceding blocks (if any) form the caption
- Each outer bullet list item contains exactly one inner bullet list (the cells)

Invalid structure reasons (for warnings):
- "list-table div must contain at least one bullet list"
- "list-table div's last block must be a bullet list"
- "each row in list-table must contain exactly one bullet list (the cells)"

#### 1.1.1 Error Code Assignment

Add to `error_catalog.json`:
```json
"Q-2-35": {
  "subsystem": "markdown",
  "title": "Invalid List-Table Structure",
  "message_template": "A div with class 'list-table' has an invalid structure and cannot be converted to a table.",
  "docs_url": "https://quarto.org/docs/errors/Q-2-35",
  "since_version": "99.9.9"
}
```

#### 1.2 Parsing: `transform_list_table_div(div: Div) -> Block`

Extract from div attributes:
- `header-rows`: Number of header rows (default: 0)
- `aligns`: Comma-separated alignment chars (l/c/r/d for left/center/right/default)
- `widths`: Comma-separated column width ratios

For each row (outer bullet list item):
- For each cell (inner bullet list item):
  - Check if first inline is an empty span with colspan/rowspan/align attributes
  - Extract cell content (remaining blocks after attribute span)
  - Build `Cell` with appropriate `row_span`, `col_span`, `alignment`

Build Table structure:
- First `header-rows` rows go to `TableHead`
- Remaining rows go to `TableBody`
- Column specs derived from `aligns` and `widths` attributes

### Filter Integration

Add `.with_div()` handler in `postprocess()` function (similar to definition-list pattern):
```rust
.with_div(|div, _ctx| {
    match validate_list_table_div(&div) {
        ListTableValidation::Valid => {
            FilterResult(vec![transform_list_table_div(div)], false)
        }
        ListTableValidation::Invalid { reason, location } => {
            // Emit warning for malformed list-table div with precise location
            error_collector_ref.borrow_mut().push_diagnostic(
                DiagnosticMessageBuilder::warning("Invalid List-Table Structure")
                    .with_code("Q-2-35")
                    .with_location(location)
                    .problem(reason)
                    .add_hint("Check the list-table documentation for the correct structure?")
                    .build()
            );
            Unchanged(div)  // Leave as-is
        }
        ListTableValidation::NotListTable => {
            // Check other div transformations
            if is_valid_definition_list_div(&div) {
                FilterResult(vec![transform_definition_list_div(div)], false)
            } else {
                Unchanged(div)
            }
        }
    }
})
```

## Part 2: Writer Direction (Table to list-table div)

### Location
`src/writers/qmd.rs`

### 2.1 Pipe Table Eligibility Check: `table_can_use_pipe_format(table: &Table) -> bool`

A table can be written as a pipe table if ALL of these are true:
- No cells have `row_span > 1` or `col_span > 1`
- All cells contain only "simple" content:
  - Single `Plain` or `Paragraph` block
  - No `SoftBreak` or `LineBreak` inlines in content
  - (Optional: restrict to single line of text)

Implementation:
```rust
fn table_can_use_pipe_format(table: &Table) -> bool {
    let all_rows = collect_all_rows(table);
    for row in all_rows {
        for cell in &row.cells {
            if cell.row_span > 1 || cell.col_span > 1 {
                return false;
            }
            if !cell_has_simple_content(&cell.content) {
                return false;
            }
        }
    }
    true
}

fn cell_has_simple_content(blocks: &[Block]) -> bool {
    if blocks.len() != 1 {
        return false;
    }
    match &blocks[0] {
        Block::Plain(plain) => !has_breaks(&plain.content),
        Block::Paragraph(para) => !has_breaks(&para.content),
        _ => false,
    }
}

fn has_breaks(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| matches!(inline,
        Inline::SoftBreak(_) | Inline::LineBreak(_)))
}
```

### 2.2 List-Table Writer: `write_list_table(table: &Table, buf, ctx)`

Structure output:
```markdown
::: {.list-table header-rows="N" aligns="..." widths="..."}

Caption text if present

* - Cell 1,1
  - Cell 1,2
* - Cell 2,1
  - Cell 2,2

:::
```

Logic:
1. Write div opening with class and attributes
2. Write caption if present (as regular blocks)
3. Write blank line
4. For each row:
   - Write `* ` for first cell, `  - ` for subsequent cells
   - If cell has non-default span/alignment, prepend `[]{colspan="N" rowspan="N" align="X"}`
   - Write cell content
5. Write closing `:::`

### 2.3 Integration in write_table

Modify `write_table()`:
```rust
fn write_table(table: &Table, buf: &mut dyn Write, ctx: &mut QmdWriterContext) -> io::Result<()> {
    if table_can_use_pipe_format(table) {
        write_pipe_table(table, buf, ctx)  // existing implementation
    } else {
        write_list_table(table, buf, ctx)  // new implementation
    }
}
```

## Part 3: Test Plan

### 3.1 Reader Tests (postprocess)

Create test files in `tests/snapshots/native/`:

| Test File | Content | Purpose |
|-----------|---------|---------|
| `list-table-basic.qmd` | Simple 2x2 table | Basic conversion |
| `list-table-with-header.qmd` | Table with header-rows=1 | Header row handling |
| `list-table-with-caption.qmd` | Table with caption text | Caption extraction |
| `list-table-with-alignments.qmd` | aligns="l,c,r" | Column alignment |
| `list-table-with-widths.qmd` | widths="1,2,1" | Column widths |
| `list-table-colspan.qmd` | Cell with colspan=2 | Column spanning |
| `list-table-rowspan.qmd` | Cell with rowspan=2 | Row spanning |
| `list-table-cell-align.qmd` | Cell-level alignment | Per-cell alignment |
| `list-table-multiline-cell.qmd` | Cell with multiple paragraphs | Complex cell content |
| `list-table-invalid-no-bullet.qmd` | Div with no bullet list | Should remain as Div + warning |
| `list-table-invalid-wrong-last.qmd` | Last block not bullet list | Should remain as Div + warning |
| `list-table-invalid-row-structure.qmd` | Row without inner bullet list | Should remain as Div + warning |

### 3.2 Writer Tests (qmd writer)

Create test files in `tests/roundtrip_tests/qmd-json-qmd/`:

| Test File | Content | Purpose |
|-----------|---------|---------|
| `table_pipe_simple.qmd` | Simple pipe table | Stays as pipe |
| `table_list_colspan.qmd` | List-table with colspan | Roundtrips as list-table |
| `table_list_rowspan.qmd` | List-table with rowspan | Roundtrips as list-table |
| `table_list_multiline.qmd` | List-table with multi-para cell | Roundtrips as list-table |

### 3.3 Integration Tests

Test the full pipeline:
1. Parse list-table div -> get Table AST -> write back to qmd -> should get list-table div
2. Parse pipe table -> get Table AST -> write back to qmd -> should get pipe table

## Implementation Order

1. **Phase 0: Error Catalog**
   - [ ] Add Q-2-35 to `error_catalog.json`

2. **Phase 1: Reader (postprocess.rs)**
   - [ ] Define `ListTableValidation` enum with `Invalid { reason, location }` variant
   - [ ] Add `validate_list_table_div()` validation function (returns validation result with SourceInfo)
   - [ ] Add `transform_list_table_div()` transformation function
   - [ ] Integrate with `.with_div()` handler
   - [ ] Write reader tests, verify they fail
   - [ ] Implement until tests pass

3. **Phase 2: Writer Utility (qmd.rs)**
   - [ ] Add `table_can_use_pipe_format()` function
   - [ ] Add `cell_has_simple_content()` helper
   - [ ] Write eligibility tests, verify they work

4. **Phase 3: List-Table Writer (qmd.rs)**
   - [ ] Add `write_list_table()` function
   - [ ] Modify `write_table()` to choose format
   - [ ] Write roundtrip tests, verify they fail
   - [ ] Implement until tests pass

5. **Phase 4: Integration**
   - [ ] Full roundtrip tests
   - [ ] Edge case testing
   - [ ] Documentation updates

## Design Decisions (Resolved)

1. **Empty cell handling**: Use `- []` (explicit empty span, matches parser input) ✓

2. **Alignment attribute name**: The Lua filter uses `align` for per-cell alignment
   - Match the Lua filter convention for interoperability

3. **Default column widths**: Omit `widths` attribute when all equal/default ✓

4. **Invalid list-table divs**: Leave as-is (don't transform) AND emit warning ✓
   - The QMD writer supports `DiagnosticMessage` output via `ctx.errors`
   - Use `DiagnosticMessageBuilder::warning()` for malformed list-table divs

## Reference Files

- Existing pattern: `src/pandoc/treesitter_utils/postprocess.rs` (definition-list transform)
- Lua reference: `Tools/grid-table-fixer/grid-table-to-list-table.lua`
- Table types: `crates/quarto-pandoc-types/src/table.rs`
- Filter infrastructure: `src/filters.rs`
- Current table writer: `src/writers/qmd.rs` (lines 782-891)
