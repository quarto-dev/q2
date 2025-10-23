# SourceInfo Enum Refactor Plan

**Issue:** k-136
**Date:** 2025-10-22

## Problem

The current `SourceInfo` design has a confusing dual-structure:
- `SourceInfo` is a struct with `range: Range` and `mapping: SourceMapping`
- The `range` field means different things depending on the `mapping` variant
- For `Original`: the range represents the actual position in the file
- For `Substring`, `Concat`, `Transformed`: the range represents a synthetic "current text" position

This makes the API confusing and error-prone.

## Proposed Design

**CRITICAL FINDING**: The current design has a fundamental flaw where code directly accesses `.range.start.row` and `.range.start.column` without calling `map_offset()`. This means error messages for Substring/Concat/Transformed variants show synthetic coordinates (0, 0) instead of actual file locations!

**CORRECTED DESIGN**: Store only offsets everywhere. Row/column should ONLY be computed via `map_offset()`.

Convert `SourceInfo` into an enum where each variant contains exactly the data it needs:

```rust
/// Source information tracking a location and its transformation history
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    /// Direct position in an original file
    /// Row/column are computed on-demand via FileInformation
    Original {
        file_id: FileId,
        start_offset: usize,
        end_offset: usize,
    },
    /// Substring extraction from a parent source
    /// The offsets are relative to the parent's text
    Substring {
        parent: Rc<SourceInfo>,
        start_offset: usize,
        end_offset: usize,
    },
    /// Concatenation of multiple sources
    Concat {
        pieces: Vec<SourcePiece>,
    },
    /// Transformed text with piecewise mapping
    Transformed {
        parent: Rc<SourceInfo>,
        mapping: Vec<RangeMapping>,
    },
}
```

**Key API Changes:**

1. **Construction:**
   ```rust
   // Old
   SourceInfo::original(file_id, range)
   // New (factory method)
   SourceInfo::original(file_id, start_offset, end_offset)
   // Or from Range:
   SourceInfo::from_range(file_id, range)
   ```

2. **Accessing offsets:**
   ```rust
   // Old (BUGGY when used for error messages!)
   source_info.range.start.offset
   source_info.range.start.row  // WRONG for Substring/Concat/Transformed!

   // New
   source_info.start_offset()  // Works for all variants
   source_info.end_offset()    // Works for all variants
   ```

3. **Getting row/column (CORRECTED):**
   ```rust
   // Old (BUGGY!)
   source_info.range.start.row

   // New (CORRECT)
   let mapped = source_info.map_offset(offset, &ctx)?;
   mapped.location.row   // Correctly maps through transformation chain
   ```

4. **Computing length:**
   ```rust
   // Old
   source_info.range.end.offset - source_info.range.start.offset
   // New
   source_info.length()  // Helper method
   ```

### Helper Methods Needed

To ease migration and provide a consistent API:

```rust
impl SourceInfo {
    // Factory methods (for migration compatibility)
    pub fn original(file_id: FileId, start_offset: usize, end_offset: usize) -> Self
    pub fn from_range(file_id: FileId, range: Range) -> Self  // For existing code using Range
    pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self
    pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self
    pub fn transformed(parent: SourceInfo, mapping: Vec<RangeMapping>) -> Self
    pub fn combine(&self, other: &SourceInfo) -> Self

    // New helper methods
    pub fn length(&self) -> usize
    pub fn start_offset(&self) -> usize
    pub fn end_offset(&self) -> usize

    // Existing methods (unchanged)
    pub fn map_offset(&self, offset: usize, ctx: &SourceContext) -> Option<MappedLocation>
    pub fn map_range(&self, start: usize, end: usize, ctx: &SourceContext) -> Option<(MappedLocation, MappedLocation)>
}
```

**IMPORTANT**: Code that currently accesses `.range.start.row` or `.range.start.column` must be updated to use `map_offset()` instead!

### Benefits

1. **Type Safety**: Each variant has exactly the fields it needs
2. **Clarity**: No confusing overloaded `range` field
3. **Correctness**: Impossible to construct invalid states
4. **Simplicity**: Easier to understand and maintain
5. **Migration**: Factory methods keep most existing code working

### Concerns

1. **API Changes**: This is a breaking change for direct field access
2. **Migration**: Need to update sites that access `.range` directly
3. **Serialization**: JSON format will change (but we'll document it)

## Implementation Plan

### Phase 1: Design Review
- [ ] Review current usage patterns across codebase
- [ ] Identify all public API surface that depends on SourceInfo
- [ ] Decide on final enum variant names and fields
- [ ] Consider backward compatibility for JSON serialization

### Phase 2: Core Implementation
- [ ] Add new `SourceInfo` enum alongside old struct (with different name temporarily)
- [ ] Implement `From` traits for conversions
- [ ] Implement methods: `map_offset`, `map_range`, etc.
- [ ] Add comprehensive tests

### Phase 3: Migration
- [ ] Update `quarto-source-map` internal usage
- [ ] **FIX BUG**: Update `quarto-yaml/src/error.rs` to use `map_offset()` instead of `.range.start.row`
- [ ] Update other `quarto-yaml` usage
- [ ] Update `quarto-markdown-pandoc` usage
- [ ] Update other crates
- [ ] Run full test suite

### Phase 4: Cleanup
- [ ] Remove old `SourceInfo` struct
- [ ] Remove temporary compatibility code
- [ ] Update documentation
- [ ] Add migration notes if needed

## Analysis of Current Usage

After analyzing the codebase, here are the key findings:

### How `range` is Currently Used

1. **In `Original`**: The range represents the actual position in the source file, with full Location info (offset, row, column)
2. **In `Substring`, `Concat`, `Transformed`**: The range is synthetic - always starts at (0,0,0) and has only the offset populated in the end location to represent length
3. **External Access**: Code accesses `source_info.range.start.offset` and `source_info.range.end.offset` primarily for:
   - Computing lengths (`end.offset - start.offset`)
   - Displaying in error messages
   - Converting between different SourceInfo types (e.g., pandoc::location::SourceInfo)

### Critical Observations

1. The `range` field is **never meaningful** for Substring/Concat/Transformed except for storing length
2. Row/column are always 0 for non-Original variants
3. The `combine()` method computes length from the range field
4. Most usage is through `map_offset()` which recurses through the structure

## Design Decisions

### 1. For `Original`: Store ONLY offsets, not Location

**Decision: Store `start_offset: usize` and `end_offset: usize`**

Rationale:
- **CRITICAL**: Current code that accesses `.range.start.row` without `map_offset()` is BUGGY
- Row/column should ONLY be computed via `FileInformation.offset_to_location()`
- Storing Location encourages incorrect direct access to row/column
- Reduces memory footprint (2 usize instead of 6 usize)
- Forces correct usage pattern: always use `map_offset()` for row/column

**Rejected alternative**: Store full `Location` structs
- Con: Encourages buggy direct access to row/column
- Con: Wastes memory storing redundant row/column
- Con: Creates confusion about when row/column are valid

### 2. For `Substring`: Store both start and end offsets

**Decision: Store `start_offset: usize, end_offset: usize`**

Rationale:
- Makes the API explicit and clear
- No need to traverse parent to compute length
- Matches the current behavior where length is implicitly stored
- Simple and efficient

Alternative considered: Store `start_offset: usize, length: usize`
- Pro: Matches current implementation more closely
- Con: Less intuitive - callers usually want the range, not the length

### 3. For `Concat`: Keep SourcePiece

**Decision: Keep `SourcePiece` as-is**

Rationale:
- Already well-designed
- Clear separation of concerns
- Minimal changes needed

### 4. For `Transformed`: Keep RangeMapping

**Decision: Keep `RangeMapping` vector as-is**

Rationale:
- Well-designed, no issues found
- Clear mapping semantics

### 5. Serialization: Accept breaking change

**Decision: Accept the JSON format change, document it**

Rationale:
- This is an internal API (no external consumers yet)
- New format will be clearer
- Can add compatibility layer if needed later

## Testing Strategy

1. All existing tests must pass
2. Add new tests for:
   - Each enum variant construction
   - Pattern matching on variants
   - Edge cases (empty ranges, single-character ranges)
3. Property-based tests for `map_offset` correctness

## Timeline Estimate

- Phase 1: 30 minutes
- Phase 2: 2-3 hours
- Phase 3: 2-4 hours
- Phase 4: 1 hour

Total: 5-8 hours

## Notes

- This refactor will **fix an existing bug** in quarto-yaml error messages
- The clearer structure will help prevent similar bugs in the future
- Consider doing this refactor before tackling more complex location tracking features

## Bug Found During Analysis

**Location**: `quarto-yaml/src/error.rs:38-39` (and similar locations)

**Problem**:
```rust
write!(f, " at {}:{}",
    loc.range.start.row + 1,     // BUG: Always 0 for Substring/Concat/Transformed!
    loc.range.start.column + 1   // BUG: Always 0 for Substring/Concat/Transformed!
)?;
```

**Impact**: Error messages for YAML values that are Substring/Concat/Transformed will show "at 1:1" instead of the actual file location.

**Fix**: Must use `map_offset()` to get the true file location:
```rust
// Need to pass SourceContext and use map_offset
let mapped = loc.map_offset(loc.range.start.offset, &ctx)?;
write!(f, " at {}:{}",
    mapped.location.row + 1,
    mapped.location.column + 1
)?;
```

**However**: This requires SourceContext, which Error types don't typically have access to. This is a design issue that needs addressing separately.
