# CSL Test Categorization Report

**Date**: 2025-12-01
**Status**: 70 unknown tests remaining (82.2% enabled, 9.7% deferred, 8.2% unknown)

## Summary

| Category | Count | Effort | Priority |
|----------|-------|--------|----------|
| Quick Wins | 2 | None | Immediate |
| Citation Sequence/Incremental | 6 | High | Tier 4 |
| Subsequent Author Substitute | 2 | Medium | Tier 3 |
| Collapse/Delimiter Handling | 3 | Medium | Tier 3 |
| Quote/Punctuation Positioning | 6 | Medium | Tier 3 |
| Group/Variable Suppression | 3 | Medium | Tier 3 |
| Disambiguation/Year Suffix | 4 | High | Tier 4 |
| Names-Delimiter Inheritance | 4 | Low | Tier 2 |
| Sort Order/Particles | 8 | Medium | Tier 3 |
| Locale/Text-Case | 6 | Medium | Tier 3 |
| Locator Handling | 3 | Low | Tier 2 |
| Display Attribute | 1 | Low | Tier 2 |
| Term/Label Handling | 4 | Medium | Tier 3 |
| Et-Al Subsequent | 2 | Low | Tier 2 |
| Misc Punctuation | 4 | Medium | Tier 3 |
| Variables | 3 | Low | Tier 2 |
| Name Suffix/Particle | 3 | Medium | Tier 3 |
| Remaining Bugreports | 5 | Varies | Tier 3-4 |

---

## CATEGORY 1: Quick Wins (2 tests)

Tests that currently pass but aren't in `enabled_tests.txt`:

| Test | Notes |
|------|-------|
| `magic_SubsequentAuthorSubstituteOfTitleField` | Already passing |
| `substitute_SharedMacro` | Already passing |

**Action**: Enable immediately.

---

## CATEGORY 2: Citation Sequence / Incremental Processing (6 tests)

Tests using `>>` vs `..` notation for incremental citation updates. Failing because we output all intermediate states:

| Test | Issue |
|------|-------|
| `bugreports_CreepingAddNames` | Shows all 3 citation states instead of final |
| `integration_DuplicateItem` | Shows all citation states |
| `integration_DuplicateItem2` | Same |
| `integration_DisambiguateAddGivenname1` | Shows multiple states |
| `integration_DisambiguateAddGivenname2` | Same |
| `integration_YearSuffixOnOffOn` | Incremental processing |

**Root cause**: The `CITATIONS` block test format processes multiple citation updates incrementally. We're outputting all states instead of final.

**Effort**: High - requires understanding the CITATIONS test format and incremental citation processing.

---

## CATEGORY 3: Subsequent Author Substitute (2 tests)

Related to "---" substitution for repeated authors in bibliographies:

| Test | Issue |
|------|-------|
| `magic_SubsequentAuthorSubstitute` | Extra ", and," being inserted when substituting |
| `sort_AguStyle` | Year suffix incorrectly applied after substitution |

**Root cause**: The `subsequent-author-substitute` feature has a bug with remaining delimiter/conjunction text.

**Example**:
```
Expected: ———, Book B (2001)
Actual:   ———, and, Book B (2001)
```

---

## CATEGORY 4: Collapse / Delimiter Handling (3 tests)

Citation collapsing and delimiter handling:

| Test | Issue |
|------|-------|
| `collapse_ChicagoAfterCollapse` | Wrong delimiter "," instead of ";" between author groups |
| `collapse_TrailingDelimiter` | Wrong delimiter between different author groups |
| `collapse_CitationNumberRangesWithAffixesGroupedLocator` | Locator handling in collapsed ranges |

**Root cause**: `after-collapse-delimiter` attribute not being applied correctly.

**Example** (`collapse_ChicagoAfterCollapse`):
```
Expected: (Whittaker 1967, 1975; Wiens 1989a, 1989b)
Actual:   (Whittaker 1967, 1975, Wiens 1989a, 1989b)
```

---

## CATEGORY 5: Quote / Punctuation Positioning (6 tests)

Moving punctuation inside/outside quotes:

| Test | Issue |
|------|-------|
| `decorations_NestedQuotes` | Quote nesting order wrong: `"'` vs `'"` |
| `decorations_NestedQuotesInnerReverse` | Same issue |
| `decorations_Baseline` | Font decoration reset issue |
| `decorations_NoNormalWithoutDecoration` | Font reset behavior |
| `quotes_PunctuationWithInnerQuote` | Punctuation position |
| `bugreports_ThesisUniversityAppearsTwice` | Period inside vs outside quotes |

**Root cause**: Quote nesting depth tracking, punctuation-in-quote rules.

**Example** (`decorations_NestedQuotes`):
```
Expected: "My '"Amazing" <b>and</b> Bogus' Title"
Actual:   "My "'Amazing' <b>and</b> Bogus" Title"
```

---

## CATEGORY 6: Group/Variable Suppression (3 tests)

Suppressing groups containing empty variables or preventing duplicate rendering:

| Test | Issue |
|------|-------|
| `group_SuppressValueWithEmptySubgroup` | Extra ", Disponible sur :" should be suppressed |
| `substitute_SuppressOrdinaryVariable` | Duplicate variable rendering (double names) |
| `magic_SuppressDuplicateVariableRendering` | "BlobBlob" instead of "Blob" |

**Root cause**: Variable suppression tracking not working across macros/substitutes.

**Example** (`magic_SuppressDuplicateVariableRendering`):
```
Expected: ... but this only once: Blob
Actual:   ... but this only once: BlobBlob
```

---

## CATEGORY 7: Disambiguation / Year Suffix (4 tests)

Year suffix and name disambiguation:

| Test | Issue |
|------|-------|
| `disambiguate_BasedOnEtAlSubsequent` | Year suffix not added to et al references |
| `disambiguate_BasedOnSubsequentFormWithBackref2` | Backref disambiguation |
| `disambiguate_ByCiteIncremental2` | Incremental disambiguation |
| `disambiguate_WithOriginalYear` | Original year variable handling |

**Root cause**: Complex disambiguation scenarios with et-al and subsequent forms.

**Example** (`disambiguate_BasedOnEtAlSubsequent`):
```
Expected: (Baur, Fröberg, Baur, et al. 2000a; Baur, Schileyko & Baur 2000b; Doe 2000)
Actual:   (Baur, Fröberg, Baur, et al. 2000; Baur, Schileyko, Baur, et al. 2000; Doe 2000)
```

---

## CATEGORY 8: Names-Delimiter Inheritance (4 tests)

`names-delimiter` attribute inheritance from style/citation/bibliography:

| Test | Issue |
|------|-------|
| `nameattr_NamesDelimiterOnBibliographyInBibliography` | Missing "AND ALSO THE EDITOR" between names |
| `nameattr_NamesDelimiterOnCitationInCitation` | Same |
| `nameattr_NamesDelimiterOnStyleInBibliography` | Same |
| `nameattr_NamesDelimiterOnStyleInCitation` | Same |

**Root cause**: `names-delimiter` attribute not being inherited from parent elements.

**Example** (`nameattr_NamesDelimiterOnBibliographyInBibliography`):
```
Expected: John Doe AND ALSO THE EDITOR Jane Roe
Actual:   John DoeJane Roe
```

**Effort**: Low - attribute propagation fix.

---

## CATEGORY 9: Sort Order / Particles (8 tests)

Bibliography sorting with name particles:

| Test | Issue |
|------|-------|
| `sort_NameParticleInNameSortTrue` | Sorting "di", "van", "von" particles incorrectly |
| `sort_AguStyleReverseGroups` | Reverse grouping |
| `sort_CitationNumberSecondaryAscendingViaMacroBibliography` | Secondary sort by citation-number |
| `sort_CitationNumberSecondaryAscendingViaMacroCitation` | Same |
| `sort_CitationNumberSecondaryAscendingViaVariableCitation` | Same |
| `sort_DropNameLabelInSort` | Label should be dropped when sorting |
| `sort_SeparateAuthorsAndOthers` | "et al." handling in sort |
| `sort_WithAndInOneEntry` | "and" in single author name |

**Root cause**: `demote-non-dropping-particle` and sort key generation issues.

**Example** (`sort_NameParticleInNameSortTrue`):
```
Expected order: di Noakes, van Roe, von Doe
Actual order:   von Doe, di Noakes, van Roe
```

---

## CATEGORY 10: Locale / Text-Case (6 tests)

Locale-specific behavior:

| Test | Issue |
|------|-------|
| `locale_ForceEmptyAndOthersTerm` | "et al." rendered instead of empty |
| `locale_NonExistentLocaleDef` | Non-existent locale handling |
| `locale_OverloadWithEmptyString` | Empty string locale override |
| `locale_TitleCaseEmptyLangNonEnglishLocale` | Title case with non-English locale |
| `textcase_LocaleUnicode` | Turkish dotted İ uppercase |
| `textcase_NonEnglishChars` | Non-ASCII character case transformations |

**Root cause**: Locale term override with empty strings, Unicode case mapping.

**Example** (`textcase_LocaleUnicode`):
```
Expected: İC AND ID TR  (Turkish dotted I)
Actual:   IC AND ID TR  (regular I)
```

---

## CATEGORY 11: Locator Handling (3 tests)

Locator formatting:

| Test | Issue |
|------|-------|
| `locator_SimpleLocators` | En-dash vs hyphen in ranges ("200 - 201" vs "200–201") |
| `locator_TrickyEntryForPlurals` | Plural detection |
| `locator_WithLeadingSpace` | Leading space handling |

**Root cause**: Page range en-dash normalization in locators.

**Example** (`locator_SimpleLocators`):
```
Expected: chaps. 200–201
Actual:   chaps. 200 - 201
```

**Effort**: Low - string replacement.

---

## CATEGORY 12: Display Attribute (1 test)

Display attribute (block, left-margin, right-inline):

| Test | Issue |
|------|-------|
| `display_AuthorAsHeading` | Missing newlines around `display="block"` elements |

**Root cause**: HTML whitespace/newline formatting for display:block.

**Effort**: Low - HTML formatting tweak.

---

## CATEGORY 13: Term/Label Handling (4 tests)

CSL terms and labels:

| Test | Issue |
|------|-------|
| `bugreports_ByBy` | Missing "by" term before author |
| `label_NameLabelThroughSubstitute` | Label in substitute |
| `name_CollapseRoleLabels` | "eds. & trans." vs separate labels |
| `name_EditorTranslatorSameWithTerm` | Combined editor-translator term |

**Root cause**: Term rendering and label collapse logic.

**Example** (`name_CollapseRoleLabels`):
```
Expected: Albert Asthma et al. (eds. & trans.)
Actual:   Albert Asthma et al. (eds.), Albert Asthma et al. (trans.)
```

---

## CATEGORY 14: Et-Al Subsequent (2 tests)

Et-al behavior on subsequent citations:

| Test | Issue |
|------|-------|
| `bugreports_EtAlSubsequent` | Full names instead of et al on subsequent |
| `bugreports_CitationSortsWithEtAl` | Sorting with et al |

**Root cause**: `et-al-subsequent-min/use-first` attributes not being applied.

**Example** (`bugreports_EtAlSubsequent`):
```
Expected: >>[1] John Doe et al.
Actual:   >>[1] John Doe, Jane Roe, Katie Harper, Emmanuel Clutterbuck
```

**Effort**: Low - check attribute handling.

---

## CATEGORY 15: Miscellaneous Punctuation (4 tests)

Various punctuation issues:

| Test | Issue |
|------|-------|
| `bugreports_DoubleEncodedAngleBraces` | Space before final punctuation (`. .` vs `..`) |
| `bugreports_DelimitersOnLocator` | Delimiter handling |
| `punctuation_OnMacro` | Punctuation in macro output |
| `punctuation_SuppressPrefixPeriodForDelimiterSemicolon` | Period suppression |

---

## CATEGORY 16: Variables (3 tests)

Variable handling:

| Test | Issue |
|------|-------|
| `variables_ContainerTitleShort` | container-title-short not rendering |
| `variables_ContainerTitleShort2` | Same |
| `bugreports_MissingItemInJoin` | Missing variable in join |

**Root cause**: `container-title-short` variable not being resolved.

**Effort**: Low - add variable resolution.

---

## CATEGORY 17: Name Suffix/Particle (3 tests)

Name suffix comma handling:

| Test | Issue |
|------|-------|
| `magic_NameSuffixNoComma` | Suffix comma removal |
| `magic_NameSuffixWithComma` | Suffix comma preservation |
| `magic_ImplicitYearSuffixExplicitDelimiter` | Year suffix delimiter |

---

## CATEGORY 18: Remaining Bugreports (5 tests)

| Test | Likely Issue |
|------|--------------|
| `bugreports_NoTitle` | Title suppression |
| `bugreports_OldMhraDisambiguationFailure` | MHRA style disambiguation |
| `date_VariousInvalidDates` | Season date parsing ("Spring") |
| `integration_SimpleFirstReferenceNoteNumber` | first-reference-note-number tracking |
| `integration_DeleteName` | Citation deletion |

---

## Recommended Work Order

### Tier 1: Immediate (2 tests)
Enable quick wins - no code changes needed.

### Tier 2: Low-Hanging Fruit (~15-20 tests)
1. **Names-delimiter inheritance** (4 tests) - attribute propagation
2. **Locator en-dash normalization** (3 tests) - simple string replacement
3. **Et-al subsequent** (2 tests) - check `et-al-subsequent-min/use-first`
4. **Display block newlines** (1 test) - HTML formatting tweak
5. **Container-title-short variable** (2-3 tests) - add variable resolution

### Tier 3: Medium Effort (~20 tests)
- Collapse/delimiter handling (3 tests)
- Group/variable suppression (3 tests)
- Quote nesting (4-6 tests)
- Sort particles (8 tests)
- Term/label handling (4 tests)

### Tier 4: Complex / Consider Deferring (~15-20 tests)
- Citation sequence/incremental processing (6 tests)
- Subsequent author substitute bugs (2 tests)
- Complex disambiguation scenarios (4 tests)
- Locale edge cases (4-6 tests)

---

## Progress Log

- **2025-12-01**: Initial categorization of 70 unknown tests
