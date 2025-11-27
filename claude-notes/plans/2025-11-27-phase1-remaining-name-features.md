# Phase 1 Remaining: Name Formatting Features

**Parent issue:** k-422 (quarto-citeproc: Citation processing engine)
**Status:** Optional/Deferred
**Priority:** Low - these are edge cases that can be addressed later

## Overview

Phase 1 (a, b, c) brought test coverage from 155 → 262 tests (107 new tests).
The remaining items are edge cases that affect fewer tests and can be deferred.

## Remaining Features

### 1. HTML Entity Escaping for Bibliography Output

**Current behavior:** `&` is output as literal `&`
**Expected:** `&` should be `&#38;` in HTML bibliography output

**Affected tests:**
- `name_DelimiterAfterInverted` (the core logic is fixed, just HTML escaping remains)
- Potentially other bibliography tests with special characters

**Implementation notes:**
- Add HTML entity escaping in bibliography output rendering
- Escape `&`, `<`, `>`, `"` to their entity equivalents
- May need to be conditional based on output format

**Effort:** Low
**Impact:** ~5-10 tests

### 2. Complex `initialize="false"` Variants

**Current behavior:** Basic `initialize="false"` with space or empty works
**Missing:** Period-space patterns like `initialize-with=". "`

**Affected tests:**
- `name_InitialsInitializeFalsePeriod`
- `name_InitialsInitializeFalsePeriodSpace`
- `name_InitialsInitializeTrue*` variants

**Implementation notes:**
- When `initialize="false"` and `initialize-with` contains periods
- Need to add trailing period to normalized names
- Complex rules about when spaces/periods are added

**Effort:** Medium
**Impact:** ~6 tests

### 3. `name-part` Element Support

**Description:** CSL `<name-part>` elements allow per-part formatting

```xml
<name-part name="given" suffix="&#160;"/>  <!-- non-breaking space after given -->
<name-part name="family" text-case="uppercase"/>
```

**Affected tests:**
- `name_WithNonBreakingSpace`
- `name_namepartAffixes*` tests
- Various name formatting edge cases

**Implementation notes:**
- Parse `<name-part>` children of `<name>` element
- Store as `Vec<NamePart>` in `Name` struct
- Apply prefix/suffix/formatting per name part during rendering

**Effort:** Medium-High
**Impact:** ~10-15 tests

### 4. Apostrophe Handling in Given Names

**Current behavior:** Apostrophes may cause initialization issues
**Expected:** "O'Brien" should initialize correctly

**Affected tests:**
- `name_ApostropheInGivenName`

**Implementation notes:**
- Treat apostrophe as part of the name, not a separator
- "D'Angelo" → "D." (not "D.'A.")

**Effort:** Low
**Impact:** ~2 tests

## Recommendation

These features are low priority because:
1. They affect relatively few tests
2. The core name formatting is working well
3. Phase 2 (bibliography sorting) will unlock 62+ tests with potentially less effort

Suggest deferring these until after Phase 2-4 are complete, then revisiting
if test coverage goals require them.

## Test Summary

| Feature | Tests Affected | Effort | Priority |
|---------|---------------|--------|----------|
| HTML entity escaping | ~10 | Low | Medium |
| Complex initialize="false" | ~6 | Medium | Low |
| name-part elements | ~15 | Medium-High | Low |
| Apostrophe handling | ~2 | Low | Low |

**Total potential:** ~33 tests
**Current coverage:** 262/896 (29.2%)
**After all Phase 1:** ~295/896 (32.9%)
