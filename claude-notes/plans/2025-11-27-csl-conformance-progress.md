# CSL Conformance Test Progress (k-422)

## Current Status

**Last Updated**: 2025-11-27
**Pass Rate**: 118/858 (13.8%)
**Unit Tests**: 22 passing

## Test Infrastructure

- `crates/quarto-citeproc/build.rs` - Generates test functions
- `crates/quarto-citeproc/tests/csl_conformance.rs` - Test harness
- `crates/quarto-citeproc/tests/enabled_tests.txt` - Passing test manifest

### Running Tests

```bash
# Run enabled tests only
cargo nextest run -p quarto-citeproc

# Run ALL tests (858 total)
cargo nextest run -p quarto-citeproc -- --include-ignored

# Run only pending/failing tests
cargo nextest run -p quarto-citeproc -- --ignored
```

## Implementation Priorities

### Priority 1: Full Locale Loading ✅ Completed
**Actual Impact**: +1 additional test (bugreports_abnt)

All 60 locale XML files are now embedded and parsed via `rust_embed`.

**Tasks**:
- [x] Add `rust_embed` dependency for embedding locale files
- [x] Parse locale XML files using quarto-xml (`locale_parser.rs`)
- [x] Implement full term lookup with form variants (long, short, verb, symbol)
- [x] All 60 locales verified to parse correctly
- [ ] Implement date formatting terms (month names in date rendering) - needs eval.rs changes
- [ ] Implement ordinal suffixes - needs eval.rs changes

**Remaining work**: Month names are now accessible via `get_term("month-01", ...)` etc., but the date evaluation code in `eval.rs` doesn't yet use them for rendering dates with months.

### Priority 2: Fix Default Name Order ⬜ Not Started
**Estimated Impact**: ~20-30 additional tests

Current implementation outputs "Family, Given" but CSL default is "Given Family".
The `name-as-sort-order` attribute controls this, but the default should be display order.

**Tasks**:
- [ ] Change default name rendering to "Given Family"
- [ ] Respect `name-as-sort-order="first"` and `name-as-sort-order="all"`
- [ ] Handle `sort-separator` attribute properly

**Tests to unlock**: name_* category (many)

### Priority 3: Full Date Formatting ✅ Completed
**Actual Impact**: +42 additional tests (total: 26 date tests passing)

Date formatting is now feature-complete for basic use cases.

**Tasks**:
- [x] Add `date_parts` field to DateElement in quarto-csl
- [x] Parse `date-parts` attribute in CSL parser
- [x] Implement month name lookup from locales
- [x] Implement day formatting (numeric, leading zeros, ordinal)
- [x] Eagerly load default locale in LocaleManager
- [x] Implement date ranges ("2020-2021", "June 15-17")
- [x] Implement seasons (months 21-24 map to season-01 through season-04)
- [ ] Implement approximate dates (circa) - lower priority

**Notes**:
- Date ranges work via `end_parts()` in DateVariable
- Seasons use months 21-24 which map to "season-01" through "season-04" terms
- Many other tests unlocked by this work (locale, name substitute, sort, etc.)

### Priority 4: Sorting Algorithm ⬜ Not Started
**Estimated Impact**: ~40-50 additional tests (prerequisite for disambiguation)

**Tasks**:
- [ ] Implement sort key evaluation from CSL `<sort>` element
- [ ] Implement multi-key sorting
- [ ] Handle ascending/descending
- [ ] Implement macro-based sort keys
- [ ] Handle missing values (sort after present values)

**Tests to unlock**: sort_* category

### Priority 5: Disambiguation Algorithm ⬜ Not Started
**Estimated Impact**: ~60-70 additional tests

Complex multi-phase algorithm. Requires sorting to work first.

**Tasks**:
- [ ] Phase 1: Add-names disambiguation
- [ ] Phase 2: Add-given-names disambiguation
- [ ] Phase 3: Year-suffix disambiguation (a, b, c)
- [ ] Phase 4: Conditional disambiguation flag

**Tests to unlock**: disambiguate_* category

### Priority 6: Position Tracking ⬜ Not Started
**Estimated Impact**: ~15-20 additional tests

**Tasks**:
- [ ] Track first/subsequent citation positions
- [ ] Implement ibid detection
- [ ] Implement near-note detection
- [ ] Update condition evaluation for position checks

**Tests to unlock**: position_* category

### Priority 7: Collapsing ⬜ Not Started
**Estimated Impact**: ~20 additional tests

**Tasks**:
- [ ] Implement citation number collapsing ([1-3])
- [ ] Implement year collapsing (Smith 2000a, b, c)
- [ ] Implement author collapsing

**Tests to unlock**: collapse_* category

## Passing Tests by Category

| Category | Passing | Total | Notes |
|----------|---------|-------|-------|
| affix | 1 | 9 | |
| bugreports | 9 | 83 | +3 from date work |
| collapse | 0 | 21 | Needs collapsing algorithm |
| condition | 10 | 17 | +1 (EmptyDate) |
| date | 26 | 101 | **+21 from ranges/seasons!** |
| decorations | 0 | 7 | |
| disambiguate | 5 | 72 | +3 from date work |
| display | 0 | 5 | |
| etal | 0 | 4 | |
| flipflop | 0 | 19 | Needs decoration tracking |
| form | 1 | 3 | |
| fullstyles | 0 | 5 | Complex integration |
| group | 0 | 7 | group_SuppressTermInMacro disabled |
| integration | 0 | 14 | |
| label | 1 | 19 | |
| locale | 7 | 23 | +3 (dates, terms) |
| locator | 0 | 6 | |
| magic | 1 | 40 | +1 (AllowRepeatDateRenderings) |
| name | 14 | 111 | +6 (substitute tests) |
| nameattr | 18 | 97 | Good coverage! |
| nameorder | 3 | 6 | |
| namespaces | 1 | 1 | Complete |
| number | 5 | 20 | |
| page | 0 | 10 | |
| plural | 0 | 7 | |
| position | 3 | 16 | |
| punctuation | 3 | 16 | +2 from date work |
| quotes | 0 | 4 | |
| sort | 4 | 66 | +2 (date-related) |
| sortseparator | 1 | 1 | Complete |
| substitute | 3 | 7 | |
| testers | 0 | 2 | |
| textcase | 1 | 31 | |
| unicode | 1 | 1 | Complete |
| variables | 2 | 5 | |
| virtual | 0 | 1 | |

## Session Log

### 2025-11-27 Session 4 - Date Ranges and Seasons

**Completed**:
- Implemented date ranges (e.g., "August 1987–October 2003")
- Implemented seasons (months 21-24 map to season-01 through season-04)
- Renamed `format_month` to `format_month_or_season` with season detection
- Added `render_date_parts` helper function for range rendering
- Made test manifest case-insensitive (updated build.rs)
- 42 new tests passing
- All 140 tests passing (22 unit + 118 conformance)

**Key Files Changed**:
- `crates/quarto-citeproc/src/eval.rs` - Date ranges, seasons, render_date_parts
- `crates/quarto-citeproc/build.rs` - Case-insensitive test manifest matching
- `tests/enabled_tests.txt` - Added 42 new passing tests

**New Test Categories Unlocked**:
- 21 more date tests (ranges, localized formats, seasons)
- 6 more name substitute tests
- 3 more disambiguate tests
- 3 more bugreports tests
- 3 more locale tests
- 2 more punctuation tests
- 2 more sort tests
- 1 magic test
- 1 condition test

### 2025-11-27 Session 3 - Date Formatting

**Completed**:
- Added `date_parts` field to `DateElement` in quarto-csl
- Parsed `date-parts` attribute in CSL parser
- Implemented full date evaluation with month names from locales
- Implemented day formatting (numeric, leading zeros, ordinal)
- Fixed locale loading to eagerly load default locale
- 4 new date tests passing
- Discovered group suppression bug exposed by locale loading
- All 98 tests passing (22 unit + 76 conformance)

**Key Files Changed**:
- `crates/quarto-csl/src/types.rs` - Added `DatePartsFilter` enum, `date_parts` field
- `crates/quarto-csl/src/parser.rs` - Parse `date-parts` attribute
- `crates/quarto-citeproc/src/eval.rs` - Rewrote `evaluate_date` function
- `crates/quarto-citeproc/src/locale.rs` - Eagerly load default locale
- `crates/quarto-citeproc/src/types.rs` - Added `get_date_format` method

**Bugs Found**:
- `group_SuppressTermInMacro` was passing incorrectly (term wasn't being found). Now exposes group suppression bug.

### 2025-11-27 Session 2 - Full Locale Loading

**Completed**:
- Added `rust_embed` dependency with `include-exclude` feature
- Created `locale_parser.rs` to parse locale XML files
- Integrated parser with `locale.rs` using embedded files
- All 60 locale XML files now load and parse correctly
- Added test verifying all locale files parse (`test_all_embedded_locales_parse`)
- Added tests for month names and German locale
- 1 new CSL test passing (`bugreports_Abnt`)
- All 95 tests passing (22 unit + 73 conformance)

**Key Files Changed**:
- `src/locale_parser.rs` (new) - Parses CSL locale XML
- `src/locale.rs` - Now uses rust_embed for real locale loading
- `Cargo.toml` - Added rust-embed dependency

### 2025-11-27 Session 1 - Test Infrastructure Setup

**Completed**:
- Created `build.rs` to generate test functions from 858 CSL test files
- Created `tests/csl_conformance.rs` with test parser and runner
- Fixed section marker parsing (handles both 4 and 5 equals signs)
- Fixed date-parts deserialization to accept strings or integers
- Created `enabled_tests.txt` manifest with 71 passing tests
- All 89 tests passing (17 unit + 72 conformance)

**Key Findings**:
- Many tests fail on name order (default should be "Given Family")
- Date tests need locale month names
- Many tests blocked on sorting/disambiguation features

---

## References

- Design document: `claude-notes/plans/2025-11-26-citeproc-rust-port-design.md`
- Pandoc citeproc source: `external-sources/citeproc/src/Citeproc/`
- CSL spec analysis: `claude-notes/research/2025-11-26-citeproc-output-architecture.md`
- Issue tracker: `br show k-422`
