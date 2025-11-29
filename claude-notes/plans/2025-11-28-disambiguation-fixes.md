# CSL Disambiguation Fixes Plan

## Analysis Summary

After studying the failing tests, current implementation, and reference citeproc implementation, I've identified several categories of issues in our disambiguation algorithm.

### Current Test Status
- **858 total** CSL conformance tests
- **477 passing** (55.6%)
- **381 failing** (44.4%)
- **~34 failing** specifically related to disambiguation

## Issue Categories

### 1. Year Suffix Not Being Rendered (Critical)

**Symptoms:**
- Tests like `disambiguate_YearSuffixFiftyTwoEntries` show no year suffixes at all
- Expected: `(Smith 1986a; Smith 1986b; ...)`
- Actual: `(Smith 1986; Smith 1986; ...)`

**Root Cause:**
Year suffixes are assigned to references, but the **re-rendering after disambiguation** doesn't happen. The algorithm:
1. Renders citations initially
2. Detects ambiguities
3. Assigns year suffixes to references
4. **Never re-renders** with the new suffixes

**Fix:**
The citeproc reference implementation does this with `refreshAmbiguities`:
```haskell
>>= (\as ->
     (if not (null as) && disambiguateAddYearSuffix strategy
         then do
           addYearSuffixes bibSortKeyMap as
           refreshAmbiguities cs  -- RE-RENDER HERE
         else return as))
```

We need to implement an iterative disambiguation loop that:
1. Renders citations
2. Detects ambiguities
3. Applies one disambiguation method
4. **Re-renders** citations
5. Detects remaining ambiguities
6. Repeat until no more disambiguation methods or no more ambiguities

### 2. Add Names Not Preserving Et-Al Truncation (High)

**Symptoms:**
- Test `disambiguate_AndreaEg1a` expects: `Smith, Brown, et al. (1980)`
- We produce: `Smith, Brown & Jones (1980)` (showing all names)

**Root Cause:**
When we expand et-al for disambiguation, we set `et_al_names` to show more names. But the implementation shows **all** names instead of the minimum required to disambiguate.

**Fix:**
The `try_add_names` function should:
1. Start with `et_al_use_first` names
2. Incrementally add one name at a time
3. Stop as soon as disambiguation is achieved
4. Re-render and check if still ambiguous

### 3. Year Suffix Letter Generation (Medium)

**Current code:**
```rust
fn suffix_to_letter(suffix: i32) -> String {
    // 1 -> "a", 2 -> "b", ... 26 -> "z", 27 -> "aa", 28 -> "ab"...
}
```

This appears correct but needs verification for:
- Suffixes beyond 26 (aa, ab, ac, ..., az, ba, bb, ...)
- The test `disambiguate_YearSuffixFiftyTwoEntries` expects suffixes up to "az" (52 entries)

### 4. Citation-Label Disambiguation (Medium)

**Symptoms:**
- Tests like `disambiguate_CitationLabelDefault` and `disambiguate_CitationLabelInData` fail
- These involve the `citation-label` variable which is auto-generated

**Root Cause:**
We may not be generating `citation-label` values correctly, or not applying disambiguation to them.

### 5. Incremental Citation Format (Low)

**Symptoms:**
- Many integration tests show `..[0]` and `>>[1]` markers
- These indicate incremental citation processing

**Root Cause:**
The test framework uses a special "CITATIONS" format for incremental citation testing. The markers mean:
- `..` = citation unchanged from previous
- `>>` = citation updated

This is a test harness feature, not a core disambiguation issue.

### 6. JSON Parsing Issues (Technical Debt)

**Symptoms:**
- Some tests fail with: `Input JSON error: invalid type: string "22:56:08", expected i32`

**Root Cause:**
The `accessed.season` field in some test data contains a time string, but our parser expects an integer.

## Proposed Implementation Order

### Phase 1: Core Re-rendering Loop (Highest Impact)

**Estimated tests fixed:** ~15-20

1. Refactor `process_citations_with_disambiguation_to_outputs` to use an iterative loop:
   ```rust
   loop {
       // 1. Render all citations
       let outputs = self.render_citations(&citations);

       // 2. Extract disambiguation data
       let disamb_data = extract_disamb_data(&outputs);

       // 3. Find ambiguities
       let ambiguities = find_ambiguities(disamb_data);

       if ambiguities.is_empty() {
           break; // Done
       }

       // 4. Try next disambiguation method
       if !applied_method(&ambiguities) {
           break; // No more methods
       }

       // 5. Loop continues with re-render
   }
   ```

2. Order of disambiguation methods (per CSL spec):
   - Add names (expand et-al)
   - Add given names (initials or full)
   - Add year suffixes
   - Set disambiguate condition

### Phase 2: Fix Add-Names Logic

**Estimated tests fixed:** ~5-8

1. Make `try_add_names` truly incremental:
   - Start at current `et_al_use_first`
   - Increment by 1, not jump to max
   - Stop when disambiguation achieved for a reference
   - Different references in same ambiguity group may need different counts

2. After adding names, don't show more names than needed

### Phase 3: Year Suffix Assignment

**Estimated tests fixed:** ~8-10

1. Verify year suffix assignment uses bibliography sort order
2. Verify suffix rendering in date output
3. Handle edge cases:
   - Same author, different years (no suffix needed)
   - Same author, same year, different works (suffix needed)
   - Mixed author lists with partial overlap

### Phase 4: Citation-Label

**Estimated tests fixed:** ~3-5

1. Implement `citation-label` variable generation
2. Apply year suffixes to citation-labels

### Phase 5: Cleanup

1. Fix JSON parsing for `season` field
2. Handle edge cases in incremental citation format

## Architecture Notes from Citeproc Reference

Key insight: Citeproc uses **monadic state** to:
1. Track which references have been modified (et_al_names, name_hints, year_suffix)
2. Re-render citations with updated state
3. Detect new ambiguities after each modification

Our current architecture sets state on references but doesn't re-render. The fix requires:

1. **Separation of concerns:**
   - `render_citation()` - pure rendering with current reference state
   - `disambiguate_citations()` - iterative loop that modifies state and re-renders

2. **State tracking:**
   - Current: We set `et_al_names`, `name_hints`, `year_suffix` on Reference
   - These need to affect rendering when set

3. **Termination guarantee:**
   - Each disambiguation method should make progress
   - After all methods exhausted, set `disambiguate_condition` on remaining ambiguities

## Test Cases to Focus On

### Blocking Issues (Must Fix First)
1. `disambiguate_YearSuffixFiftyTwoEntries` - year suffixes not appearing
2. `disambiguate_AndreaEg1a` - et-al truncation not working
3. `disambiguate_AndreaEg3` - year suffixes + given names

### Quick Wins
1. JSON parsing fixes - simple type change
2. Integration test format - test harness adjustment

### Complex Cases (After Core Fixes)
1. `disambiguate_ByCiteIncremental*` - incremental + ByCite rule
2. `disambiguate_SetsOfNames` - complex name matching
3. `bugreports_DisambiguationAddNames*` - edge cases

## Implementation Priority

1. **Week 1:** Implement re-rendering loop (Phase 1)
   - This will likely fix many year suffix tests

2. **Week 2:** Fix add-names logic (Phase 2)
   - Focus on et-al truncation preservation

3. **Week 3:** Year suffix edge cases + citation-label (Phases 3-4)

4. **Ongoing:** Fix remaining edge cases as they're discovered

## Files to Modify

1. `crates/quarto-citeproc/src/types.rs` - `process_citations_with_disambiguation_to_outputs()`
2. `crates/quarto-citeproc/src/disambiguation.rs` - Core disambiguation functions
3. `crates/quarto-citeproc/src/eval.rs` - Ensure year_suffix rendering works
4. `crates/quarto-citeproc/src/reference.rs` - May need to add `citation_label` field

## Success Metrics

- Pass rate increase from 55.6% to 65%+ after Phase 1
- Pass rate increase to 70%+ after Phase 2
- Pass rate increase to 75%+ after Phases 3-4
