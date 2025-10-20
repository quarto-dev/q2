# k-69 Session 4 Progress - Source Map Migration Final Switchover

**Date:** 2025-10-20
**Task:** k-69 - Replace source_info with source_info_qsm throughout
**Status:** Part 1 Complete (struct definitions), Part 2-4 remaining

## Summary

Successfully completed the struct field migration phase of k-69. All 42 AST structs now use `quarto_source_map::SourceInfo` instead of dual fields. Created subtasks for remaining work.

## Completed Work

### 1. Struct Definition Changes (✅ Committed: 3fe7a7e)

- **Removed** old `pub source_info: SourceInfo` (pandoc::location) from all 42 structs
- **Renamed** `source_info_qsm` → `source_info` in all struct definitions
- **Changed type** from `Option<quarto_source_map::SourceInfo>` → `quarto_source_map::SourceInfo`

### 2. Updated Struct Constructions (35+ files)

Pattern replacements:
- `source_info_qsm: Some(x)` → `source_info: x`
- Removed all `source_info_qsm: None,` lines
- Special cases use `source_map_compat::old_to_new_source_info()`

Key files updated:
- All readers/ files (qmd.rs, json.rs)
- All treesitter_utils/ files (35+ helpers)
- Test files (test_json_roundtrip.rs, test_meta.rs, etc.)
- filters.rs, meta.rs, treesitter.rs

### 3. Fixed impl_source_location! Macro

The old `SourceLocation` trait expected direct field access to `filename_index` and `range`, but the new `quarto_source_map::SourceInfo` has a recursive structure.

**Solution:** Created helper functions in `location.rs`:

```rust
pub fn extract_filename_index(info: &quarto_source_map::SourceInfo) -> Option<usize> {
    // Walks SourceMapping tree to find Original { file_id }
    match &info.mapping {
        SourceMapping::Original { file_id } => Some(file_id.0),
        SourceMapping::Substring { parent, .. } => extract_filename_index(parent),
        // ... handles all variants
    }
}

pub fn convert_range(range: &quarto_source_map::Range) -> Range {
    // Converts between Range types
}
```

Updated macro to use these helpers instead of direct field access.

## Remaining Work (Created as Beads Tasks)

### k-79: Update function signatures (~126 type errors)

**Problem:** Helper functions still expect old `location::SourceInfo` type:
- `make_span_inline(source_info: SourceInfo)` in inline.rs
- Similar functions throughout codebase

**Solution:** Update signatures to use new type, convert at boundaries

### k-80: Update imports

**Problem:** Many files import old type:
```rust
use crate::pandoc::location::SourceInfo;  // old type
```

**Solution:** Remove/update imports, clean up unused imports

### k-81: Verify compilation and tests

- Run `cargo check` (should be 0 errors)
- Run `cargo test` (all should pass)
- Fix any discovered issues

## Key Insights

1. **Macro complexity:** The `impl_source_location!` macro was a hidden dependency that required careful handling. The new SourceInfo's recursive structure needed special extraction logic.

2. **Type system safety:** Rust's type system caught all the mismatches - no silent bugs. The 126 remaining errors are all explicit type mismatches that need fixing.

3. **Incremental commits:** Breaking this into parts makes it easier to review and rollback if needed.

## Files Modified: 37

```
src/filters.rs
src/pandoc/block.rs
src/pandoc/inline.rs
src/pandoc/location.rs
src/pandoc/meta.rs
src/pandoc/shortcode.rs
src/pandoc/table.rs
src/pandoc/treesitter.rs
src/pandoc/treesitter_utils/* (29 files)
src/readers/json.rs
src/readers/qmd.rs
src/writers/native.rs
tests/* (3 test files)
```

## Next Session

Start with k-79 (update function signatures). Key files to tackle:
- `src/pandoc/inline.rs` - `make_span_inline()` and related
- Any other functions with `SourceInfo` parameters
- Update callers to pass new type

## Technical Notes

- Old `SourceInfo` had: `filename_index: Option<usize>`, `range: Range`
- New `SourceInfo` has: `range: Range`, `mapping: SourceMapping` (recursive)
- The `SourceMapping` enum encodes transformation history
- JSON serialization still works (derives handle it)
- The `SourceLocation` trait is used by JSON writer (writers/json.rs)

## Session Update - Continuing k-79

**Current Status:** 63 errors remaining (down from 126)

**Commits Made:**
1. `3fe7a7e` - Part 1: Struct definitions complete (37 files)
2. `ce0354e` - Part 1.5: inline.rs function signatures (126→116 errors)
3. `8a766d9` - Part 1.6: readers/json.rs (116→79 errors)
4. `2be14c9` - Part 1.7: postprocess.rs (79→63 errors)

**Strategy Working Well:**
- Remove old `location::SourceInfo` imports first
- Add helper functions for conversion where needed
- Use sed for bulk replacements of simple patterns
- Commit frequently to track progress

**Next:** Fix remaining 63 errors across various files
