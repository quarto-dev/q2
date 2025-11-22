# All Writer Panics - Complete Audit

**Date**: 2025-11-21
**Issue**: k-327

## Summary

Found **29 panic!() statements** across 4 writers. All of these crash the program instead of emitting helpful errors.

## Panic Inventory by Writer

### Native Writer (`native.rs`) - 3 panics

1. **Line 94**: ColWidthPercentage - `panic!("ColWidthPercentage is not implemented yet")`
2. **Line 355**: Unsupported inline - `panic!("Unsupported inline type: {:?}", text)`
3. **Line 667**: Unsupported block - `panic!("Unsupported block type in native writer: {:?}", block)`

**Affected types**:
- Blocks: BlockMetadata, CaptionBlock
- Inlines: Shortcode, NoteReference, Attr, Insert, Delete, Highlight, EditComment
- Tables: ColWidth::Percentage

### ANSI Writer (`ansi.rs`) - 10 panics

**Block panics** (all with same pattern: "not yet implemented in ANSI writer"):

1. **Line 505**: LineBlock
2. **Line 510**: CodeBlock
3. **Line 593**: Table
4. **Line 598**: Figure
5. **Line 603**: BlockMetadata
6. **Line 608**: NoteDefinitionPara
7. **Line 613**: NoteDefinitionFencedBlock
8. **Line 618**: CaptionBlock

**Note**: ANSI writer appears to be incomplete - many standard Pandoc types also panic.

### QMD Writer (`qmd.rs`) - 2 panics

1. **Line 272**: Non-MetaMap metadata - `panic!("Expected MetaMap for metadata")`
2. **Line 1284**: CaptionBlock - `panic!("CaptionBlock found in QMD writer - should have been processed during postprocessing")`

### JSON Writer (`json.rs`) - 4 panics

1. **Line 542**: Editorial marks (Insert, Delete, Highlight, EditComment) - `panic!("Unsupported inline type: {:?}", inline)`
2. **Line 993**: CaptionBlock - `panic!("CaptionBlock found in JSON writer - should have been processed during postprocessing")`
3. **Line 1070**: Non-MetaMap metadata - `panic!("Expected MetaMap for Pandoc.meta")`
4. **Line 1216, 1258**: Test assertions (acceptable - in #[test] functions)

## Categorization by Severity

### P0 Critical - Program Crashes on Valid Input

**Native Writer**:
- All 7 inline extension types (Shortcode, Insert, Delete, Highlight, EditComment, Attr, NoteReference)
- 2 block extension types (BlockMetadata, CaptionBlock)
- ColWidth::Percentage (valid Pandoc!)

**JSON Writer**:
- 4 editorial mark inline types (Insert, Delete, Highlight, EditComment)
- CaptionBlock (but claims "should be processed before")

**QMD Writer**:
- CaptionBlock (but claims "should be processed before")

### P1 High - Expected to Be Preprocessed

These types claim they "should have been processed during postprocessing" but still crash:

- **CaptionBlock** in QMD writer (line 1284)
- **CaptionBlock** in JSON writer (line 993)

**Question**: If postprocessing always removes them, why panic? Should emit error instead.

### P2 Medium - Incomplete Implementation

**ANSI Writer**: 
- Appears to be work-in-progress
- Panics on many standard types (LineBlock, CodeBlock, Table, Figure)
- Also panics on extension types

**Status**: May not be user-facing? Need to check if ANSI writer is enabled.

### P3 Low - Defensive Checks

**QMD/JSON Writers**:
- `panic!("Expected MetaMap for metadata")` - defensive check
- **Question**: Can this ever trigger in practice?

## Recommended Action Plan

### Phase 1: Critical Fixes (Native Writer)

**Priority**: Immediate - P0 bugs that crash on valid user input

**Tasks**:
1. Replace block catch-all panic with explicit handling for BlockMetadata, CaptionBlock
2. Replace inline catch-all panic with explicit handling for all 7 extension types
3. Fix ColWidth::Percentage (should work, not panic)

**Error codes**: Q-3-20 through Q-3-36

**Estimate**: 4-6 hours

### Phase 2: JSON Writer Extension Types

**Priority**: High - P0 bugs in JSON output

**Tasks**:
1. Handle editorial marks (Insert, Delete, Highlight, EditComment)
2. Better handling for CaptionBlock (error instead of panic)

**Options**:
- Desugar editorial marks to Span with classes?
- Emit errors if can't represent in Pandoc JSON?

**Estimate**: 2-3 hours

### Phase 3: Defensive Checks

**Priority**: Medium - Improve error messages

**Tasks**:
1. Replace "Expected MetaMap" panics with proper errors
2. Replace "should have been processed" panics with errors
3. Add defensive checks for postprocessing failures

**Estimate**: 1-2 hours

### Phase 4: ANSI Writer (Optional)

**Priority**: Low - If ANSI writer is actually used

**Tasks**:
1. Determine if ANSI writer is user-facing
2. If yes: Complete implementation or emit proper errors
3. If no: Document as internal/incomplete

**Estimate**: Unknown (depends on scope)

## Testing Strategy

For each fixed panic, add test:

```rust
#[test]
fn test_blockmetadata_error_not_panic() {
    let doc = /* document with BlockMetadata */;
    let mut buf = Vec::new();
    
    // Should return Err with diagnostic, not panic
    let result = native::write(&doc, &context, &mut buf);
    
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors[0].code, Some("Q-3-20".to_string()));
}
```

## Questions for User

1. **Is ANSI writer user-facing?**
   - If yes, needs full audit and fixes
   - If no, can leave as-is with warning

2. **Should CaptionBlock ever reach writers?**
   - Claims "should be processed in postprocessing"
   - If true: defensive error is fine
   - If false: it's a postprocessing bug

3. **How should editorial marks be represented in JSON?**
   - Option A: Desugar to Span with classes (like Insert → `<span class="critic-insert">`)
   - Option B: Emit error (not supported in Pandoc JSON)
   - Option C: Custom JSON extension (breaks Pandoc compatibility)

4. **Single issue or multiple issues?**
   - Option A: One issue "Fix all writer panics" (~8-12 hours)
   - Option B: Per-writer issues (native, json, qmd, ansi)
   - Option C: Per-type issues (10+ separate issues)

## Recommendation

**Start with Phase 1 (Native Writer)** as one comprehensive issue:
- "Replace panic!() with proper error handling in native writer"
- Fixes all 3 panics, ~10 extension types
- Most critical user-facing issues
- Can be done in one focused session

Then reassess other writers based on user feedback.
