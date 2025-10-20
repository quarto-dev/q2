# Phase 3: SourceInfo Migration Guide

## Overview
Migrate all 42 AST structs from `pandoc::location::SourceInfo` to `quarto_source_map::SourceInfo` using gradual dual-field approach.

## Current State
- **Phase 1**: ✅ Complete (quarto-yaml integration)
- **Phase 2**: ✅ Complete (infrastructure, proof-of-concept)
- **Phase 3**: 🔄 In Progress (systematic migration)

## Struct Inventory
Total: 42 structs with source_info fields
- Inline types: 20 structs (inline.rs)
- Block types: 21 structs (block.rs)  
- Table types: 1 struct (table.rs)

Already migrated: 2 (Str, HorizontalRule from proof-of-concept)
Remaining: 40 structs

## Migration Steps

### Step 1: Add source_info_qsm field to struct definitions
**Goal**: Add `pub source_info_qsm: Option<quarto_source_map::SourceInfo>` to all 42 structs

**Script**: Use Python regex to add field after `source_info` field in struct definitions

**Files**:
- `src/pandoc/inline.rs` - 20 structs
- `src/pandoc/block.rs` - 21 structs
- `src/pandoc/table.rs` - 1 struct

**Validation**: 
```bash
grep -c "source_info_qsm" crates/quarto-markdown-pandoc/src/pandoc/{inline,block,table}.rs
# Should show: 20, 21, 1
```

### Step 2: Fix struct construction sites (manual per-file approach)
**Goal**: Add `source_info_qsm: None,` to all struct initializers

**Strategy**: Work file-by-file, starting with most critical:

**Priority 1 - Core parsing (treesitter_utils/):**
1. `paragraph.rs` - Paragraph, Plain
2. `atx_heading.rs` - Header
3. `setext_heading.rs` - Header  
4. `thematic_break.rs` - HorizontalRule (already done)
5. `code_span.rs` - Code
6. `fenced_code_block.rs` - CodeBlock
7. `indented_code_block.rs` - CodeBlock
8. `block_quote.rs` - BlockQuote
9. `quoted_span.rs` - Quoted
10. `inline_link.rs` - Link
11. `image.rs` - Image, Link
12. `citation.rs` - Cite
13. `note_definition_para.rs` - NoteDefinitionPara
14. `note_definition_fenced_block.rs` - NoteDefinitionFencedBlock
15. `fenced_div_block.rs` - Div
16. `editorial_marks.rs` - Insert, Delete, Highlight, EditComment
17. `latex_span.rs` - Math, RawInline
18. `caption.rs` - CaptionBlock
19. `pipe_table.rs` - Table
20. `document.rs` - various

**Priority 2 - Filters and utilities:**
21. `filters.rs` - All inline/block traversal
22. `pandoc/treesitter.rs` - Str, Space, LineBreak, SoftBreak constructions
23. `pandoc/meta.rs` - MetaBlock constructions

**Priority 3 - Readers/Writers:**
24. `readers/json.rs` - Deserialization
25. `writers/json.rs` - Serialization (shouldn't need changes)

**Pattern to find**:
```rust
// FIND THIS:
SomeStruct {
    field: value,
    source_info: some_value,
}

// CHANGE TO:
SomeStruct {
    field: value,
    source_info: some_value,
    source_info_qsm: None,
}
```

**Common patterns**:
- `Inline::Str(Str { text, source_info })`
- `Block::Paragraph(Paragraph { content, source_info })`
- `node_source_info(node)` or `node_source_info_with_context(node, ctx)`

**Validation per file**:
```bash
cargo check --package quarto-markdown-pandoc 2>&1 | grep "missing field"
```

### Step 3: Run tests
**Goal**: Ensure all tests pass with dual-field approach

```bash
cargo test --package quarto-markdown-pandoc
```

### Step 4: Populate source_info_qsm fields properly (future)
**Goal**: Use `node_to_source_info_with_context()` to populate actual values

**Pattern**:
```rust
// Change from:
SomeStruct {
    source_info: node_source_info_with_context(node, ctx),
    source_info_qsm: None,
}

// To:
SomeStruct {
    source_info: node_source_info_with_context(node, ctx),
    source_info_qsm: Some(crate::pandoc::source_map_compat::node_to_source_info_with_context(node, ctx)),
}
```

This step can be done incrementally after Step 2 is complete.

### Step 5: Final switchover (after everything works)
**Goal**: Remove old pandoc::location types

1. Remove `source_info: SourceInfo` fields
2. Rename `source_info_qsm` → `source_info`  
3. Change type from `Option<quarto_source_map::SourceInfo>` → `quarto_source_map::SourceInfo`
4. Remove `None` initializers
5. Delete `src/pandoc/location.rs`
6. Delete `src/pandoc/source_map_compat.rs`
7. Update all imports

## Beads Task Breakdown
- k-63: Parent epic (Phase 3 migration)
- k-64: ✅ Audit complete
- k-65: ✅ Add fields to Inline types
- k-66: ✅ Add fields to Block types
- k-67: 🔄 Fix all construction sites (current)
- k-68: Test dual-field approach
- k-69: Final switchover
- k-70: Remove old module
- k-71: Final validation

## Notes
- Work on branch for granular commits
- Commit after each file or small group of files
- Use `cargo check` frequently
- Tests must pass at each stage
