# Remaining Tests Analysis - 2025-11-28

**Current State**: 534/930 tests passing (57.4% coverage)
**Previous**: 501 tests (added 33 quick wins from ignored tests)

## Summary of Failing Tests by Category

| Category | Failures | Key Issues |
|----------|----------|------------|
| name | 124 | Text-case on names, labels, edge cases |
| bugreports | 124 | Various edge cases |
| sort | 72 | Year suffix collapse, complex sorting |
| magic | 56 | Various CSL "magic" features |
| disambiguate | 56 | Year suffix, name disambiguation |
| date | 34 | Locale overrides, formatting |
| number | 26 | Ordinal gender, page ranges |
| locale | 26 | Date order, empty term handling |
| textcase | 24 | Uppercase on names |
| integration | 24 | Most need CITATIONS format parsing |
| punctuation | 22 | Various punctuation rules |
| position | 20 | Empty output (CITATIONS format needed) |
| flipflop | 20 | Nested formatting |
| collapse | 18 | Extra comma, year-suffix ranges |
| page | 16 | Page range formatting |
| nameattr | 16 | Name attribute edge cases |
| label | 16 | Label rendering |
| plural | 14 | Plural detection |
| group | 12 | Group rendering edge cases |
| decorations | 10 | Formatting decorations |

## Root Cause Analysis

### 1. CITATIONS Format Not Parsed (~40-60 tests)

Many tests use the complex CITATIONS format instead of CITATION-ITEMS:

```json
[
  [
    {
      "citationID": "CITATION-1",
      "citationItems": [{"id": "ITEM-1"}],
      "properties": {"noteIndex": 0}
    },
    [],  // citations_pre
    []   // citations_post
  ]
]
```

Our parser expects simpler format and falls through to default behavior (empty citations).

**Impact**: Most `position_*`, many `integration_*`, and various `bugreports_*` tests

### 2. Text-Case Not Applied to Names (~20-30 tests)

Example from `textcase_uppercase`:
- Expected: `SMITH, John`
- Actual: `Smith, John`

The `text-case="uppercase"` on names element isn't being applied.

### 3. Year Suffix Range Collapsing (~15-20 tests)

Example from `collapse_yearsuffixcollapse`:
- Expected: `Smith 2000a–e, 2001`
- Actual: `Smith 2000, 2000a, 2000b, 2000c, 2000d, 2001`

Need to detect consecutive year suffixes and collapse to ranges.

### 4. Label Rendering After Names (~10-15 tests)

Example from `label_compactnamesafterfullnames`:
- Expected: `Alan Aalto, editor`
- Actual: `Alan Aalto`

Labels after names aren't being rendered.

### 5. Extra Delimiter After Author Collapse (~10 tests)

Example from `collapse_authorcollapsenodatesorted`:
- Expected: `(Smith 325 AD, 2000)`
- Actual: `(Smith, 325 AD, 2000)`

Extra comma appears after author when years are collapsed.

### 6. Locale Date Part Order (~5-10 tests)

Example from `locale_emptyplusoverridedate`:
- Expected: `2000 June 18`
- Actual: `18 June 2000`

Locale overrides for date-parts order not being applied.

### 7. Empty Term Trailing Space (~5-10 tests)

Example from `locale_forceemptyetalterm`:
- Expected: `John Doe`
- Actual: `John Doe `

When et-al term is empty, we still add the delimiter space.

## Proposed Priority Order

### Priority 1: CITATIONS Format Parsing (High Impact)

**Estimated effort**: Medium
**Tests unlocked**: 40-60

Update `build_citations()` in test harness to parse the full CITATIONS format:
- Extract `citationID`, `citationItems` from first element of each array
- Extract `noteIndex` from `properties` and set on Citation
- Handle `citations_pre` and `citations_post` arrays (for incremental processing tests)

This is primarily a test harness change, not core library change.

### Priority 2: Text-Case on Names (Medium Impact)

**Estimated effort**: Small
**Tests unlocked**: 20-30

Ensure text-case transformations are applied to names elements when specified.
Check `format_names()` to see if formatting.text_case is being applied.

### Priority 3: Label After Names (Medium Impact)

**Estimated effort**: Small-Medium
**Tests unlocked**: 10-15

Check why labels aren't rendering after names. May be a missing case in name rendering.

### Priority 4: Year Suffix Range Collapsing (Medium Impact)

**Estimated effort**: Medium
**Tests unlocked**: 15-20

Implement year suffix range detection and collapsing:
- Detect sequences like `a, b, c, d, e` → `a–e`
- Handle non-consecutive like `a, c, d, e` → `a, c–e`

### Priority 5: Collapse Delimiter Cleanup (Small Impact)

**Estimated effort**: Small
**Tests unlocked**: 10

Fix extra delimiter after author collapse. Likely a punctuation fixup issue at collapse boundaries.

### Priority 6: Locale Date Part Order (Small Impact)

**Estimated effort**: Small
**Tests unlocked**: 5-10

Ensure locale-overridden date formats use the locale's part order.

### Priority 7: Empty Term Space Handling (Small Impact)

**Estimated effort**: Small
**Tests unlocked**: 5-10

Don't add delimiter space when term expands to empty string.

## Recommendation

Start with **Priority 1 (CITATIONS Format Parsing)** because:
1. It's a test harness change, not a core library change
2. It unlocks the most tests with a single change
3. It will reveal the true state of position tracking and integration tests
4. Many tests that currently show "empty output" will start showing actual failures with useful diffs

After that, proceed to Priority 2-3 which are relatively quick fixes that unlock significant test counts.
