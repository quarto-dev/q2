# Disambiguation Bugs Analysis

**Date:** 2025-11-29
**Beads Issue:** k-427
**Status:** IMPLEMENTED - Both bugs fixed, 14 new tests passing

## Executive Summary

After thorough study of Pandoc's citeproc-hs reference implementation and our quarto-citeproc code, I identified **two critical bugs** in the ambiguity detection logic. These bugs prevent proper disambiguation even though the re-rendering infrastructure is correctly implemented.

Current state: 523/858 tests passing (61%), 40/72 disambiguation tests passing (56%).

## Background: How Disambiguation Should Work

The CSL disambiguation algorithm applies methods in order:

1. **Add names** - Expand et-al truncated lists (e.g., "Smith et al." → "Smith, Brown, et al.")
2. **Add given names** - Show initials or full given names (e.g., "Smith" → "J. Smith")
3. **Add year suffix** - Append a, b, c... to years (e.g., "2020" → "2020a")
4. **Set disambiguate condition** - Enable `<if disambiguate="true">` for remaining ambiguities

**Critical insight:** After each method, the algorithm must:
1. Re-render all citations
2. Re-detect ambiguities from the NEW rendered output
3. Only pass REMAINING ambiguities to the next method

Our code has the re-rendering loop (good!), but the ambiguity detection is flawed.

---

## Bug 1: `find_ambiguities` Uses Wrong Grouping Strategy

### Location
`crates/quarto-citeproc/src/disambiguation.rs`, lines 109-152

### The Problem

Our `find_ambiguities` function first groups items by ALL author family names, then by rendered text within each name group. This two-stage approach prevents detecting ambiguity between items with different authors but identical rendered output.

**Our code (incorrect):**
```rust
pub fn find_ambiguities(items: Vec<DisambData>) -> Vec<Vec<DisambData>> {
    // Stage 1: Group by ALL family names
    let mut name_groups: HashMap<String, Vec<DisambData>> = HashMap::new();
    for data in items {
        let family_names: Vec<&str> = data.names.iter()
            .filter_map(|n| n.family.as_ref().map(|s| s.as_str()))
            .collect();
        let name_key = family_names.join("|");  // ← Groups by ALL names!
        name_groups.entry(name_key).or_default().push(data);
    }

    // Stage 2: Within each name group, group by rendered text
    for (_name_key, group) in name_groups {
        let mut render_groups: HashMap<String, Vec<DisambData>> = HashMap::new();
        // ...
    }
}
```

**Pandoc's code (correct):**
```haskell
getAmbiguities :: CiteprocOutput a => [Output a] -> [[DisambData]]
getAmbiguities =
        mapMaybe (filterAmbiguous)
      . groupBy (\x y -> ddRendered x == ddRendered y)  -- Just group by rendered text!
      . sortOn ddRendered
      . map toDisambData
      . extractTagItems
```

### Concrete Example

Test: `disambiguate_AddNamesSuccess`

**Input:**
- ITEM-1: authors = [Smith, Brown, Jones], year = 1980
- ITEM-2: authors = [Smith, Beefheart, Jones], year = 1980

**With et-al-min=3, et-al-use-first=1:**
- Both render as "Smith et al. (1980)" - **should be detected as ambiguous!**

**What our code does:**
- ITEM-1 gets name_key = "Smith|Brown|Jones"
- ITEM-2 gets name_key = "Smith|Beefheart|Jones"
- Different name_keys → different groups → **never compared for ambiguity!**

**Result:**
- Expected: "Smith, Brown, et al. (1980); Smith, Beefheart, et al. (1980)"
- Actual: "Smith et al. (1980); Smith et al. (1980)"

### Tests Affected

This bug affects all tests that rely on `add-names` disambiguation:
- `disambiguate_AddNamesSuccess`
- `disambiguate_AndreaEg1a`, `AndreaEg1b`, `AndreaEg2`, `AndreaEg3`, `AndreaEg4`, `AndreaEg5`
- `disambiguate_AllNamesBaseNameCountOnFailureIfYearSuffixAvailable`
- `disambiguate_ByCiteBaseNameCountOnFailureIfYearSuffixAvailable`
- And others where different author lists render identically

### The Fix

Replace the two-stage grouping with simple rendered-text grouping:

```rust
pub fn find_ambiguities(items: Vec<DisambData>) -> Vec<Vec<DisambData>> {
    // Group by rendered text only - this is what Pandoc does
    let mut render_groups: HashMap<String, Vec<DisambData>> = HashMap::new();
    for data in items {
        render_groups.entry(data.rendered.clone()).or_default().push(data);
    }

    // Return groups with >1 unique item ID
    render_groups.into_values()
        .filter(|group| {
            let mut unique_ids: Vec<&str> = group.iter()
                .map(|d| d.item_id.as_str())
                .collect();
            unique_ids.sort();
            unique_ids.dedup();
            unique_ids.len() > 1
        })
        .collect()
}
```

---

## Bug 2: Year Suffix Uses Wrong Ambiguity Detection

### Location
`crates/quarto-citeproc/src/types.rs`, lines 1000-1018

### The Problem

When deciding which items need year suffixes, we call `find_year_suffix_ambiguities` which creates FRESH groups based on author family names + year. This ignores whether previous disambiguation methods already resolved the ambiguity.

**Our code (incorrect):**
```rust
// 4. Add year suffixes if enabled
if add_year_suffix {
    // BUG: Creates new groups instead of using current ambiguities!
    let year_suffix_ambiguities = find_year_suffix_ambiguities(disamb_data.clone(), self);
    if !year_suffix_ambiguities.is_empty() {
        let suffixes = assign_year_suffixes(self, &year_suffix_ambiguities);
        // ...
    }
}
```

**Pandoc's code (correct):**
```haskell
analyzeAmbiguities mblang strategy cs ambiguities = do
    return ambiguities
        >>= (\as -> ...)  -- add names
        >>= (\as -> ...)  -- add given names
        >>= (\as ->       -- add year suffixes
             (if not (null as) && disambiguateAddYearSuffix strategy
                 then do
                   addYearSuffixes bibSortKeyMap as  -- Uses CURRENT ambiguities!
                   refreshAmbiguities cs
                 else return as))
```

### Concrete Example

Test: `disambiguate_YearSuffixMacroSameYearExplicit`

**Input:**
- ITEM-1: author = A Smith, year = 2001
- ITEM-2: author = B Smith, year = 2001

**Style:** `disambiguate-add-givenname="true" disambiguate-add-year-suffix="true"`

**Correct behavior:**
1. Initial render: both "Smith 2001" - ambiguous
2. Apply add-givenname: "A Smith 2001" vs "B Smith 2001" - **now disambiguated!**
3. Year suffix check: no remaining ambiguities → **no suffixes needed**

**What our code does:**
1. Initial render: both "Smith 2001" - ambiguous
2. Apply add-givenname: "A Smith 2001" vs "B Smith 2001" - disambiguated
3. Year suffix check: `find_year_suffix_ambiguities` creates NEW groups by family name + year
   - Both have family "Smith" and year 2001 → grouped together → **suffixes added!**

**Result:**
- Expected: "A Smith 2001" and "B Smith 2001"
- Actual: "A Smith 2001a" and "B Smith 2001b"

### Tests Affected

This bug affects tests where earlier methods should prevent year suffix:
- `disambiguate_YearSuffixMacroSameYearExplicit`
- `disambiguate_YearSuffixMacroSameYearImplicit`
- Any test with both `add-givenname` and `add-year-suffix` where givenname resolves it

### The Fix

Use the current `ambiguities` variable (which reflects remaining ambiguities after previous steps) instead of creating new groups:

```rust
// 4. Add year suffixes if enabled - only for REMAINING ambiguities
if add_year_suffix && !ambiguities.is_empty() {
    let suffixes = assign_year_suffixes(self, &ambiguities);
    for (item_id, suffix) in suffixes {
        self.set_year_suffix(&item_id, suffix);
    }
    // Re-render and refresh ambiguities
    outputs = citations_with_positions
        .iter()
        .map(|c| self.process_citation_to_output(c))
        .collect::<Result<Vec<_>>>()?;
    disamb_data = extract_disamb_data(&outputs);
    ambiguities = find_ambiguities(disamb_data);
}
```

Note: `find_year_suffix_ambiguities` function can be removed or kept for other purposes, but should NOT be used in the main disambiguation flow.

---

## Summary of All Failing Disambiguation Tests (32 total)

| Test Name | Primary Bug | Notes |
|-----------|-------------|-------|
| `AddNamesSuccess` | Bug 1 | Different 2nd author, same rendered |
| `AllNamesBaseNameCountOnFailureIfYearSuffixAvailable` | Bug 1 | |
| `AndreaEg1a`, `1b`, `2`, `3`, `4`, `5` | Bug 1 | Different author lists |
| `BasedOnEtAlSubsequent` | Unknown | Position-related? |
| `BasedOnSubsequentFormWithBackref2` | Unknown | |
| `ByCiteBaseNameCountOnFailureIfYearSuffixAvailable` | Bug 1 | |
| `ByCiteIncremental1`, `2` | Bug 1 | |
| `CitationLabelDefault`, `InData` | Missing feature | citation-label variable |
| `DisambiguationHang` | Unknown | Performance? |
| `IncrementalExtraText` | Unknown | |
| `InitializeWithButNoDisambiguation` | Unknown | |
| `SetsOfNames` | Bug 1 | |
| `SkipAccessedYearSuffix` | Unknown | |
| `Trigraph` | Missing feature | trigraph generation |
| `WithOriginalYear` | Unknown | |
| `YearCollapseWithInstitution` | Unknown | Institution names? |
| `YearSuffixAndSort` | Bug 2 + sorting | Suffix order wrong |
| `YearSuffixAtTwoLevels` | Unknown | |
| `YearSuffixMacroSameYearExplicit` | Bug 2 | Givenname should prevent |
| `YearSuffixMacroSameYearImplicit` | Bug 2 | |
| `YearSuffixMixedDates` | Unknown | |
| `YearSuffixTwoPairsFullNamesBibliography` | Unknown | |
| `YearSuffixWithEtAlSubequent` | Unknown | |
| `YearSuffixWithEtAlSubsequent` | Unknown | |
| `YearSuffixWithMixedCreatorTypes` | Unknown | |

---

## Implementation Plan

### Phase 1: Fix Bug 1 (Highest Impact)

1. Simplify `find_ambiguities` in `disambiguation.rs`
2. Remove the name-based pre-grouping
3. Test with `disambiguate_AddNamesSuccess` and `AndreaEg*` tests

**Expected impact:** ~10-15 tests fixed

### Phase 2: Fix Bug 2

1. Modify `process_citations_with_disambiguation_to_outputs` in `types.rs`
2. Use `ambiguities` variable instead of `find_year_suffix_ambiguities`
3. Test with `YearSuffixMacroSameYear*` tests

**Expected impact:** ~3-5 tests fixed

### Phase 3: Cleanup and Edge Cases

1. Consider removing `find_year_suffix_ambiguities` if no longer needed
2. Address remaining test failures case by case
3. Update enabled_tests.txt and lockfile

---

## Related Files

- `crates/quarto-citeproc/src/disambiguation.rs` - Core disambiguation functions
- `crates/quarto-citeproc/src/types.rs` - Main disambiguation flow
- `crates/quarto-citeproc/src/eval.rs` - Citation/bibliography rendering
- `crates/quarto-citeproc/src/output.rs` - Output AST and name extraction

## Reference Implementation

- `external-sources/citeproc/src/Citeproc/Eval.hs` - Pandoc's disambiguation
  - `disambiguateCitations` (line 424) - Main entry point
  - `analyzeAmbiguities` (line 531) - The disambiguation loop
  - `getAmbiguities` (line 732) - Simple rendered-text grouping
  - `refreshAmbiguities` (line 523) - Re-render and re-detect

## CSL Spec Reference

- `claude-notes/csl-spec/04-disambiguation.md` - Summarized spec
- `external-sources/csl-spec/specification.rst` lines 1584-1677 - Original spec

---

## Implementation Results (2025-11-29)

### Changes Made

1. **Bug 1 Fixed:** Simplified `find_ambiguities` in `disambiguation.rs:111-135`
   - Removed the two-stage grouping (by family names, then by rendered text)
   - Now groups by rendered text only, matching Pandoc's `getAmbiguities`

2. **Bug 2 Fixed:** Updated year suffix logic in `types.rs:1016-1039`
   - Use rendered-text `ambiguities` when available (preserves citation order)
   - Fall back to `find_year_suffix_with_full_author_match` for collapsed citations
   - Added new function for full author match (family + given names)

3. **New Function:** `find_year_suffix_with_full_author_match` in `disambiguation.rs:197-243`
   - Groups by full author list (family + given) + year
   - Handles collapsed citation case where rendered text differs due to name suppression

### Test Results

- **Before:** 523 passing (61.0%)
- **After:** 537 passing (62.6%)
- **Improvement:** +14 tests (all in disambiguate category)

### Newly Enabled Tests

1. `disambiguate_AddNamesSuccess`
2. `disambiguate_AllNamesBaseNameCountOnFailureIfYearSuffixAvailable`
3. `disambiguate_AndreaEg1a`
4. `disambiguate_AndreaEg1b`
5. `disambiguate_AndreaEg2`
6. `disambiguate_AndreaEg3`
7. `disambiguate_AndreaEg4`
8. `disambiguate_AndreaEg5`
9. `disambiguate_ByCiteBaseNameCountOnFailureIfYearSuffixAvailable`
10. `disambiguate_YearSuffixAndSort`
11. `disambiguate_YearSuffixMacroSameYearExplicit`
12. `disambiguate_YearSuffixMacroSameYearImplicit`
13. `disambiguate_YearSuffixTwoPairsFullNamesBibliography`
14. `disambiguate_YearSuffixWithMixedCreatorTypes`

### Remaining Failing Tests (11 unknown in disambiguate category)

Many of these also fail in Pandoc's citeproc or require features not yet implemented:
- `basedonetalsubsequent`, `basedonsubsequentformwithbackref2` - Position-based disambiguation
- `byciteincremental1/2` - Incremental ByCite processing
- `citationlabeldefault/indata` - Missing `citation-label` variable
- `disambiguationhang` - Fails in Pandoc too
- `initializewithbutnodisambiguation` - Fails in Pandoc too
- `setsofnames` - Complex name matching
- `trigraph` - Missing trigraph generation
- `yearcollapsewithinstitution` - Institution name handling
- `yearsuffixattwolevels` - Fails in Pandoc too
