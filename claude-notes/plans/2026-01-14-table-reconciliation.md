# Table Reconciliation Plan

**Date:** 2026-01-14
**Status:** ✅ COMPLETE
**Parent:** k-xvte (Design structural hash-based AST reconciliation algorithm)
**Epic:** kyoto-tsq (complete)

## Problem Statement

Tables are semantic containers that hold nested block content in cells, but the reconciliation system currently doesn't recursively reconcile table cell content. This means:

1. **Same hash** → `KeepBefore` → Uses original table (correct)
2. **Different hash** → `UseAfter` → Uses executed table entirely (loses source_info preservation opportunity)

When a table cell contains content that matches between `before` and `after`, we lose the opportunity to preserve that content's `source_info`. For example:

```
Before:                           After:
┌─────────┬─────────┐            ┌─────────┬─────────┐
│ Hello   │ World   │            │ Hello   │ Changed │
├─────────┼─────────┤            ├─────────┼─────────┤
│ Foo     │ Bar     │            │ Foo     │ Bar     │
└─────────┴─────────┘            └─────────┴─────────┘

Current behavior: Entire table replaced (UseAfter)
Desired behavior: "Hello", "Foo", "Bar" cells preserve source_info
```

## Why Table Isn't Currently a Container

In compute.rs:179-189, `is_container_block()` determines which blocks get `RecurseIntoContainer` alignment:

```rust
fn is_container_block(block: &Block) -> bool {
    matches!(
        block,
        Block::Div(_)
            | Block::BlockQuote(_)
            | Block::OrderedList(_)
            | Block::BulletList(_)
            | Block::DefinitionList(_)
            | Block::Figure(_)
            | Block::Custom(_)
    )
    // Note: Block::Table(_) is NOT listed
}
```

This was intentional because Table's nested structure is more complex than other containers:
- Table → head/bodies/foot → rows → cells → content: Blocks
- Multiple levels of nesting require special handling

## Table Structure

```rust
pub struct Table {
    pub attr: Attr,
    pub caption: Caption,           // has long: Option<Blocks>
    pub colspec: Vec<ColSpec>,
    pub head: TableHead,            // has rows: Vec<Row>
    pub bodies: Vec<TableBody>,     // each has head: Vec<Row>, body: Vec<Row>
    pub foot: TableFoot,            // has rows: Vec<Row>
    pub source_info: SourceInfo,
    pub attr_source: AttrSourceInfo,
}

pub struct Row {
    pub attr: Attr,
    pub cells: Vec<Cell>,
    pub source_info: SourceInfo,
    pub attr_source: AttrSourceInfo,
}

pub struct Cell {
    pub attr: Attr,
    pub alignment: Alignment,
    pub row_span: usize,
    pub col_span: usize,
    pub content: Blocks,            // <-- Nested block content!
    pub source_info: SourceInfo,
    pub attr_source: AttrSourceInfo,
}
```

## Design Approach

### Cell Matching Strategy: Position-Based

Unlike list items (which can be reordered), table cells have semantic positions (row, column). A cell at position (2, 3) in the before table corresponds to position (2, 3) in the after table.

Position-based matching:
- Simple to implement
- Semantically correct for tables
- Gracefully handles structural changes (if table grows/shrinks, unmatched cells use exec's content)

### Plan Structure

Add a new `TableReconciliationPlan` type:

```rust
/// Plan for reconciling a Table's nested content.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableReconciliationPlan {
    /// Plan for caption.long (if both tables have long captions)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub caption_plan: Option<ReconciliationPlan>,

    /// Plans for head cells, indexed by (row_index, cell_index)
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub head_cell_plans: LinkedHashMap<(usize, usize), ReconciliationPlan>,

    /// Plans for body cells, indexed by (body_index, is_head_row, row_index, cell_index)
    /// is_head_row distinguishes TableBody.head from TableBody.body
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub body_cell_plans: LinkedHashMap<(usize, bool, usize, usize), ReconciliationPlan>,

    /// Plans for foot cells, indexed by (row_index, cell_index)
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub foot_cell_plans: LinkedHashMap<(usize, usize), ReconciliationPlan>,
}
```

Add to `ReconciliationPlan`:

```rust
pub struct ReconciliationPlan {
    // ... existing fields ...

    /// Plans for Table cell content (Block::Table).
    /// Key: index into block_alignments where alignment is RecurseIntoContainer
    /// and the block is a Table.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub table_plans: LinkedHashMap<usize, TableReconciliationPlan>,
}
```

## Implementation Plan

### Phase 1: Type Definitions

1. Add `TableReconciliationPlan` struct to types.rs
2. Add `table_plans` field to `ReconciliationPlan`
3. Add serialization/deserialization tests

### Phase 2: Compute Phase

1. Add `Block::Table(_)` to `is_container_block()`
2. Add `compute_table_plan()` function:
   ```rust
   fn compute_table_plan<'a>(
       orig_table: &'a Table,
       exec_table: &Table,
       cache: &mut HashCache<'a>,
   ) -> TableReconciliationPlan {
       // For each cell position that exists in both tables,
       // compute a ReconciliationPlan for the cell's content
   }
   ```
3. Add Table case to `compute_container_plan()`:
   ```rust
   (Block::Table(orig), Block::Table(exec)) => {
       let table_plan = compute_table_plan(orig, exec, cache);
       // Store in table_plans HashMap
   }
   ```
4. Update caller in main compute function to handle table_plans

### Phase 3: Apply Phase

1. Add Table case to `apply_block_container_reconciliation()`:
   ```rust
   (Block::Table(mut orig), Block::Table(exec)) => {
       // Use exec's structural fields (attr, colspec, etc.)
       // Preserve orig's source_info
       // Recursively reconcile matching cells using table_plan
   }
   ```
2. Add helper functions:
   - `apply_table_head_reconciliation()`
   - `apply_table_body_reconciliation()`
   - `apply_table_foot_reconciliation()`
   - `apply_row_reconciliation()`
   - `apply_cell_reconciliation()`

### Phase 4: Testing

1. Add unit tests for `compute_table_plan()`
2. Add unit tests for Table apply reconciliation
3. Update property test generators (already done)
4. Run full test suite

## Reconciliation Semantics

For each table element, the reconciliation follows this pattern:

| Field | Source | Rationale |
|-------|--------|-----------|
| `attr` | exec | Structural (may have changed) |
| `source_info` | orig (if exists) | Preserve source location |
| `attr_source` | exec | Structural metadata |
| `colspec` | exec | Structural |
| `caption` | exec (but reconcile caption.long) | Mostly structural |
| `head/bodies/foot structure` | exec | Structural |
| `cell.content` | Recursively reconciled | Main value of reconciliation |

## Edge Cases

1. **Table structure changed** (different number of rows/columns):
   - Use exec's structure
   - Only reconcile cells that exist in both tables at same position

2. **Row/column spans**:
   - Match by nominal position, not spanned positions
   - If spans change, cell may not match (falls back to exec)

3. **Empty tables**:
   - If either table has no cells, no cell reconciliation needed

4. **Caption changes**:
   - caption.short always uses exec
   - caption.long is reconciled if both exist

## Success Criteria

- [x] `TableReconciliationPlan` type defined and tested
- [x] `compute_table_plan()` correctly generates plans for matching cells
- [x] `apply_table_block_reconciliation()` handles Table correctly
- [x] Table cells with matching content preserve source_info
- [x] All existing property tests pass (187 tests)
- [x] Table generators already in place from prior work

## Estimated Complexity

This is a moderate-complexity change:
- ~100-150 lines for types
- ~100-150 lines for compute
- ~150-200 lines for apply
- Similar pattern to existing list reconciliation

The main complexity is the multi-level nesting (table → section → rows → cells), but each level is straightforward.
