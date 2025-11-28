# CSL Conformance Roadmap

**Parent Issue**: k-422 (CSL conformance testing)
**Created**: 2025-11-27
**Status**: In Progress

## Important Implementation Notes

### Reference Implementation

**ALWAYS consult `external-sources/citeproc` (Pandoc's Haskell citeproc) as the reference implementation.** Do NOT consult citeproc-js via web searches. The Pandoc citeproc is the authoritative reference for this project.

When an implementation or a bugfix seems particularly challenging, consider studying the reference implementation alongside quarto-citeproc to identify architectural differences. Our goal is to get to the same level of CSL Conformance as citeproc, and its implementation is a good guide.

Key files in `external-sources/citeproc/src/`:
- `Citeproc/Types.hs` - Core types including `Output a`, `CiteprocOutput` typeclass
- `Citeproc/Eval.hs` - Evaluation logic including collapse, disambiguation
- `Citeproc/CslJson.hs` - HTML output format for CSL test suite
- `Citeproc/Pandoc.hs` - Pandoc Inlines output format

### Output Format Architecture

Pandoc's citeproc uses a **format-agnostic design**:

1. `Output a` - Parameterized AST where `a` is the output type
2. `CiteprocOutput` typeclass - Defines formatting operations (`addFontWeight`, `addFontStyle`, etc.)
3. **Two implementations provided:**
   - `CslJson Text` - Renders to HTML (`<b>`, `<i>`, `<sup>`, etc.) - **used by CSL test suite**
   - `Inlines` - Renders to Pandoc AST - used for Pandoc integration

**The CSL test suite expects HTML output** because the test harness uses `CslJson Text`.
See `external-sources/citeproc/src/Citeproc/CslJson.hs:renderCslJson` for the HTML rendering.

Our current implementation hardcodes markdown output (`**bold**`, `*italic*`), which is why
some tests fail with output like `(**[1]–[3]**)` instead of `<b>([1]–[3])</b>`.

**TODO**: Implement format-agnostic output rendering:
- Add an output format enum/trait
- For tests: render to HTML to match test expectations
- For Quarto: render to Pandoc AST or appropriate format

### CSL Specification Reference

Detailed CSL spec documentation has been created in `claude-notes/csl-spec/`:
- `00-index.md` - Overview and navigation
- `01-data-model.md` - CSL data model (references, variables)
- `02-rendering-elements.md` - Text, number, names, dates, groups
- `03-names.md` - Name formatting details
- `04-disambiguation.md` - Disambiguation algorithm (critical for k-444)
- `05-sorting.md` - Bibliography and citation sorting
- `06-localization.md` - Locale handling
- `07-formatting.md` - Font styles, text-case, affixes

These documents are cross-referenced with the original spec at `external-sources/csl-spec/specification.rst`.

## Test Organization

### Test Files

- `tests/enabled_tests.txt` - Tests that are expected to pass (one test name per line)
- `tests/deferred_tests.txt` - Tests intentionally set aside with documented reasons

### Deferred Tests Policy

**IMPORTANT**: Adding tests to `deferred_tests.txt` requires user approval. These are tests we've decided to intentionally skip, not just "not yet attempted" tests.

Before proposing to defer a test:
1. Investigate the test thoroughly
2. Understand why it fails and what would be needed to fix it
3. Ask the user for approval, explaining the rationale

Valid reasons for deferring:
- CSL style quirk that produces technically-correct but undesirable output
- Edge case that would require disproportionate effort relative to its value
- Test that conflicts with other more important behaviors

Format in `deferred_tests.txt`:
```
# Date and reason for deferring
# Explanation of why this test is deferred
test_name
```

## Current State

- **501 enabled tests passing (58.4% coverage)** - Updated 2025-11-28
- Position tracking implemented (first, subsequent, ibid, ibid-with-locator, near-note)
- Unicode curly quotes for locale rendering
- Sentence-initial capitalization for citation terms (ibid, etc.)
- Variable `form="short"` support (title-short, container-title-short with journalAbbreviation fallback)
- k-444 infrastructure (in progress) - added delimiter field to Formatting struct, smart punctuation handling, substitute context inheritance
- Up from 377 after k-443 multi-pass disambiguation - properly re-renders and refreshes ambiguities after each disambiguation step; year suffixes now correctly skip items already disambiguated by givenname expansion
- Up from 358 after k-443 (integration fixes) - fixed layout delimiter bug (was applying layout delimiter between elements within a layout, should only join citation items)
- Up from 346 after k-442 (disambiguation condition fix) - always detect ambiguities for `<if disambiguate="true">`
- Up from 346 after given name disambiguation for all-names rule (proper global disambiguation)
- Up from 405 after Output AST-based disambiguation refactoring (extracts names from Tag::Names)
- Up from 402 after disambiguation Phase 4 (k-441) - name disambiguation infrastructure
- Up from 394 after disambiguation Phase 1-3 (k-438, k-439, k-440)
- Up from 348 at start of 2025-11-28 session
- Up from 343 after k-434 (date era formatting) fix
- Up from 332 after k-430, k-431, k-432, k-433 fixes
- Output AST → Pandoc Inlines pipeline complete (k-429)
- Output AST architecture complete (k-423)
- Name formatting inheritance complete
- et-al-use-last implemented
- Many additional name tests enabled
- Bibliography sorting (basic) implemented
- Citation sorting implemented
- Reference insertion order preserved (using LinkedHashMap)
- Citation-number sorting (basic) implemented
- Negative year (BC) and ancient year (AD) formatting implemented
- Smart date range collapsing implemented
- Season-as-month substitution implemented
- Raw/literal date fallback implemented
- Day ordinal limiting (`limit-day-ordinals-to-day-1`) implemented

## Recent Progress (2025-11-28 Session)

### Date Test Improvements (+46 tests)

1. **Made `type` field optional** in Reference struct - defaults to empty string, matching Pandoc citeproc behavior

2. **Fixed UTF-8 BOM handling** in test parser - strips BOM before parsing test files

3. **Season substitution** - When a date has `season` but no month in date-parts, substitute season as pseudo-month (21-24)
   - Enabled: `date_OtherWithDate`, `date_SeasonSubstituteInGroup`

4. **Raw/literal date fallback** - When `date-parts` is empty but `raw` field is present, use raw string
   - Enabled: `date_String`

5. **Smart date range collapsing** - Suppress repeated parts in date ranges:
   - Same month+year: `10 August 2003–23 August 2003` → `10–23 August 2003`
   - Same year: `3 August–23 October 2003` (year not repeated)
   - Open ranges: `2003–` for ongoing dates
   - Enabled: `date_SeasonRange2`, `date_SeasonRange3`, `date_TextFormFulldateDayRange`, `date_TextFormFulldateMonthRange`, `date_TextFormMonthdateMonthRange`, `date_TextFormYeardateYearRangeOpen`

6. **Localized date formats** - All 39 locale-specific date format tests now pass
   - Enabled: `date_LocalizedDateFormats-*` (af-ZA through zh-TW)

7. **Day ordinal limiting** - `limit-day-ordinals-to-day-1="true"` style option
   - Only day 1 gets ordinal suffix ("1st"), other days are numeric ("2", "3")
   - Added `options` field to Locale struct for locale-level style option overrides
   - Enabled: `date_DayOrdinalDayOneOnly`

## Test Coverage Analysis (Updated 2025-11-28)

| Category | Enabled | Total | Gap | Coverage |
|----------|---------|-------|-----|----------|
| nameattr | 89 | 97 | 8 | 91% |
| condition | 15 | 17 | 2 | 88% |
| date | 83 | 101 | 18 | 82% |
| textcase | 23 | 31 | 8 | 74% |
| disambiguate | 43 | 72 | 29 | 59% |
| collapse | 12 | 21 | 9 | 57% |
| substitute | 4 | 7 | 3 | 57% |
| label | 10 | 19 | 9 | 52% |
| sort | 30 | 66 | 36 | 45% |
| name | 48 | 111 | 63 | 43% |
| locale | 9 | 23 | 14 | 39% |
| position | 6 | 16 | 10 | 37% |
| number | 6 | 20 | 14 | 30% |
| bugreports | 21 | 83 | 62 | 25% |
| affix | 2 | 9 | 7 | 22% |
| flipflop | 4 | 19 | 15 | 21% |
| magic | 2 | 40 | 38 | 5% |

### Remaining Date Tests (26 ignored)

- **Year suffix** tests - Part of disambiguation system (larger feature)
- Various edge cases and complex scenarios

## Roadmap

### Phase 1: Name Formatting Edge Cases (k-424)

**Goal**: Unlock 20-30 tests with incremental changes to `format_names()`

Missing features:
- `et-al-use-last` - "A, B, C, … Z" instead of "A, B, C, et al."
- `delimiter-precedes-last` variations - "A, B, and C" vs "A, B and C"
- `delimiter-precedes-et-al` - comma before "et al."
- Name form inheritance from style/citation/bibliography levels

Example test: `name_EtAlUseLast.txt`
- Input: 8 authors
- Expected: "John Anderson, John Brown, John Catharsis, John Doldrums, John Evergreen, John Fugedaboutit, … John Houynym"
- Requires: et-al-use-last="true" support

### Phase 2: Bibliography Sorting (k-425)

**Goal**: Unlock 62+ tests, foundation for proper bibliography output

**Completed**:
- ✅ Sort keys in `<bibliography><sort>` element
- ✅ Macro evaluation for sort keys
- ✅ Multi-level sorting (primary, secondary, tertiary keys)
- ✅ Ascending/descending order
- ✅ Case-insensitive sorting with bracket normalization
- ✅ Reference insertion order preservation (using LinkedHashMap)
- ✅ Citation sorting (`<citation><sort>`) - sort items within a single citation
- ✅ HTML markup stripping for sort comparison

**Remaining**:
- Citation-number variable sorting (complex - involves reassignment)
- Year suffix assignment (1989a, 1989b for disambiguation)
- Complex sort tests like `sort_AguStyle.txt`

Example test: `sort_AguStyle.txt`
- Complex multi-key sorting by author name, author count, year
- Also involves year suffixes (1989a, 1989b)

### Phase 3: Citation Collapsing (k-426)

**Goal**: Use Output AST tags for citation grouping

Required features:
- `collapse="year"` - "(Smith 1900, 2000)" instead of "(Smith 1900, Smith 2000)"
- `collapse="year-suffix"` - "(Smith 2020a, b)"
- `collapse="citation-number"` - "[1-3]" instead of "[1, 2, 3]"

Uses `Tag::Names` and `Tag::Date` from Output AST.

Example test: `collapse_AuthorCollapse.txt`
- Input: Two citations from Smith (1900, 2000)
- Expected: "(Smith 1900, 2000)"

### Phase 4: Full Disambiguation (k-427)

**Goal**: Complete disambiguation algorithm

Required features:
- `disambiguate-add-givenname` - Add initials to distinguish "J. Smith" vs "A. Smith"
- `disambiguate-add-year-suffix` - Add "a", "b" to years
- `givenname-disambiguation-rule` - Various strategies
- Name expansion strategies

Uses `Tag::Names` and `Tag::Date` from Output AST.
Depends on sorting being complete (year suffixes assigned in sort order).

### Phase 5: Multi-Pass Rendering Architecture (k-444)

**Goal**: Refactor to match Pandoc's citeproc architecture for proper delimiter handling and disambiguation.

See detailed design: `claude-notes/plans/2025-11-28-multi-pass-rendering-architecture.md`

**Completed work (2025-11-28)**:
- [x] Added `delimiter` field to `Formatting` struct (`quarto-csl/src/types.rs`)
- [x] Added `Output::formatted_with_delimiter()` helper method
- [x] Implemented `fix_punct()` - smart punctuation collision handling (based on Pandoc citeproc)
- [x] Implemented `join_with_smart_delim()` - delimiter insertion with punctuation awareness
- [x] Updated `Output::render()` to use smart punctuation handling
- [x] Added `substitute_name_options` and `in_substitute` to `EvalContext`
- [x] Updated `evaluate_names()` to pass substitute context through
- [x] Updated `format_names()` to inherit parent names options in substitute blocks
- [x] Updated `evaluate_elements()` to use `formatted_with_delimiter`
- [x] Updated citation collapse functions (`collapse_by_year`, `collapse_by_citation_number`) to use `formatted_with_delimiter`
- [x] All 380 enabled tests continue to pass

**Remaining work**:
- [ ] Implement `CslRenderer` trait for format-agnostic output (optional)
- [ ] Further investigation: remaining tests fail due to other issues (title-case, quote handling, moving punctuation, etc.)

**Expected impact**: Unlock 80-150 additional tests by fixing:
- Delimiter bugs (~20-30 tests)
- Substitute inheritance (~50-100 tests)
- Year-suffix with multi-pass (~20-30 tests)

### Phase 6: Locale Post-Processing Pipeline (NEW)

**Goal**: Implement a post-processing pipeline matching Pandoc citeproc's architecture for locale-specific text transformations.

**Architectural Pattern** (from `external-sources/citeproc/src/Citeproc.hs`):

```haskell
-- Citeproc.hs lines 50-52
movePunct = case localePunctuationInQuote locale of
              Just True -> movePunctuationInsideQuotes
              _         -> id
```

The reference implementation applies these transformations **after** rendering:
1. `localizeQuotes` - Convert generic quotes to locale-specific characters
2. `movePunctuationInsideQuotes` - Move punctuation inside quotes when `punctuation-in-quote="true"`

**Key architectural insight**: The `CiteprocOutput` typeclass (Types.hs:212-216) defines these as trait methods:
- `addQuotes :: a -> a`
- `movePunctuationInsideQuotes :: a -> a`
- `localizeQuotes :: Locale -> a -> a`

**Implementation plan**:

1. **Quote Localization** (~15-20 tests)
   - Add locale term lookup for: `open-quote`, `close-quote`, `open-inner-quote`, `close-inner-quote`
   - Track nesting depth to flip between outer/inner quotes
   - Currently: hardcoded `"` `"` `'` `'`
   - Test: `affix_CommaAfterQuote` - Expected `"quote"` got `'quote'`

2. **Moving Punctuation** (~10-15 tests)
   - Parse `punctuation-in-quote` locale option
   - When true, move `,` `.` inside closing quotes
   - Test: `magic_StripPeriodsFalse` - Expected `"Article,"` got `"Article, "`

3. **Display Attribute** (~30-40 tests) - See Phase 7
   - Render `display` attribute as `<div class="csl-{value}">`
   - Values: `block`, `left-margin`, `right-inline`, `indent`
   - Many bibliography tests expect this HTML structure

**Files to modify**:
- `crates/quarto-citeproc/src/output.rs` - Add post-processing functions
- `crates/quarto-citeproc/src/locale.rs` - Add quote term lookup
- `crates/quarto-csl/src/types.rs` - Add `punctuation_in_quote` to locale options

### Phase 7: Display Attribute Support

**Goal**: Render the `display` attribute for bibliography formatting.

**Expected output** (from test `variables_ContainerTitleShort.txt`):
```html
<div class="csl-entry">
  <div class="csl-left-margin">1. </div><div class="csl-right-inline">Content...</div>
</div>
```

**Implementation**:
1. Parse `display` attribute in Formatting (may already be partially done)
2. Add `DisplayStyle` enum: `Block`, `LeftMargin`, `RightInline`, `Indent`
3. Render as `<div class="csl-{style}">` wrapper in CSL HTML output

**Expected impact**: ~30-40 tests (bugreports, sort, other categories)

## Files to Modify

1. `crates/quarto-citeproc/src/eval.rs` - Name formatting, sorting
2. `crates/quarto-csl/src/types.rs` - Parse new attributes
3. `crates/quarto-csl/src/parser.rs` - Parse new attributes
4. `crates/quarto-citeproc/src/types.rs` - Processor sorting methods

## Progress Tracking

- [x] Phase 1a: Name attribute inheritance (style → citation/bibliography → names → name)
  - Added `InheritableNameOptions` with merge semantics
  - Added style-level name options parsing
  - Fixed `Name.form` to be `Option<NameForm>` for proper inheritance
  - Fixed `et-al-use-first` logic to require both min and use-first
  - Fixed hyphenated name initialization (e.g., "John-Lee" → "J.-L.")
  - **Result**: 220 tests passing (up from 155)
- [x] Phase 1b: et-al-use-last implementation
  - Implemented "A, B, C, … Z" format for et-al-use-last="true"
  - Fixed: don't use "and" connector when et-al-use-last is active
  - Enabled 35 additional passing tests
  - **Result**: 257 tests passing (up from 220)
- [x] Phase 1c: Additional name formatting edge cases
  - Fixed only-given-name handling (don't initialize single-name people like "Banksy")
  - Added `initialize` attribute support (`initialize="false"` prevents initialization)
  - Fixed extra space before institution names
  - Fixed `delimiter-precedes-last="after-inverted-name"` for literal names (institutions)
  - Enabled 5 additional passing tests
  - **Result**: 262 tests passing (up from 257)
- [x] Phase 2: Bibliography and citation sorting
  - Implemented sort key extraction from CSL `<sort>` element
  - Support for variable sort keys (author, title, dates, etc.)
  - Support for macro sort keys (evaluate macro, use result as sort value)
  - Case-insensitive sorting with bracket and HTML tag stripping
  - Ascending/descending order
  - Updated test harness to use `generate_bibliography()` for sorted output
  - Refactored to use `LinkedHashMap` for cleaner insertion order preservation
  - Implemented citation item sorting (`<citation><sort>`)
  - Enabled additional name attribute tests (et-al-subsequent-min, et-al-subsequent-use-first)
  - Enabled 26 additional passing tests total
  - **Result**: 288 tests passing (up from 262)
- [x] Phase 2b: Citation-number sorting
  - Implemented two-phase citation number assignment:
    - Initial numbers assigned during citation processing (for sorting)
    - Final numbers reassigned after bibliography sorting (for rendering)
  - Fixed `normalize_for_sort()` to preserve alphanumeric characters (was stripping leading digits)
  - Updated test harness to process citations before generating bibliography
  - Fixed macro evaluation for sort to copy citation numbers from parent processor
  - Smart reassignment: only reassign when multiple sort keys or citation-number is secondary
  - **Result**: 295 tests passing (up from 288)
  - **Remaining**: Complex macro-based citation-number tests, citation mode tests, year suffix
- [x] Phase 2c: Condition evaluation fixes
  - Fixed `match="all"` for multi-value conditions (e.g., `variable="title edition"`)
  - Passed match_type to evaluate_condition for proper all/any/none semantics
  - Fixed integer ID parsing in references (CSL-JSON allows both string and integer IDs)
  - Fixed integer ID parsing in citation-items (test harness was skipping integer IDs)
  - Enabled additional passing tests
  - **Result**: 309 tests passing (up from 295)
- [x] Phase 3: Citation collapsing (basic)
  - Added `Collapse` enum and related attributes to Layout
  - Implemented `collapse="year"` - groups by author, suppresses repeated names
  - Implemented `collapse="citation-number"` - detects consecutive ranges
  - Added Output AST helpers: `extract_names_text()`, `suppress_names()`, `extract_citation_number()`
  - Added `Tag::CitationNumber` tagging for citation number rendering
  - Enabled 5 new collapse tests (7 total, 2 were already passing)
  - **Result**: 314 tests passing (up from 309)
  - **Remaining**: Year-suffix collapse (requires disambiguation), affixes with collapse (HTML output format)
- [ ] Phase 4: Full disambiguation

## Discovered Issues (2025-11-27)

Analysis of 582 ignored tests revealed these blocking issues. See detailed analysis in
`claude-notes/plans/2025-11-27-csl-failing-test-analysis.md`.

## Remaining Test Analysis (2025-11-28)

Updated analysis of 478 remaining failing tests: `claude-notes/plans/2025-11-28-failing-test-analysis.md`

**Current Status: 390/858 tests passing (45.5%)**

**Completed implementations:**
1. ✅ **Title Case Transformation** (10 new tests) - Stop words, colons, hyphens, quotes, slashes
   - Implemented proper English title case with stop words list
   - First word and words after colons/dashes always capitalized
   - Hyphenated compounds: first and last parts capitalized, middle follows stop word rules
   - Opening quotes (curly and straight) trigger capitalization
   - ALL-CAPS words and words with internal caps preserved

**Priority order for next implementations:**
1. **Moving Punctuation** (~15-20 tests) - CSL punctuation exchange rules
2. **Citation Position** (~15-20 tests) - ibid, near-note detection
3. **Flip-Flop Formatting** (~15-20 tests) - Nested formatting flip

| Issue | Priority | Tests Affected | Description |
|-------|----------|----------------|-------------|
| k-430 | P2 | collapse_*, formatting | ✅ FIXED: Prefix/suffix ordering - now inside formatting for layout |
| k-431 | P2 | flipflop_*, textcase_* | ✅ FIXED: HTML markup in CSL-JSON now parsed (5/6 flipflop cases pass; remaining needs k-432) |
| k-432 | P3 | 19 flipflop_* tests | ✅ FIXED: Flip-flop formatting with CslRenderContext (2 tests pass, others need title-case fixes) |
| k-433 | P3 | textcase_* tests | ✅ FIXED: nocase span support, quote escaping, whitespace in capitalize_all (6 new tests) |
| k-434 | P3 | date_Negative* | ✅ FIXED: Date era formatting (BC/AD for negative years, sort key adjustment for chronological order) |

### Recommended Implementation Order

1. **k-430**: Prefix/suffix ordering - architectural fix, unlocks many tests
2. **k-431**: HTML in CSL-JSON - common pattern, blocks multiple categories
3. **k-433**: Quote escaping - quick fix for some text case tests
4. **k-432**: Flip-flop formatting - enables 19 tests
5. **k-434**: Date era formatting - isolated feature
