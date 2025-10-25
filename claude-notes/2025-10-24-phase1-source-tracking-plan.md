# Phase 1: Add Source Tracking for Structural Elements - Implementation Plan

**Beads Issue**: k-194
**Date**: 2025-10-24
**Context**: See design document in claude-notes/2025-10-24-structural-components-design.md

## Analysis Summary

After studying the codebase, particularly `filters.rs`, `table.rs`, and `pipe_table.rs`, I've identified the following key points:

### Filter Infrastructure

**Observation**: Structural elements (Row, Cell, TableHead, TableBody, TableFoot, Caption) are NOT separately filterable.

From `filters.rs:31-82`, the Filter struct has fields for semantic nodes (Blocks, Inlines) but NOT for structural sub-components:
- ✓ Has: `table: BlockFilterField<'a, pandoc::Table>`
- ✗ Does NOT have: `row`, `cell`, `table_head`, etc.

**Traversal Pattern** (`filters.rs:931-943`):
```rust
fn traverse_row(row: crate::pandoc::Row, filter: &mut Filter) -> crate::pandoc::Row {
    crate::pandoc::Row {
        cells: row
            .cells
            .into_iter()
            .map(|cell| crate::pandoc::Cell {
                content: topdown_traverse_blocks(cell.content, filter),
                ..cell  // ← Spread operator preserves all other fields
            })
            .collect(),
        ..row  // ← Spread operator preserves all other fields
    }
}
```

**Key Insight**: The spread operator (`..row`, `..cell`) will automatically preserve any new `source_info` fields we add. **No changes needed to filters.rs** as long as structural elements remain non-filterable.

### Current Type Structure

From `table.rs`:
```rust
pub struct Row {
    pub attr: Attr,
    pub cells: Vec<Cell>,
    pub attr_source: AttrSourceInfo,  // ← Already has attr_source
    // ✗ NO source_info field
}

pub struct Cell {
    pub attr: Attr,
    pub alignment: Alignment,
    pub row_span: usize,
    pub col_span: usize,
    pub content: Blocks,
    pub attr_source: AttrSourceInfo,
    // ✗ NO source_info field
}

pub struct TableHead {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub attr_source: AttrSourceInfo,
    // ✗ NO source_info field
}

pub struct TableBody {
    pub attr: Attr,
    pub rowhead_columns: usize,
    pub head: Vec<Row>,
    pub body: Vec<Row>,
    pub attr_source: AttrSourceInfo,
    // ✗ NO source_info field
}

pub struct TableFoot {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub attr_source: AttrSourceInfo,
    // ✗ NO source_info field
}

pub struct Table {
    pub attr: Attr,
    pub caption: Caption,
    pub colspec: Vec<ColSpec>,
    pub head: TableHead,
    pub bodies: Vec<TableBody>,
    pub foot: TableFoot,
    pub source_info: quarto_source_map::SourceInfo,  // ← Already has source_info
    pub attr_source: AttrSourceInfo,
}
```

From `caption.rs`:
```rust
pub struct Caption {
    pub short: Option<Inlines>,
    pub long: Option<Blocks>,
    // ✗ NO source_info field
}
```

### Parser Construction Pattern

From `pipe_table.rs:147-256`, the `process_pipe_table()` function shows:

```rust
// Row constructed without source info (line 53-56)
let mut row = Row {
    attr: empty_attr(),
    cells: Vec::new(),
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
};

// Cell constructed without source info (line 107-114)
let mut table_cell = Cell {
    alignment: Alignment::Default,
    col_span: 1,
    row_span: 1,
    attr: ("".to_string(), vec![], HashMap::new()),
    content: vec![],
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
};

// TableHead/Body/Foot constructed without source info (lines 236-252)
head: TableHead {
    attr: empty_attr(),
    rows: thead_rows,
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
},
bodies: vec![TableBody {
    attr: empty_attr(),
    rowhead_columns: 0,
    head: vec![],
    body: body_rows,
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
}],
foot: TableFoot {
    attr: empty_attr(),
    rows: vec![],
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
},
```

**Challenge**: Need to capture tree-sitter node ranges for these structural elements during parsing.

### JSON Serialization

Need to check `writers/json.rs` to understand how tables are currently serialized and where to add `s` fields.

## Implementation Plan

### Step 1: Add source_info Fields to Types

**File**: `src/pandoc/table.rs`

**Changes**:
```rust
pub struct Row {
    pub attr: Attr,
    pub cells: Vec<Cell>,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub attr_source: AttrSourceInfo,
}

pub struct Cell {
    pub attr: Attr,
    pub alignment: Alignment,
    pub row_span: usize,
    pub col_span: usize,
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub attr_source: AttrSourceInfo,
}

pub struct TableHead {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub attr_source: AttrSourceInfo,
}

pub struct TableBody {
    pub attr: Attr,
    pub rowhead_columns: usize,
    pub head: Vec<Row>,
    pub body: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub attr_source: AttrSourceInfo,
}

pub struct TableFoot {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub attr_source: AttrSourceInfo,
}
```

**File**: `src/pandoc/caption.rs`

**Changes**:
```rust
pub struct Caption {
    pub short: Option<Inlines>,
    pub long: Option<Blocks>,
    pub source_info: quarto_source_map::SourceInfo,  // NEW
}
```

**Rationale**: Following the pattern used by Table itself (which already has source_info).

---

### Step 2: Update Parser to Capture Source Locations

**File**: `src/pandoc/treesitter_utils/pipe_table.rs`

**2.1: Update `process_pipe_table_header_or_row()` (line 49)**

```rust
pub fn process_pipe_table_header_or_row(
    node: &tree_sitter::Node,  // NEW: need node parameter
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,  // Already has context
) -> PandocNativeIntermediate {
    let mut row = Row {
        attr: empty_attr(),
        cells: Vec::new(),
        source_info: node_source_info_with_context(node, context),  // NEW
        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
    };
    // ... rest of function
}
```

**2.2: Update `process_pipe_table_cell()` (line 101)**

Already has `node` parameter. Just add:
```rust
let mut table_cell = Cell {
    alignment: Alignment::Default,
    col_span: 1,
    row_span: 1,
    attr: ("".to_string(), vec![], HashMap::new()),
    content: vec![],
    source_info: node_source_info_with_context(node, context),  // NEW
    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
};
```

**2.3: Update `process_pipe_table()` (line 147)**

Need to capture separate nodes for TableHead, TableBody, TableFoot, Caption.

**Challenge**: Current parsing doesn't have explicit nodes for thead/tbody/tfoot - they're implicitly constructed based on header presence. Need to:
- Use header node's range for TableHead source
- Use body rows' aggregate range for TableBody source
- Use empty/zero range for TableFoot (since pipe tables don't have footers)
- Capture caption node range when present

```rust
// For TableHead - use range of header row if present
let thead_source_info = if let Some(ref header_row) = header {
    header_row.source_info.clone()  // Use header row's source
} else {
    // Empty thead - use table node's start position with zero length
    // Or: compute from first delimiter row
    node_source_info_with_context(node, context) // placeholder, refine later
};

// For TableBody - compute from body rows
let tbody_source_info = if !rows.is_empty() {
    // Compute range from first to last body row
    let first = &rows[0].source_info;
    let last = &rows[rows.len() - 1].source_info;
    // Need helper to merge SourceInfo ranges
    merge_source_info_range(first, last, context)
} else {
    node_source_info_with_context(node, context)
};

// For TableFoot - pipe tables don't have footers, use zero-length at table end
let tfoot_source_info = node_source_info_with_context(node, context); // placeholder

// For Caption - use caption node if present
let (caption, caption_source_info) = if let Some(inlines) = caption_inlines {
    let cap_source = /* need to capture from table_caption node */;
    (Caption {
        short: None,
        long: Some(vec![Block::Plain(Plain {
            content: inlines,
            source_info: cap_source.clone(),
        })]),
        source_info: cap_source,
    }, cap_source)
} else {
    (Caption {
        short: None,
        long: None,
        source_info: node_source_info_with_context(node, context), // zero-length
    }, node_source_info_with_context(node, context))
};
```

**Note**: This reveals a complexity - some structural elements are implicit (tbody, tfoot) and don't have direct tree-sitter nodes. Options:
1. Compute ranges by merging child source infos
2. Use placeholder/synthetic source infos for implicit structures
3. Only track source for explicit structures (rows, cells)

**Recommendation**: Start with explicit structures (Row, Cell) and use computed ranges for implicit containers (TableHead, TableBody, TableFoot computed from their rows).

---

### Step 3: Helper Function for Merging Source Ranges

**File**: `src/pandoc/location.rs` (or wherever source info helpers live)

**New function**:
```rust
pub fn merge_source_info_range(
    start: &quarto_source_map::SourceInfo,
    end: &quarto_source_map::SourceInfo,
    _context: &ASTContext,
) -> quarto_source_map::SourceInfo {
    // Merge two SourceInfo to create a range spanning both
    // Implementation depends on SourceInfo structure
    // May need to check if they're from same file, etc.
    todo!("Implement source info merging")
}
```

**Alternative**: Use first/last child's source as approximation if exact merging is complex.

---

### Step 4: Update All Parser Call Sites

**Challenge**: `process_pipe_table_header_or_row()` signature changed to add `node` parameter.

**Files to update**: Need to grep for all call sites and add node parameter:
```bash
grep -r "process_pipe_table_header_or_row" src/
```

Likely in the dispatching code that calls these functions from the tree-sitter traversal.

---

### Step 5: Update JSON Serialization

**File**: `src/writers/json.rs`

Need to add `s` field serialization for Row, Cell, TableHead, TableBody, TableFoot, Caption.

**Pattern** (from existing code for blocks/inlines):
```rust
// Example from existing serialization
"s": serializer.to_json_ref(&inline.source_info)
```

**Changes needed**:
```rust
// For Row
json!({
    "attr": write_attr(&row.attr),
    "cells": row.cells.iter().map(|cell| write_cell(cell, serializer)).collect::<Vec<_>>(),
    "s": serializer.to_json_ref(&row.source_info),  // NEW
    "attrS": write_attr_source(&row.attr_source, serializer),
})

// For Cell
json!({
    "attr": write_attr(&cell.attr),
    "alignment": write_alignment(&cell.alignment),
    "rowSpan": cell.row_span,
    "colSpan": cell.col_span,
    "content": cell.content.iter().map(...).collect::<Vec<_>>(),
    "s": serializer.to_json_ref(&cell.source_info),  // NEW
    "attrS": write_attr_source(&cell.attr_source, serializer),
})

// Similar for TableHead, TableBody, TableFoot, Caption
```

---

### Step 6: Update JSON Deserialization

**File**: `src/readers/json.rs`

Need to handle `s` field when reading JSON back into Rust structs.

**Pattern**:
```rust
// Read source_info from JSON
let source_info = read_source_info(&json["s"], context)?;
```

---

### Step 7: Filter Infrastructure

**File**: `src/filters.rs`

**Change needed**: NONE (if we keep structural elements non-filterable)

The spread operator in existing traverse functions will automatically preserve source_info:
- `traverse_row()` line 931: `..row` preserves source_info
- Cell reconstruction line 936: `..cell` preserves source_info

**Verification**: Run existing filter tests to ensure nothing breaks.

**Alternative** (if we want to make structural elements filterable):
Add filter fields:
```rust
pub struct Filter<'a> {
    // ... existing fields ...
    pub row: Option<...>,  // NEW
    pub cell: Option<...>,  // NEW
    pub table_head: Option<...>,  // NEW
    // etc.
}
```

**Recommendation**: Keep them non-filterable for now. Can add filter support later if needed.

---

### Step 8: Handle Lists (Optional for Phase 1)

**Consideration**: Should we also add source tracking for list items?

**Current structure** (from filters.rs:442-486):
- BulletList: `content: Vec<Blocks>` - items are just `Vec<Block>`
- OrderedList: same pattern
- DefinitionList: `content: Vec<(Inlines, Vec<Blocks>)>` - tuples, no struct

**Options**:
1. **Defer to separate issue**: List items are tuples/arrays, not structs. Would require more design work.
2. **Create new structs**: Define `ListItem`, `DefinitionListItem` structs with source_info.

**Recommendation**: **Defer lists** to a follow-up issue. Focus Phase 1 on tables since:
- Tables already have proper struct types
- Tables have the most complex structure
- Lists would require changing from tuples to structs (more invasive)

Create a separate beads issue for list source tracking after tables are proven out.

---

### Step 9: Testing Strategy

**9.1: Unit Tests**

Create test in `tests/test_attr_source_parsing.rs` (or new file):
```rust
#[test]
fn test_table_row_source_info() {
    let input = "| A | B |\n| C | D |";
    let pandoc = parse_qmd(input);
    let json = to_json(&pandoc);

    // Verify Row has 's' field
    let table = &json["blocks"][0];
    let row = &table["c"][3]["rows"][0];  // First row in head
    assert!(row["s"].is_number(), "Row should have source info");
}

#[test]
fn test_table_cell_source_info() {
    let input = "| Cell A |";
    let pandoc = parse_qmd(input);
    let json = to_json(&pandoc);

    let cell = &json["blocks"][0]["c"][3]["rows"][0]["cells"][0];
    assert!(cell["s"].is_number(), "Cell should have source info");
}

// Similar for TableHead, TableBody, TableFoot, Caption
```

**9.2: Integration Tests**

Run existing test suite:
```bash
cargo test
```

Verify:
- All existing tests still pass
- Filter tests work (spread operator preserves source_info)
- JSON roundtrip tests work (de/serialization)

**9.3: Manual Testing**

Create sample tables and verify JSON output has `s` fields at all levels:
```bash
echo "| A | B |\n| C | D |" | cargo run -- -t json | jq .
```

Check that all structural elements have `s` fields.

---

## Implementation Order

1. ✅ **Study phase** (COMPLETED)
   - Understand filters.rs
   - Understand table types
   - Understand parser

2. **Type changes** (LOW RISK)
   - Add source_info to Row, Cell, TableHead, TableBody, TableFoot, Caption
   - Run `cargo check` - expect compilation errors in parsers

3. **Parser updates** (MEDIUM RISK)
   - Update pipe_table.rs to capture source ranges
   - Fix compilation errors from step 2
   - Run `cargo check` until clean

4. **JSON serialization** (LOW RISK)
   - Add `s` field to JSON writer
   - Test with manual table examples

5. **JSON deserialization** (MEDIUM RISK)
   - Add `s` field reading
   - Test roundtrip

6. **Testing** (VERIFICATION)
   - Write unit tests
   - Run full test suite
   - Verify no regressions

7. **Documentation**
   - Update TypeScript types (k-195 will handle)
   - Document in claude-notes if needed

---

## Open Questions / Decisions Needed

### Q1: Implicit Structural Elements

TableHead, TableBody, TableFoot are constructed implicitly in pipe tables (no explicit tree-sitter nodes).

**Options**:
- A) Compute source range by merging first/last child ranges
- B) Use table's overall range as placeholder
- C) Use zero-length source info at logical position

**Recommendation**: Option A (computed ranges) is most accurate

---

### Q2: Empty Structures

What source_info for empty TableFoot (pipe tables never have footers)?

**Options**:
- A) Zero-length range at end of table
- B) Clone table's range
- C) Sentinel value indicating "no source"

**Recommendation**: Option A (zero-length at logical position)

---

### Q3: Caption Source

Caption can be absent, or have short/long parts. What source_info?

**Options**:
- A) Capture from `table_caption` tree-sitter node when present
- B) Compute from short/long content ranges
- C) Use table's range when caption absent

**Recommendation**: Option A for present captions, Option C for absent

---

### Q4: Filter Support

Should structural elements be separately filterable?

**Current**: NO - only semantic nodes (Block, Inline) are filterable
**Proposed**: Keep as-is, spread operator preserves source_info

**If YES needed later**:
- Add filter fields to Filter struct
- Add traverse hooks
- Define FilterReturn semantics for structural transforms

**Recommendation**: Keep non-filterable for Phase 1

---

### Q5: List Items

Should Phase 1 include list item source tracking?

**Complexity**:
- Would require new structs (ListItem, etc.)
- Currently just tuples/vectors
- Less urgent than tables

**Recommendation**: Defer to separate issue after tables proven

---

## Risk Assessment

**LOW RISK**:
- Type additions (just new fields)
- JSON serialization (additive)
- Filter preservation (spread operator automatic)

**MEDIUM RISK**:
- Parser changes (need to capture correct ranges)
- Implicit structure source computation
- JSON deserialization (need to handle missing 's' in old JSON)

**HIGH RISK**:
- None identified

**MITIGATION**:
- Comprehensive testing at each step
- Manual verification of source ranges
- Keep changes incremental
- Run full test suite frequently

---

## Success Criteria

✅ Phase 1 complete when:
1. Row, Cell, TableHead, TableBody, TableFoot, Caption all have `source_info` fields
2. Parser captures source ranges for these structures
3. JSON output includes `s` field for all structural elements
4. JSON reader can deserialize with `s` fields
5. Filter traversal preserves `source_info` (no regressions)
6. All existing tests pass
7. New tests verify source locations are correct
8. TypeScript can proceed with k-195 (structural AnnotatedParse nodes)

---

## Next Steps After This Plan

1. Get approval on design decisions (Q1-Q5 above)
2. Start implementation at Step 2 (Type changes)
3. Work through steps incrementally
4. Report back with any unexpected issues
5. Once complete, unblock k-195 (TypeScript structural nodes)
