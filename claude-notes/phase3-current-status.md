# Phase 3 Migration - Current Status

**Last Updated**: 2025-10-20

## Completed Work
- ✅ k-62: Fixed YAML tag preservation bug
- ✅ k-64: Completed audit (42 structs identified)
- ✅ k-65: Added source_info_qsm to all 20 Inline types
- ✅ k-66: Added source_info_qsm to all 21 Block + 1 Table types
- ✅ Created comprehensive migration guide
- ✅ Committed Step 1: All struct definitions now have source_info_qsm field

## Current State
**Branch**: `kyoto-source-map-migration`
**Last Commit**: e03a520 "Phase 3 Step 2 (partial): Start fixing struct construction sites"

**Compilation Status**: 141 missing field errors
**Files Fixed**: 1 (paragraph.rs)
**Files Remaining**: ~20

## Next Steps - Sub-tasks Created

### Step 2: Fix Struct Construction Sites
Work through these in order:

1. **k-72**: Fix filters.rs (Priority: HIGH)
   - ~20-30 struct constructions
   - Pattern: `source_info: self.source_info`

2. **k-73**: Fix pandoc/treesitter.rs  
   - ~10-15 Str, Space, LineBreak, SoftBreak constructions
   - Pattern: `source_info: node_source_info(node)`

3. **k-74**: Fix treesitter_utils/ Priority 1 files
   - 10 files: atx_heading, setext_heading, code_span, fenced_code_block, etc.
   - ~30-40 constructions total

4. **k-75**: Fix treesitter_utils/ Priority 2 files
   - 8 files: note definitions, editorial marks, etc.
   - ~30-40 constructions total

5. **k-76**: Fix pandoc/meta.rs and other pandoc/ files
   - MetaBlock, helper functions
   - ~20-30 constructions

6. **k-77**: Fix readers/ and writers/
   - JSON deserialization
   - ~20-30 constructions
   - **After this**: cargo check should show 0 errors

### After Step 2 Completes
- **k-68**: Run tests (all should pass)
- **k-69**: Final switchover (remove old fields, rename)
- **k-70**: Remove pandoc::location module
- **k-71**: Final validation

## For Next Claude Session

**Start with**:
```bash
cd /Users/cscheid/repos/github/cscheid/kyoto
git checkout kyoto-source-map-migration
bd ready --json  # Check available work
```

**Begin work on k-72** (filters.rs):
```bash
bd update k-72 --status in_progress
```

**Pattern to apply**:
```rust
// BEFORE:
SomeStruct {
    field: value,
    source_info: some_value,
}

// AFTER:
SomeStruct {
    field: value,
    source_info: some_value,
    source_info_qsm: None,
}
```

**Validation**:
```bash
cargo check --package quarto-markdown-pandoc 2>&1 | grep "error\[E0063\]" | wc -l
# Watch this number decrease
```

## Files Already Modified
- ✅ src/pandoc/inline.rs (struct definitions)
- ✅ src/pandoc/block.rs (struct definitions)
- ✅ src/pandoc/table.rs (struct definitions)
- ✅ src/pandoc/treesitter_utils/paragraph.rs (1 construction fixed)

## Key Files
- **Migration Guide**: `claude-notes/phase3-sourceinfo-migration-guide.md`
- **Beads Tasks**: k-63 (epic), k-67 (current), k-72-k-77 (sub-tasks)
- **Branch**: `kyoto-source-map-migration`
