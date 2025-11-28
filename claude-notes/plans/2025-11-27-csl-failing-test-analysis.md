# CSL Failing Test Analysis

**Created**: 2025-11-27
**Status**: Analysis complete, ready for implementation

## Overview

Analysis of 582 ignored CSL conformance tests to identify patterns and prioritize fixes.
Current state: 276 enabled tests passing, 582 tests still ignored.

## Issue Categories

### 1. Prefix/Suffix Ordering (HIGH PRIORITY)

**Affected tests**: collapse_*, many formatting tests
**Example**: `collapse_CitationNumberRangesWithAffixes.txt`

**Problem**: CSL spec puts prefix/suffix INSIDE formatting for layout elements.

```
Layout: font-weight="bold" prefix="(" suffix=")"
Expected: <b>([1]–[3])</b>
Actual:   (<b>[1]–[3]</b>)
```

**Root cause**: In `output.rs:to_inlines_inner()`, we apply formatting first, then add prefix/suffix. Should be reversed for layout elements.

**Reference**: Pandoc citeproc uses `formatAffixesInside = True` for Layout elements.
See: `external-sources/citeproc/src/Citeproc/Style.hs:591`

**Fix location**: `crates/quarto-citeproc/src/output.rs` - reorder prefix/suffix application

### 2. HTML in CSL-JSON Fields (HIGH PRIORITY)

**Affected tests**: flipflop_*, textcase_*, many others
**Example**: `flipflop_ItalicsSimple.txt`

**Problem**: CSL-JSON allows HTML markup in text fields. We escape it instead of preserving.

```
Input JSON: "title": "One TwoA <i>Three Four</i> Five!"
Expected:   One TwoA <i>Three Four</i> Five!
Actual:     One TwoA &lt;i&gt;Three Four&lt;/i&gt; Five!
```

**Root cause**: `html_escape()` in CSL HTML writer escapes all `<` and `>` characters.

**Fix needed**: 
- Parse HTML from CSL-JSON text fields during reference loading
- Store as structured data (or pass through without escaping)
- Handle in `render_inlines_to_csl_html()`

### 3. Flip-Flop Formatting (MEDIUM PRIORITY)

**Affected tests**: 19 flipflop_* tests
**Example**: `flipflop_ItalicsSimple.txt`

**Problem**: When italic is applied to content already containing `<i>`, nested italics should flip to normal.

```
Input:    <i>One TwoE <i>Three</i> Four Five!</i>
Expected: <i>One TwoE <span style="font-style:normal;">Three</span> Four Five!</i>
Actual:   <i>One TwoE <i>Three</i> Four Five!</i>
```

**Fix needed**: Track formatting state during rendering, flip nested identical styles.

### 4. Text Case Edge Cases (MEDIUM PRIORITY)

**Affected tests**: textcase_* tests
**Example**: `textcase_InQuotes.txt`

**Problems**:
1. Content inside quotes being lowercased in title case
2. Quote characters being HTML-escaped (`&quot;` instead of `"`)
3. Stop words not handled correctly

```
Expected: From "Distance" to "Friction": Substituting...
Actual:   From &quot;distance&quot; To &quot;friction&quot;: Substituting...
```

**Fix needed**: 
- Don't transform text case inside quotes
- Don't escape quote characters in output
- Implement proper stop word list for title case

### 5. Date Formatting - Negative Years/Eras (LOW PRIORITY)

**Affected tests**: date_NegativeDate*.txt
**Example**: `date_NegativeDateSort.txt`

**Problem**: Negative years should display with era suffix.

```
Expected: BookX (100BC-7-14)
Actual:   BookX (-100714)
```

**Fix needed**: Detect negative years and add BC suffix, positive ancient years add AD.

### 6. Bibliography Layout Structure (LOW PRIORITY)

**Affected tests**: variables_ContainerTitleShort*.txt

**Problem**: Complex bibliography layouts expect specific div structure.

```
Expected: <div class="csl-left-margin">1. </div><div class="csl-right-inline">...</div>
Actual:   1. ...
```

**Fix needed**: Implement `display` attribute support for bibliography second-field-align.

### 7. Superscript Character Handling (LOW PRIORITY)

**Affected tests**: bugreports_NumberAffixEscape.txt

**Problem**: Ordinal indicators rendered as Unicode characters instead of superscript HTML.

```
Expected: (<sup>a</sup>2)
Actual:   (ª2)
```

**Fix needed**: Use `<sup>` tags for ordinal superscripts instead of Unicode.

## Recommended Implementation Order

1. **Prefix/suffix ordering** - Architectural fix, unlocks many tests
2. **HTML in CSL-JSON** - Common pattern, blocks many test categories  
3. **Quote escaping fix** - Quick fix for text case tests
4. **Flip-flop formatting** - Enables 19 tests
5. **Text case refinements** - Stop words, quote handling
6. **Date era formatting** - Isolated feature
7. **Bibliography layout** - Complex, lower priority

## Test Commands

```bash
# Run all ignored tests
cargo nextest run -p quarto-citeproc -- --ignored

# Run specific category
cargo nextest run -p quarto-citeproc "flipflop" -- --ignored
cargo nextest run -p quarto-citeproc "textcase" -- --ignored
cargo nextest run -p quarto-citeproc "collapse" -- --ignored

# Check a specific test failure
cargo nextest run -p quarto-citeproc csl_collapse_citationnumberrangeswithaffixes -- --ignored
```

## References

- Pandoc citeproc: `external-sources/citeproc/src/Citeproc/`
  - `Types.hs` - `addFormatting` function shows affix ordering
  - `Style.hs:591` - `formatAffixesInside = True` for Layout
- CSL conformance roadmap: `claude-notes/plans/2025-11-27-csl-conformance-roadmap.md`
- Test files: `crates/quarto-citeproc/test-data/csl-suite/`
