# CSL Conformance Roadmap

**Parent Issue**: k-422 (CSL conformance testing)
**Created**: 2025-11-27
**Status**: In Progress

## Important Implementation Notes

### Reference Implementation

**ALWAYS consult `external-sources/citeproc` (Pandoc's Haskell citeproc) as the reference implementation.** Do NOT consult citeproc-js via web searches. The Pandoc citeproc is the authoritative reference for this project.

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

## Current State

- 314/896 tests passing (~35.0%)
- Output AST architecture complete (k-423)
- Name formatting inheritance complete
- et-al-use-last implemented
- Many additional name tests enabled
- Bibliography sorting (basic) implemented
- Citation sorting implemented
- Reference insertion order preserved (using LinkedHashMap)
- Citation-number sorting (basic) implemented

## Test Coverage Analysis

| Category | Enabled | Total | Gap | Coverage |
|----------|---------|-------|-----|----------|
| name | 14 | 111 | 97 | 12.6% |
| nameattr | 16 | 97 | 81 | 16.5% |
| date | 26 | 101 | 75 | 25.7% |
| bugreports | 9 | 83 | 74 | 10.8% |
| disambiguate | 5 | 72 | 67 | 6.9% |
| sort | 4 | 66 | 62 | 6.1% |
| magic | 1 | 40 | 39 | 2.5% |
| textcase | 1 | 31 | 30 | 3.2% |
| collapse | 0 | 21 | 21 | 0% |
| flipflop | 0 | 19 | 19 | 0% |
| condition | 10 | 17 | 7 | 58.8% |

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
