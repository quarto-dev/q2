# Location Information Self-Health Test Plan

**Date**: 2025-10-22
**Issue**: k-129
**Goal**: Add comprehensive self-health tests for location information that can run on any well-parsed .qmd file in the test suite

## Background

The parser needs to maintain accurate location information (byte offsets, row, column) for every AST node. A recent bug (manifested as parse error at EOF having start.row=341 but end.row=22) revealed that conversions between (row, col) and byte offsets can fail, especially at edge cases like EOF.

Instead of writing specific tests for specific files, we want **property-based tests** that verify invariants that MUST hold for ALL well-formed parsed documents.

## Research Summary

### How Location Information Works

1. **Storage**: Every Block and Inline AST node has a `source_info: SourceInfo` field
2. **SourceInfo contains**:
   - `Range` with start/end `Location` (offset, row, column)
   - `SourceMapping` (Original, Substring, Concat, Transformed)
3. **Conversion functions** (in `crates/quarto-source-map/src/utils.rs`):
   - `offset_to_location(source, offset) -> Location`
   - `line_col_to_offset(source, row, col) -> offset`
   - These should be proper inverses

### JSON Representation

- `sourceInfoPool`: Deduplicated array of location entries
- AST nodes reference pool via `s` field (source pool index)
- Pool entry format: `{"r": [start_off, start_row, start_col, end_off, end_row, end_col], "t": type, "d": data}`

## Invariant Properties to Test

### 1. Well-Formed Ranges
Every Range should have:
- `start.offset <= end.offset`
- `start.row <= end.row`
- If `start.row == end.row`, then `start.column <= end.column`

### 2. Offset/Row/Column Consistency
For every Location in the document:
- `offset_to_location(source, loc.offset)` should return `(loc.row, loc.column)`
- `line_col_to_offset(source, loc.row, loc.column)` should return `loc.offset`
- These conversions should be proper inverses

### 3. Bounds Checking
- All offsets should be `<= source.len()`
- All row numbers should be `<= number_of_lines`
- Column numbers should be valid for their respective rows

### 4. Nesting Consistency
If AST node B is a child of node A:
- `A.range.start.offset <= B.range.start.offset`
- `B.range.end.offset <= A.range.end.offset`

### 5. Sequential Consistency (Siblings)
If nodes A and B are siblings where A appears before B in the content array:
- `A.range.end.offset <= B.range.start.offset` (no overlap)

Note: SoftBreak and other synthetic nodes might have special handling

### 6. SourceMapping Validity
- For `Original`: file_id should be valid in ASTContext.filenames
- For `Substring`: parent should exist, offset within parent's range
- For `Concat`: pieces should be non-empty and sequential
- For `Transformed`: parent should exist

## Implementation Plan

### Phase 1: Infrastructure Setup
1. Create test file: `tests/test_location_health.rs`
2. Add helper functions to extract all SourceInfo from a Document
3. Add helper to recursively walk Block and Inline AST nodes
4. Create validator struct that accumulates violations

### Phase 2: Core Property Tests
1. Implement well-formed range checks
2. Implement offset/row/column consistency checks using existing utils
3. Implement bounds checking
4. Add clear error messages that show:
   - Which file failed
   - Which node failed (with context)
   - What property was violated
   - Actual vs expected values

### Phase 3: Structural Tests
1. Implement nesting consistency checks (parent-child relationships)
2. Implement sequential consistency checks (sibling relationships)
3. Handle special cases (SoftBreak, synthetic nodes)

### Phase 4: SourceMapping Tests
1. Validate Original mappings
2. Validate Substring mappings
3. Validate Concat mappings
4. Validate Transformed mappings

### Phase 5: Integration
1. Create test that runs on ALL .qmd files in `tests/smoke/`
2. Create test that runs on selected larger files
3. Add edge case files:
   - Empty file
   - File without trailing newline
   - Single line file
   - Multi-line file
   - File with UTF-8 multi-byte characters
4. Document which properties are checked and why

## Test Structure

```rust
#[test]
fn test_location_health_on_smoke_tests() {
    let smoke_dir = Path::new("tests/smoke");
    for entry in fs::read_dir(smoke_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension() == Some(OsStr::new("qmd")) {
            let source = fs::read_to_string(&path).unwrap();
            let doc = parse_qmd(&source);

            let violations = validate_location_properties(&doc, &source);

            if !violations.is_empty() {
                panic!("Location health violations in {:?}:\n{}",
                       path, violations.join("\n"));
            }
        }
    }
}
```

## Specific Bug Coverage

The original bug involved:
- `calculate_byte_offset` returning out-of-bounds offset (10188) for 10188-byte file
- This caused `offset_to_location` to return wrong row values
- Parse error at EOF had inconsistent start.row vs end.row

This will be caught by:
1. **Bounds checking**: offset > source.len() violation
2. **Consistency checking**: offset_to_location roundtrip failure
3. **Well-formed ranges**: start.row != end.row when they should match

## Edge Cases to Test

1. **EOF positions**: Last byte, one-past-last byte
2. **Empty files**: len() == 0
3. **Trailing newline**: File ends with \n vs no trailing newline
4. **First/last positions**: offset 0, offset len()
5. **Newline positions**: Right before/after \n characters
6. **Multi-byte UTF-8**: Characters that take >1 byte

## Success Criteria

1. Tests pass on all existing smoke test files
2. Tests catch the original bug if reintroduced
3. Tests can run on arbitrary .qmd files without modification
4. Clear, actionable error messages when violations occur
5. Fast enough to run on every test execution (<1s for all smoke tests)
6. Well-documented property definitions

## Future Extensions

1. Add property-based testing with quickcheck/proptest
2. Generate random valid documents and verify properties
3. Generate random mutations and verify they're caught
4. Performance testing on large documents
5. Concurrent testing with multiple files

## Implementation Notes

- Use existing `quarto_source_map::utils` functions (don't duplicate)
- Consider whether to fail fast (first violation) or accumulate all violations
- Add `#[cfg(test)]` helpers that are reusable
- Consider extracting to a separate validation module if it grows large
- Document assumptions about what constitutes "well-formed" input
