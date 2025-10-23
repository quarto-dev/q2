# Location Health Test Findings

**Date**: 2025-10-22
**Context**: k-129 - Add self-health tests for location information

## Summary

Successfully implemented Phase 1 and Phase 2 of location health tests. The tests FOUND REAL BUGS in existing smoke test files.

## What Was Implemented

### Phase 1: Infrastructure (k-130)
- Created `tests/test_location_health.rs`
- Implemented AST walker to recursively extract all SourceInfo from Pandoc documents
- Handles all Block types (Paragraph, Header, Table, Figure, etc.)
- Handles all Inline types (Str, Emph, Strong, Link, Note, etc.)
- Created `LocationHealthViolation` type for clear error reporting

### Phase 2: Core Property Validators (k-131)
Implemented three categories of validators:

1. **Well-formed Range Checks**
   - `start.offset <= end.offset`
   - `start.row <= end.row`
   - If same row: `start.column <= end.column`

2. **Offset/Row/Column Consistency**
   - `offset_to_location(offset)` → should give stored row/col
   - `line_col_to_offset(row, col)` → should give stored offset
   - These conversions should be proper inverses

3. **Bounds Checking**
   - All offsets `<= source.len()`
   - Row numbers valid for number of lines
   - Handles edge cases: empty files, EOF, trailing newlines

## Bugs Found

Running on all 30 smoke test files found **24 violations across 6 files**:

### Pattern: End Locations with (row:0, col:0)

Multiple files have SourceInfo where the **end** location has:
- Correct offset (e.g., 5, 7, 8, 55)
- But row=0, col=0 (incorrect!)

Examples:
- `014.qmd`: SourceInfo #1 end has offset=5 but row:0,col:0 (should be row:0,col:5)
- `002.qmd`: SourceInfo #1 end has offset=7 but row:0,col:0 (should be row:0,col:7)
- `007.qmd`: SourceInfo #21 end has offset=8 but row:0,col:0 (should be row:0,col:8)
- `018.qmd`: SourceInfo #3 end has offset=55 but row:0,col:0 (should be row:2,col:46)

### Affected Files
1. `014.qmd` - 2 violations (1 SourceInfo)
2. `002.qmd` - 4 violations (2 SourceInfos)
3. `007.qmd` - 8 violations (4 SourceInfos)
4. `021.qmd` - 2 violations (1 SourceInfo)
5. `018.qmd` - 2 violations (1 SourceInfo)
6. `table.qmd` - 6 violations (3 SourceInfos)

### Root Cause Hypothesis

The pattern suggests:
- When creating Ranges, the **start** Location is calculated correctly
- But the **end** Location sometimes gets default-initialized to (offset: X, row: 0, column: 0)
- Instead of properly calculating row/column from the offset

This likely happens in one of these places:
- Tree-sitter node-to-SourceInfo conversion (`node_to_source_info`)
- Range combination/merging logic
- Synthetic node creation (SoftBreak, Space, etc.)

## Test Coverage

### Edge Cases Tested ✓
- Empty files
- Files without trailing newline
- Files with trailing newline
- Simple single-line documents
- Multi-line documents
- Nested structures (emphasis, strong, etc.)
- All 30 smoke test files

### Properties Verified
- ✓ Well-formed ranges (start <= end)
- ✓ Offset/row/column consistency
- ✓ Bounds checking
- ⏱ Nesting consistency (Phase 3 - not yet implemented)
- ⏱ Sequential consistency (Phase 3 - not yet implemented)
- ⏱ SourceMapping validity (Phase 4 - not yet implemented)

## Next Steps

1. **Fix the bugs** - Investigate why end locations have row=0,col=0
   - Look at `node_to_source_info` in tree-sitter conversion
   - Look at Range creation/combination code
   - Check if this affects only certain node types

2. **Phase 3: Structural Tests** (k-132)
   - Nesting consistency (child ranges inside parent ranges)
   - Sequential consistency (sibling nodes don't overlap)

3. **Phase 4: SourceMapping Validation** (k-133)
   - Validate Original mappings
   - Validate Substring mappings
   - Validate Concat mappings
   - Validate Transformed mappings

4. **Phase 5: Integration** (k-134)
   - Run on ALL smoke tests (done!)
   - Add more edge case files
   - Document findings

## Test Status

**Currently**: Test fails because bugs were found (this is good!)

To make the test pass, we need to either:
1. Fix the underlying bugs (recommended)
2. Mark the test as `#[ignore]` until bugs are fixed
3. Change assertion to just report violations without failing

## Code Location

- Test file: `/Users/cscheid/repos/github/cscheid/kyoto/crates/quarto-markdown-pandoc/tests/test_location_health.rs`
- 790+ lines of comprehensive validation logic
- 9 tests total (8 passing, 1 finding bugs)

## Success Criteria Met

✓ Tests can run on arbitrary .qmd files without modification
✓ Clear, actionable error messages when violations occur
✓ Fast execution (<1s for all smoke tests)
✓ Found real bugs in existing code
✓ Well-documented property definitions

## Recommendation

**DO NOT CLOSE k-129 yet**. The infrastructure is complete, but the bugs found should be investigated and fixed as part of this task. The test suite is working perfectly - it caught real problems!

Create a follow-up issue to fix the specific bug: "End locations incorrectly have row=0,col=0 instead of computed values"
