# Structural Components Design for AnnotatedParse

**Date**: 2025-10-24
**Context**: During k-163 implementation (annotated Pandoc AST), discovered that flattening complex nested structures loses critical navigation information.

## Executive Summary

The current flattening approach for complex nested structures (lists, tables) loses critical structural information needed for navigation. To support proper navigation, we need to introduce **structural AnnotatedParse nodes** that represent intermediate levels of the hierarchy (rows, cells, list items, etc.). However, this requires source location information that Rust currently doesn't provide for these structural elements.

## Key Constraint: Contiguity Requirement

For structural AnnotatedParse nodes to work properly, they need valid `start`, `end`, and `source` fields. As noted during discussion:

> "For those to work well, we need the structural parts of 'components' to be, more or less, contiguous in the QMD objects."

A table row like `| A | B |` IS contiguous in source, so conceptually we should be able to create a structural node for it. However, we need Rust to provide the source location information for these structural boundaries.

## Current Source Information from Rust

### What We Have (✓)
- Block nodes: `s: number` (source ID)
- Inline nodes: `s: number`
- Attributes: `attrS: AttrSourceInfo` (source IDs for id/classes/kvs)
- Link/Image targets: `targetS: TargetSourceInfo`
- Citation IDs: `citationIdS: number | null`

### What We're Missing (✗)
- **Row**: no `s: number` (only has `attrS` for attributes)
- **Cell**: no `s: number` (only has `attrS` for attributes)
- **TableHead/Body/Foot**: no `s: number` (only has `attrS`)
- **Caption**: no `s: number` for the caption structure itself
- **List items**: no intermediate type or source info (items are just tuples/arrays)
- **Definition list items**: no intermediate type or source info

## Why We Can't Compute Spans in TypeScript

You might think: "Just compute span from first child to last child." Problems:

1. **Empty structures**: Empty cell `| |` has no children - no span to compute
2. **MappedString creation**: `SourceInfoReconstructor.toMappedString()` requires a source ID from the pool. We can't create arbitrary MappedStrings for computed ranges without:
   - Having a source ID from Rust
   - Modifying the source mapping infrastructure to support "synthetic" spans
3. **Accuracy**: Computed spans might miss whitespace, delimiters, or other structural syntax

## Types That Need Structural Source Info

### Tables (High Priority)
```typescript
// Need s: number added to:
Annotated_Row          // Row boundaries (e.g., | A | B |\n)
Annotated_Cell         // Cell boundaries
Annotated_TableHead    // <thead> section
Annotated_TableBody    // <tbody> section
Annotated_TableFoot    // <tfoot> section
Annotated_Caption      // Caption as a whole (not just its contents)
```

### Lists (Medium Priority)
Need new intermediate types in Rust:
```rust
// Would need something like:
struct ListItem {
    content: Vec<Block>,
    source_info: SourceInfo,
}

struct DefinitionListItem {
    term: Vec<Inline>,
    definitions: Vec<Vec<Block>>,
    source_info: SourceInfo,
}

struct Definition {
    blocks: Vec<Block>,
    source_info: SourceInfo,
}
```

## Concrete Example: Why Tables Need This

Consider three tables that would produce identical flattened components:

### Case A: 2×2 table (4 cells, 1 para each)
```markdown
| A | B |
| C | D |
```

### Case B: 1×1 table (1 cell, 4 paras)
```markdown
| Para A
  Para B
  Para C
  Para D |
```

### Case C: 1×4 table (4 cells in one row)
```markdown
| A | B | C | D |
```

**All three flatten to**: `[paraA_AP, paraB_AP, paraC_AP, paraD_AP]`

Without structural nodes for Row and Cell, these are indistinguishable. A validator trying to check "all header cells have bold text" cannot identify which blocks belong to which cell.

## Caption Sub-structure Problem

Both Table and Figure have Caption, defined as:
```typescript
export interface Caption {
  shortCaption: Inline[] | null;
  longCaption: Block[];
}
```

Current `convertCaption()` helper (in block-converter.ts:284-300) just flattens everything:
```typescript
private convertCaption(caption: Annotated_Caption): AnnotatedParse[] {
  const components: AnnotatedParse[] = [];

  if (caption.shortCaption) {
    components.push(...caption.shortCaption.map(...));  // mixed together
  }
  components.push(...caption.longCaption.map(...));     // mixed together

  return components;
}
```

This loses the distinction between short and long caption. We need structural nodes with kinds like `'caption-short'` and `'caption-long'`.

But Caption itself doesn't have a source ID - we'd need Rust to provide one, OR we create synthetic nodes with computed/null source.

## Impact on Filters Infrastructure

**IMPORTANT**: Adding source_info fields to structural elements will affect the filter traversal system in `crates/quarto-markdown-pandoc/src/filters.rs`.

Questions to resolve:
1. Should Row/Cell/TableHead/etc be separately visitable in filters?
2. Does filter traversal need to be aware of these structural elements?
3. Will this break existing filter code?
4. Should filters be able to transform these structural elements?

See Phase 1 implementation plan for detailed analysis.

## Proposed Plan

### Phase 1: Rust Source Tracking for Structural Elements (BLOCKER)

**Beads Issue**: k-194 (to be created)

**Scope**:
1. Study filters.rs to understand impact on filter traversal
2. Add `source_info: SourceInfo` to table types:
   - `Row`
   - `Cell`
   - `TableHead`
   - `TableBody`
   - `TableFoot`
   - `Caption` (the structure itself, not just contents)

3. Update table parser to capture these ranges during parsing

4. Update JSON serializer to emit `s` field for these structures:
   ```json
   {
     "attr": ["", [], []],
     "cells": [...],
     "s": 42,      // NEW: source ID for the row
     "attrS": {...}
   }
   ```

5. Consider adding intermediate types for lists:
   - `ListItem` struct with `source_info`
   - `DefinitionListItem` struct
   - `Definition` struct

   Or at minimum, add source_info arrays parallel to the item arrays

6. Update filter infrastructure if needed to handle new structural elements

**Deliverables**:
- Updated Rust types with source_info fields
- Parser captures structural boundaries
- JSON output includes `s` for structural elements
- Filter system handles structural elements appropriately
- Tests verify source locations are accurate

**Dependencies**: None
**Blocks**: Phase 2 (TypeScript structural nodes)

---

### Phase 2: TypeScript Structural Nodes Implementation

**Beads Issue**: k-195 (to be created)

**Scope**:

#### 2.1: Update TypeScript Types
```typescript
export interface Annotated_Row {
  attr: Attr;
  cells: Annotated_Cell[];
  s: number;           // NEW
  attrS: AttrSourceInfo;
}

export interface Annotated_Cell {
  attr: Attr;
  alignment: Alignment;
  rowSpan: number;
  colSpan: number;
  content: Annotated_Block[];
  s: number;           // NEW
  attrS: AttrSourceInfo;
}

// Similar for TableHead, TableBody, TableFoot, Caption
```

#### 2.2: Define Synthetic Kinds
Create a types file or extend existing types:
```typescript
// Synthetic kinds for structural navigation
type StructuralKind =
  | 'table-head'
  | 'table-body'
  | 'table-foot'
  | 'table-row'
  | 'table-cell'
  | 'caption-short'
  | 'caption-long'
  | 'bullet-list-item'
  | 'ordered-list-item'
  | 'definition-list-item'
  | 'definition';
```

#### 2.3: Implement Table Converter with Structural Nodes
```typescript
case 'Table':
  return {
    result: block.c as unknown as JSONValue,
    kind: 'Table',
    source,
    components: [
      ...this.convertAttr(block.c[0], block.attrS),
      this.convertCaptionStructured(block.c[1]),     // NEW
      this.convertTableHead(block.c[3]),              // NEW
      ...block.c[4].map(tb => this.convertTableBody(tb)), // NEW
      this.convertTableFoot(block.c[5])               // NEW
    ],
    start,
    end
  };

private convertTableHead(thead: Annotated_TableHead): AnnotatedParse {
  const source = this.sourceReconstructor.toMappedString(thead.s);
  const [start, end] = this.sourceReconstructor.getOffsets(thead.s);

  return {
    result: thead as unknown as JSONValue,
    kind: 'table-head',  // SYNTHETIC
    source,
    components: [
      ...this.convertAttr(thead.attr, thead.attrS),
      ...thead.rows.map(row => this.convertRow(row))
    ],
    start,
    end
  };
}

private convertRow(row: Annotated_Row): AnnotatedParse {
  const source = this.sourceReconstructor.toMappedString(row.s);
  const [start, end] = this.sourceReconstructor.getOffsets(row.s);

  return {
    result: row as unknown as JSONValue,
    kind: 'table-row',  // SYNTHETIC
    source,
    components: [
      ...this.convertAttr(row.attr, row.attrS),
      ...row.cells.map(cell => this.convertCell(cell))
    ],
    start,
    end
  };
}

private convertCell(cell: Annotated_Cell): AnnotatedParse {
  const source = this.sourceReconstructor.toMappedString(cell.s);
  const [start, end] = this.sourceReconstructor.getOffsets(cell.s);

  return {
    result: cell as unknown as JSONValue,
    kind: 'table-cell',  // SYNTHETIC
    source,
    components: [
      ...this.convertAttr(cell.attr, cell.attrS),
      ...cell.content.map(block => this.convertBlock(block))
    ],
    start,
    end
  };
}
```

#### 2.4: Update List Converters Similarly
If Rust adds source info for list items, implement:
- `convertBulletListItem()`
- `convertOrderedListItem()`
- `convertDefinitionListItem()`
- `convertDefinition()`

#### 2.5: Update Tests
- Test that structural nodes preserve navigability
- Test that validators can distinguish cell boundaries
- Test caption short vs long distinction

**Deliverables**:
- Updated TypeScript types with `s` fields
- Synthetic kind definitions
- Table converter with full structural preservation
- List converters with structural nodes (if Rust support available)
- Comprehensive tests

**Dependencies**: Phase 1 complete
**Blocks**: Phase 3 (helper APIs can use structural nodes)

---

### Phase 3: Helper APIs for Navigation (k-193 already exists)

**Update existing k-193** to leverage structural nodes:

Instead of parsing `result` field:
```typescript
// OLD approach (parsing result field):
function getTableRows(tableAP: AnnotatedParse): AnnotatedParse[][] {
  const tableJSON = tableAP.result as any;
  // Parse JSON and correlate with flat components... painful
}
```

Use structural components:
```typescript
// NEW approach (using structural nodes):
function getTableRows(tableAP: AnnotatedParse): AnnotatedParse[] {
  // Filter components by kind
  return tableAP.components.filter(c => c.kind === 'table-row');
}

function getRowCells(rowAP: AnnotatedParse): AnnotatedParse[] {
  return rowAP.components.filter(c => c.kind === 'table-cell');
}

function getCellBlocks(cellAP: AnnotatedParse): AnnotatedParse[] {
  return cellAP.components.filter(c =>
    c.kind !== 'attr-id' &&
    c.kind !== 'attr-class' &&
    c.kind !== 'attr-key' &&
    c.kind !== 'attr-value'
  );
}
```

Much simpler and more reliable!

---

## Alternative: Interim Solution Without Rust Changes

If Rust changes are blocked or deferred, we could:

### Option A: Computed Spans (Fragile)
- Compute spans from first to last child
- Use first child's source ID to create MappedString (semantically wrong but functional)
- Handle empty structures with sentinel values
- Mark with TODO comments that proper source IDs needed

**Pros**: Unblocks TypeScript work
**Cons**: Semantically incorrect, breaks with empty structures, technical debt

### Option B: Null Source for Structural Nodes
- Create structural nodes with:
  - `source: nullMappedString` or similar
  - `start: 0, end: 0` or compute from children
  - `kind: 'table-row'` etc.
- Document that these are "logical" nodes without direct source mapping

**Pros**: Preserves navigation, honest about limitations
**Cons**: Loss of source tracking for structural validation

### Option C: Defer Tables Until Rust Ready
- Keep Table throwing error in k-190
- Complete Phases 1-2 before implementing Table
- Most principled approach

**Pros**: No technical debt, correct implementation
**Cons**: Table support delayed

---

## Recommendation

**Pursue Phase 1 (Rust) and Phase 2 (TypeScript) in sequence**. This is the correct long-term solution.

For immediate k-190:
- Create the two beads issues (Rust + TypeScript)
- Keep Table throwing error until Phase 1 complete
- This is the most principled approach

The structural nodes pattern will also improve lists (k-193) and make the entire AnnotatedParse API more powerful for validators.

## Related Issues

- k-190: Phase 3c: Add Table support to BlockConverter (blocked by this work)
- k-193: Create helper APIs for navigating flattened list structures (will be updated to use structural nodes)
- k-194: Phase 1 - Add source tracking for structural elements in Rust (to be created)
- k-195: Phase 2 - Implement structural AnnotatedParse nodes in TypeScript (to be created)
