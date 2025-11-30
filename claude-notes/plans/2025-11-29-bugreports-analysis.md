# Bugreports Category Analysis

**Date**: 2025-11-29
**Status**: In Progress

## Overview

Analysis of the 31 unknown tests in the `bugreports` category. All tests fail currently, but **none are in Pandoc's expected failures list**, meaning these should all be fixable.

## Pattern Summary

| Pattern | Tests Affected | Priority | Effort |
|---------|---------------|----------|--------|
| **Punctuation-in-quote** | 8 | High | Medium |
| **Year-suffix disambiguation** | 5 | High | High |
| **Page range en-dash** | 4 | Medium | Low |
| **Missing final period** | 3 | Medium | Low |
| **French locale guillemets** | 3 | Medium | Medium |
| **Space inside italic tags** | 2 | Low | Low |
| **Citation collapsing** | 2 | Medium | High |
| **Misc (single tests)** | 8 | Varies | Varies |

## Detailed Patterns

### 1. Punctuation-in-Quote (8 tests) - HIGH PRIORITY

**Tests**:
- `bugreports_asaspacing`
- `bugreports_contentpunctuationduplicate1`
- `bugreports_duplicatespaces2`
- `bugreports_ieeepunctuation`
- `bugreports_movepunctuationinsidequotesforlocator`
- `bugreports_thesisuniversityappearstwice`
- (plus 2 more with this as secondary issue)

**Symptom**:
```
Actual:   "His Anonymous Life".
Expected: "His Anonymous Life."
```

**Root cause**: When `punctuation-in-quote` is enabled in the locale, punctuation (period, comma) following quoted text should move inside the closing quote.

**Location to fix**: `crates/quarto-citeproc/src/output.rs` - look for quote handling and punctuation collision rules.

---

### 2. Year-Suffix Disambiguation (5 tests) - HIGH PRIORITY

**Tests**:
- `bugreports_collapsefailure`
- `bugreports_disambighang`
- `bugreports_disambiguationaddnames`
- `bugreports_disambiguationaddnamesbibliography`
- `bugreports_baddelimiterbeforecollapse` (partial)

**Symptom**:
```
Actual:   (Doe et al. 1965, 1965)
Expected: (Doe et al. 1965a, 1965b)
```

**Root cause**: The disambiguation system isn't assigning year suffixes (a, b, c) when needed to distinguish works by the same author in the same year.

**Location to fix**: `crates/quarto-citeproc/src/disambiguation.rs`

---

### 3. Page Range En-Dash (4 tests) - MEDIUM PRIORITY, LOW EFFORT

**Tests**:
- `bugreports_contextualpluralwithmainitemfields`
- `bugreports_ieeepunctuation`
- `bugreports_numberinmacrowithverticalalign`
- `bugreports_parenthesis`

**Symptom**:
```
Actual:   339-351
Expected: 339–351  (en-dash, not hyphen)
```

**Root cause**: Page ranges should use en-dash (–, U+2013) not hyphen-minus (-, U+002D).

**Location to fix**: `crates/quarto-citeproc/src/eval.rs` - look for page/number formatting.

**CSL Spec**: The `page-range-format` option controls this. Even without explicit format, ranges should use en-dash.

---

### 4. Missing Final Periods (3 tests)

**Tests**:
- `bugreports_duplicatespaces`
- `bugreports_singlequote`
- `bugreports_asmjournals`

**Symptom**: Bibliography entries end without terminal period.

---

### 5. French Locale Guillemets (3 tests)

**Tests**:
- `bugreports_allcapsleakage`
- `bugreports_frenchapostrophe`
- `bugreports_doubleencodedanglebraces`

**Symptom**:
```
Actual:   "text"
Expected: « text »
```

**Root cause**: French locales use guillemets (« ») instead of quotation marks.

---

### 6. Other Issues (Single Tests)

| Test | Issue |
|------|-------|
| `bugreports_etalsubsequent` | et-al-subsequent-min/use-first not working |
| `bugreports_capsafteronewordprefix` | First-word capitalization after prefix ending with period |
| `bugreports_numberaffixescape` | Unicode `ª` vs `<sup>a</sup>` |
| `bugreports_byby` | reviewed-title variable handling |
| `bugreports_creepingaddnames` | Multiple citation output issue |
| `bugreports_missingiteminjoin` | Extra semicolons between citations |
| `bugreports_notitle` | citation-label generation when no title |
| `bugreports_citationsortswithetal` | Citation sorting with et al. |
| `bugreports_delimitersonlocator` | Citation collapsing format |

---

## Recommended Fix Order

1. **Page range en-dash** (low effort, 4 tests)
2. **Punctuation-in-quote** (medium effort, 8 tests)
3. **Year-suffix disambiguation** (high effort, 5 tests)
4. **Missing final periods** (low effort, 3 tests)
5. **French locale guillemets** (medium effort, 3 tests)

---

## Related Issues

- Parent issue: k-422 (CSL conformance)
- Related: k-428 (name formatting edge cases)

## Progress

- [x] Initial analysis complete
- [x] Page range en-dash fix (k-457, CLOSED) - 14 tests enabled
- [x] Punctuation-in-quote fix (k-458, CLOSED) - 4 tests enabled
- [ ] Year-suffix disambiguation fix (k-459, OPEN)

## Session Notes

See `claude-notes/plans/2025-11-29-session-handoff-notes.md` for detailed technical notes from the 2025-11-29 session, including:
- Files modified
- Technical insights learned
- Recommended approach for disambiguation work
- Useful commands
