# Failing Test Analysis for quarto-citeproc

**Date**: 2025-11-28
**Current Status**: 380/858 tests passing (44.3%)

## Summary of Failing Tests by Category

| Category | Failing | Description |
|----------|---------|-------------|
| bugreports | 68 | Various real-world bug fixes |
| name | 65 | Name formatting edge cases |
| magic | 38 | Special citeproc behaviors |
| sort | 36 | Sorting edge cases |
| disambiguate | 30 | Disambiguation features |
| textcase | 25 | Title case transformation |
| date | 18 | Date formatting |
| label | 17 | Label rendering |
| locale | 16 | Locale/i18n handling |
| number | 15 | Number/ordinal formatting |
| flipflop | 15 | Flip-flop formatting |
| punctuation | 13 | Punctuation handling |
| integration | 13 | Integration tests |
| position | 13 | Citation position (ibid, etc.) |

## Detailed Analysis

### 1. Title Case Transformation (25 textcase + many bugreports)

**Current Issue**: Our title-case implementation doesn't handle:
- Stop words (a, an, the, of, in, on, etc.) - should stay lowercase
- Words after colons - should be capitalized
- Words after slashes - should be capitalized
- `<span class="nocase">` - should preserve case

**Example Failure**:
```
Expected: "This IS a Pen That Is a Cat/Mouse smith Pencil"
Actual:   "This IS A Pen That Is A Cat/mouse smith Pencil"
```

**Fix Complexity**: Medium - Need proper English title-case rules
**Impact**: ~30-40 tests

### 2. Moving Punctuation (13 punctuation + some bugreports)

**Current Issue**: CSL has complex rules for how prefix/suffix punctuation interacts:
- Suffix ":" + prefix ": " → should produce single ":"
- Suffix ";" + prefix ": " → colon is suppressed
- Different rules for different punctuation combinations

**Example Failure**:
```
Expected: "colon: colon"
Actual:   "colon:: colon"
```

**Fix Complexity**: Medium-High - Need to implement CSL punctuation exchange
**Impact**: ~15-20 tests

### 3. Citation Position (13 position tests)

**Current Issue**: Position detection (first, ibid, subsequent, near-note) is incomplete:
- `<if position="ibid">` not fully implemented
- `<if position="near-note">` not implemented
- Position tracking across citations

**Fix Complexity**: Medium - Need position tracking state
**Impact**: ~15-20 tests

### 4. Flip-Flop Formatting (15 flipflop tests)

**Current Issue**: When italic text contains italic markup, the inner should flip to normal:
```html
<i>Title with <i>emphasis</i> word</i>  →  <i>Title with </i>emphasis<i> word</i>
```

**Fix Complexity**: Medium - Need to track formatting state
**Impact**: ~15-20 tests

### 5. Label Formatting (17 label tests)

**Current Issue**: Missing features:
- `<label>` element not fully implemented
- Editor/translator label combinations
- Plural detection for labels
- Empty label suppression

**Fix Complexity**: Medium
**Impact**: ~17 tests

### 6. Number/Ordinal Formatting (15 number tests)

**Current Issue**: Missing features:
- Ordinal suffixes (1st, 2nd, 3rd) with locale support
- Gender-specific ordinals (French: 1er vs 1re)
- Page range formatting
- Ordinal spacing options

**Fix Complexity**: Medium
**Impact**: ~15 tests

### 7. Locale Handling (16 locale tests)

**Current Issue**: Missing features:
- Empty locale terms
- Locale override cascading
- Page range delimiter terms
- Non-existent locale fallback

**Fix Complexity**: Low-Medium
**Impact**: ~16 tests

## Recommended Priority Order

### High Priority (Most Impact)

1. **Title Case (P1)** - 30-40 tests
   - Implement proper English title-case with stop words
   - Handle words after colons, slashes, hyphens
   - Respect `<span class="nocase">`

2. **Moving Punctuation (P1)** - 15-20 tests
   - Implement CSL punctuation exchange rules
   - Integrate with our existing fix_punct infrastructure

3. **Citation Position (P2)** - 15-20 tests
   - Add position tracking to Processor
   - Implement ibid, near-note detection

### Medium Priority

4. **Flip-Flop Formatting (P2)** - 15-20 tests
   - Track formatting state during rendering
   - Flip nested identical formatting

5. **Label Element (P2)** - 17 tests
   - Implement `<label>` element fully
   - Add plural detection

6. **Ordinal Formatting (P3)** - 15 tests
   - Add ordinal suffix generation
   - Locale-aware ordinals

### Lower Priority

7. **Locale Improvements (P3)** - 16 tests
8. **Magic Behaviors (P4)** - 38 tests (many require layout changes)

## Quick Wins

Some individual tests that might pass with small fixes:

1. Several `name_*` tests may pass with minor name formatting tweaks
2. Some `condition_*` tests may pass with condition evaluation fixes
3. Some `date_*` tests may pass with date formatting improvements

## Recommendation

**Next Step**: Focus on **Title Case Transformation** (P1)

Rationale:
- Affects many tests across categories
- Well-defined rules (English title case)
- Our existing text-case code just needs improvement
- High impact for moderate effort

The implementation would involve:
1. Create a list of English stop words
2. Modify `apply_title_case()` to preserve stop words lowercase
3. Handle special cases (after colon, first word, etc.)
4. Integrate with nocase span handling

This single improvement could unlock 30-40 tests.
