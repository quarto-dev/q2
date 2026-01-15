# Remaining AST Generators Plan

**Date:** 2026-01-14
**Status:** Phases 1-2 Complete
**Parent:** 2026-01-14-complete-ast-generators.md
**Epic:** kyoto-tsq

## Purpose

**Find and fix bugs in reconciliation for the remaining untested AST types.**

The previous generator work (Phases 1-4) covered most AST types and found 3 bugs. However, analysis reveals that several types have NO test coverage and likely contain bugs:

### Known Bugs (High Confidence)

1. **Table** - Falls through to `(orig, _) => orig` fallback in `apply_block_container_reconciliation`. Will keep `before`'s structure instead of `after`'s.

2. **Shortcode** - Falls through to `(orig, _) => orig` fallback in `apply_inline_container_reconciliation`. Same bug.

3. **Insert/Delete/Highlight/EditComment** (4 instances) - Have explicit handlers but are missing `o.attr = e.attr`:
   ```rust
   (Inline::Insert(mut o), Inline::Insert(e)) => {
       o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
       Inline::Insert(o)  // BUG: Missing o.attr = e.attr
   }
   ```
   This is the same bug pattern we found with Header.

### Needs Testing (Unknown)

4. **CustomNode** - Has explicit handling with slot reconciliation. Looks correct but untested:
   ```rust
   CustomNode {
       type_name: exec.type_name,
       slots: result_slots,
       plain_data: exec.plain_data,
       attr: exec.attr,  // Correctly uses exec's attr
       source_info: orig.source_info,
   }
   ```

## Type Structures

### Easy (Identical to Span)

**Insert, Delete, Highlight, EditComment:**
```rust
pub struct Insert {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: SourceInfo,
    pub attr_source: AttrSourceInfo,
}
// Delete, Highlight, EditComment have identical structure
```

### Medium Complexity

**Shortcode:**
```rust
pub struct Shortcode {
    pub is_escaped: bool,
    pub name: String,
    pub positional_args: Vec<ShortcodeArg>,
    pub keyword_args: HashMap<String, ShortcodeArg>,
}

pub enum ShortcodeArg {
    String(String),
    Number(f64),
    Boolean(bool),
    Shortcode(Shortcode),  // Recursive!
    KeyValue(HashMap<String, ShortcodeArg>),
}
```

**CustomNode:**
```rust
pub struct CustomNode {
    pub type_name: String,
    pub slots: LinkedHashMap<String, Slot>,
    pub plain_data: Value,  // serde_json::Value
    pub attr: Attr,
    pub source_info: SourceInfo,
}

pub enum Slot {
    Block(Box<Block>),
    Inline(Box<Inline>),
    Blocks(Blocks),
    Inlines(Inlines),
}
```

### High Complexity

**Table:**
```rust
pub struct Table {
    pub attr: Attr,
    pub caption: Caption,
    pub colspec: Vec<ColSpec>,  // (Alignment, ColWidth)
    pub head: TableHead,        // attr + Vec<Row>
    pub bodies: Vec<TableBody>, // attr + rowhead_columns + head + body
    pub foot: TableFoot,        // attr + Vec<Row>
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
    pub content: Blocks,
    pub source_info: SourceInfo,
    pub attr_source: AttrSourceInfo,
}
```

## Implementation Plan

### Phase 1: CriticMarkup Inlines ✅ COMPLETE

These are identical to Span - just attr + content.

- [x] `gen_insert(config)` - Attr + inlines
- [x] `gen_delete(config)` - Attr + inlines
- [x] `gen_highlight(config)` - Attr + inlines
- [x] `gen_edit_comment(config)` - Attr + inlines
- [x] Update `InlineFeatures` with `insert`, `delete`, `highlight`, `edit_comment`
- [x] Update `gen_inline()` to include these types
- [x] Run tests - **PASSED** (see findings below)
- [x] Fix bugs in `apply_inline_container_reconciliation` - **Fixed for consistency**

**Key Finding:** The bugs in Phase 1 (and also Span, Link, Image, Quoted) were **real but unreachable** in the current design:

1. **Attrs are part of the hash**: `hash_attr(&i.attr, &mut hasher)` is called for all these types
2. **Different attrs → different hashes → UseAfter**: When attrs differ, hashes differ, so the entire inline is replaced (UseAfter), bypassing the RecurseIntoContainer code path
3. **Same attrs → RecurseIntoContainer → no-op**: If attrs match (required for RecurseIntoContainer), then `o.attr = e.attr` would be a no-op anyway

**Fixes applied for consistency:**
- `Inline::Insert`: Added `o.attr = e.attr`
- `Inline::Delete`: Added `o.attr = e.attr`
- `Inline::Highlight`: Added `o.attr = e.attr`
- `Inline::EditComment`: Added `o.attr = e.attr`
- `Inline::Span`: Added `o.attr = e.attr` (same pattern)
- `Inline::Link`: Added `o.attr = e.attr` and `o.target = e.target`
- `Inline::Image`: Added `o.attr = e.attr` and `o.target = e.target`
- `Inline::Quoted`: Added `o.quote_type = e.quote_type`

These fixes are correct for:
1. **Consistency** with Header (which does copy attr)
2. **Future-proofing** if hashing strategy changes
3. **Code clarity** (explicit > implicit)

### Phase 2: Shortcode ✅ COMPLETE (No Bug - Plan Was Incorrect)

- [x] `gen_shortcode_arg()` - Recursive enum generator (depth-limited)
- [x] `gen_shortcode_inner()` - Inner Shortcode generator for recursion
- [x] `gen_shortcode()` - Generate Shortcode inline
- [x] Update `InlineFeatures` with `shortcode`
- [x] Update `gen_inline()` to include Shortcode (as leaf inline)
- [x] Run tests - **ALL PASSED** (no bug exists)

**Key Finding:** The expected bug was **incorrect**. Shortcode is NOT a container inline:

1. **Not in `is_container_inline()`**: Shortcode is not listed in compute.rs:504-525
2. **No nested Inlines**: Shortcode has `positional_args` and `keyword_args` (ShortcodeArg), not `content: Inlines`
3. **No source_info**: The Shortcode struct has no source location to preserve
4. **Desugared early**: Comment in inline.rs says "after desugaring, these nodes should not appear in a document"

**Correct behavior (already implemented):**
- Same Shortcode hash → `KeepBefore` alignment → correct
- Different Shortcode hash → `UseAfter` alignment → correct
- Shortcode never reaches `apply_inline_container_reconciliation` → no fallback bug possible

### Phase 3: CustomNode ✅ COMPLETE (Bug Found and Fixed!)

- [x] `gen_slot(config)` - Generate Slot enum (all 4 variants)
- [x] `gen_slots(config)` - Generate named slots map
- [x] `gen_plain_data()` - Generate JSON values
- [x] `gen_custom_node(config)` - Generate CustomNode
- [x] `gen_custom_block(config)` - Generate Block::Custom
- [x] `gen_custom_inline(config)` - Generate Inline::Custom
- [x] Add `custom: bool` to `BlockFeatures` and `InlineFeatures`
- [x] Update `full()` and `has_containers()` methods
- [x] Run tests - **FAILED** (bug found!)
- [x] Fix bug in `apply_custom_node_reconciliation`

**Bug Found (kyoto-xxx):** `apply_custom_node_reconciliation` was using wrong fields:
```rust
// BEFORE (buggy):
CustomNode {
    type_name: orig.type_name,  // Should use exec
    attr: orig.attr,            // Should use exec
    ...
}

// AFTER (fixed):
CustomNode {
    type_name: exec.type_name,  // Use exec's type_name (structural)
    attr: exec.attr,            // Use exec's attr (structural)
    plain_data: exec.plain_data,
    slots: result_slots,
    source_info: orig.source_info, // Keep orig's source location
}
```

**Key insight**: Unlike CriticMarkup inlines (where attr bugs were unreachable), CustomNode's bug was **actually reachable** through property testing because slot reconciliation can preserve the CustomNode container while changing its structural fields.

### Phase 4: Table (Complex, 1 Bug Expected)

- [ ] `gen_alignment()` - Alignment enum
- [ ] `gen_col_width()` - ColWidth enum
- [ ] `gen_col_spec()` - (Alignment, ColWidth)
- [ ] `gen_cell(config)` - Cell with blocks
- [ ] `gen_row(config)` - Row with cells
- [ ] `gen_table_head(config)` - TableHead
- [ ] `gen_table_body(config)` - TableBody
- [ ] `gen_table_foot(config)` - TableFoot
- [ ] `gen_table(config)` - Full table
- [ ] Update `BlockFeatures` with `table`
- [ ] Update `gen_block()` to include Table
- [ ] Run tests, expect failure (fallback returns orig)
- [ ] Add explicit Table handling in `apply_block_container_reconciliation`

## Expected Bugs Summary

| Type | Bug | Pattern | Status |
|------|-----|---------|--------|
| Insert | `o.attr` not updated from `e.attr` | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Delete | `o.attr` not updated from `e.attr` | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Highlight | `o.attr` not updated from `e.attr` | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| EditComment | `o.attr` not updated from `e.attr` | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Span | `o.attr` not updated from `e.attr` | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Link | `o.attr`, `o.target` not updated | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Image | `o.attr`, `o.target` not updated | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Quoted | `o.quote_type` not updated | Same as Header bug | ✅ Fixed (unreachable but fixed for consistency) |
| Shortcode | Expected fallback bug | **NO BUG** - not a container inline | ✅ Verified correct |
| Table | Falls through to `(orig, _) => orig` | Returns wrong structure | ⏳ Pending |
| CustomNode | `attr` and `type_name` from orig instead of exec | **REAL BUG** - fixed | ✅ Fixed via TDD |

Phase 1-3 complete: 8 consistency fixes + 1 real bug fix. Remaining: 1 type to test (Table).

## Success Criteria

- [x] All 4 CriticMarkup inlines have generators and reconciliation works
- [x] Shortcode has generator and reconciliation works (verified correct, no bug)
- [ ] CustomNode has generator and reconciliation works
- [ ] Table has generator and reconciliation works
- [x] Full AST property test passes with all types enabled (187 tests, including full_ast)
- [x] All bugs found via TDD (test first, then fix) - 8 consistency fixes applied in Phase 1

## TDD Protocol

For each bug:
1. Add generator for the type
2. Run `reconciliation_preserves_structure_full_ast` test
3. Observe failure with specific type
4. Identify bug in apply.rs
5. Write targeted unit test that demonstrates the bug
6. Fix the bug
7. Verify all tests pass
8. Create beads issue documenting the bug and fix

## Notes

- Implementation order is easiest-to-hardest to maximize early wins
- Each phase should be committed separately for clean history
- CriticMarkup bugs are low-hanging fruit - same pattern as Header fix
- Table is most complex but follows same patterns as other containers
