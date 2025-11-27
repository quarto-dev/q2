# Citation-Number Sorting Analysis

**Date**: 2025-11-27
**Parent Issue**: k-422 (CSL conformance)

## Test Case Analysis

From `sort_BibliographyCitationNumberDescending.txt`:

**Input**: 8 items (ITEM-1 through ITEM-8) cited in a single citation
**Sort**: `<key variable="citation-number" sort="descending"/>`
**Expected Output**:
```
[1] Book 008  <- ITEM-8 (originally cited 8th)
[2] Book 007  <- ITEM-7 (originally cited 7th)
...
[8] Book 001  <- ITEM-1 (originally cited 1st)
```

**Key Insight**: The `citation-number` variable serves TWO purposes:
1. **Sort key**: Uses the original citation order (ITEM-1=1, ..., ITEM-8=8)
2. **Display value**: Uses the final bibliography position (ITEM-8=1, ..., ITEM-1=8)

## Citeproc-hs Implementation (from Eval.hs)

The Haskell implementation uses a **two-phase assignment**:

### Phase 1: Initial Assignment (line 174)
```haskell
assignCitationNumbers sortedCiteIds
```
Assigns numbers based on citation order (when items were first cited).

### Phase 2: Reassignment After Sorting (lines 196-202)
```haskell
assignCitationNumbers $
  case layoutSortKeys biblayout of
    (SortKeyVariable Descending "citation-number":_)
      -> reverse sortedIds
    (SortKeyMacro Descending _ _:_)
      -> reverse sortedIds
    _ -> sortedIds
```

The `assignCitationNumbers` function (lines 134-149):
```haskell
assignCitationNumbers sortedIds =
  modify $ \st ->
    st{ stateRefMap = ReferenceMap $ foldl'
           (\m (citeId, num) ->
               M.adjust (\ref ->
                 ref{ referenceVariables =
                       M.insert "citation-number" (NumVal num) ...
                    }) citeId m)
           (unReferenceMap (stateRefMap st))
           (zip sortedIds [1..]) }
```

### Observation

The citeproc-hs code appears to use `reverse sortedIds` for descending citation-number sort, which would assign numbers in the ORIGINAL citation order, not bibliography position order. However, the test expects bibliography-position numbering.

**Possible explanations**:
1. The code was updated after this analysis
2. There's additional post-processing not shown
3. The test suite expects different behavior than citeproc-hs

## Proposed Implementation for quarto-citeproc

Based on test expectations, implement:

### Data Model Changes

```rust
// In Processor struct
struct Processor {
    // ... existing fields ...

    /// Initial citation numbers (assigned during citation processing)
    initial_citation_numbers: HashMap<String, i32>,

    /// Final citation numbers (reassigned after bibliography sorting)
    /// This is what gets rendered for the citation-number variable
    citation_numbers: HashMap<String, i32>,
}
```

### Algorithm

1. **During citation processing**:
   - Assign initial citation numbers based on first-cite order
   - Store in `initial_citation_numbers`

2. **When sorting bibliography by `citation-number`**:
   - Use `initial_citation_numbers` as the sort key value
   - Sort ascending or descending as specified

3. **After bibliography sorting**:
   - Reassign `citation_numbers` based on final bibliography position
   - Position 1 gets number 1, position 2 gets number 2, etc.

4. **When rendering `citation-number` variable**:
   - Use `citation_numbers` (the reassigned values)
   - NOT `initial_citation_numbers`

### Edge Cases

- Citations may need to be re-rendered after bibliography sorting if they display citation-numbers
- Multiple citations of the same item should use the same number
- Items only in bibliography (not cited) may need special handling

## Implementation Steps

1. Add `initial_citation_numbers` field to track original assignment
2. Modify `get_citation_number()` to distinguish between sort-phase and render-phase
3. Add `reassign_citation_numbers()` method called after bibliography sorting
4. Update bibliography generation to call reassignment before rendering
5. Ensure citation rendering uses final numbers

## Test Coverage

Tests that require this feature:
- `sort_BibliographyCitationNumberDescending`
- `sort_BibliographyCitationNumberDescendingSecondary`
- `sort_BibliographyCitationNumberDescendingViaCompositemacro`
- `sort_BibliographyCitationNumberDescendingViaMacro`
- `sort_CitationNumberPrimary*` (multiple variations)
- `sort_CitationNumberSecondary*` (multiple variations)

Approximately 16 tests depend on this feature.
