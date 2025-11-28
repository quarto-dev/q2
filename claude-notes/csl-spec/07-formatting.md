# Formatting

Reference: `external-sources/csl-spec/specification.rst` lines 2163-2420, 1754-1888, 3128-3212

This covers formatting attributes, affixes, delimiters, and bibliography options.

## Formatting Attributes (lines 2163-2201)

Apply to: `cs:date`, `cs:date-part`, `cs:et-al`, `cs:group`, `cs:label`,
`cs:layout`, `cs:name`, `cs:name-part`, `cs:names`, `cs:number`, `cs:text`

### font-style

| Value | Effect |
|-------|--------|
| `"normal"` | Default |
| `"italic"` | Italic text |
| `"oblique"` | Slanted text |

### font-variant

| Value | Effect |
|-------|--------|
| `"normal"` | Default |
| `"small-caps"` | Small capitals |

### font-weight

| Value | Effect |
|-------|--------|
| `"normal"` | Default |
| `"bold"` | Bold text |
| `"light"` | Light weight |

### text-decoration

| Value | Effect |
|-------|--------|
| `"none"` | Default |
| `"underline"` | Underlined |

### vertical-align

| Value | Effect |
|-------|--------|
| `"baseline"` | Default |
| `"sup"` | Superscript |
| `"sub"` | Subscript |

## Affixes (lines 2203-2216)

Attributes `prefix` and `suffix` add text before/after element output.

**Apply to**: `cs:date` (not localized), `cs:date-part` (not with localized parent),
`cs:group`, `cs:label`, `cs:layout`, `cs:name`, `cs:name-part`, `cs:names`,
`cs:number`, `cs:text`

**Conditional rendering**: Affixes only appear if element produces output.

**Scope exception** (lines 2213-2216): Affixes are OUTSIDE the scope of formatting,
quotes, strip-periods, and text-case on the same element.

Workaround: Set those attributes on a parent `cs:group`.

## Delimiter (lines 2218-2241)

Separates non-empty child outputs.

**Apply to**: `cs:date`, `cs:names`, `cs:name`, `cs:group`, `cs:layout`

**Delimiter scope** (lines 2228-2230): Ancestor delimiters are NOT applied
within a delimiting element's output.

Example:
```xml
<group delimiter=": ">
  <text term="retrieved"/>
  <group>  <!-- inner group blocks outer delimiter -->
    <text value="&lt;"/>
    <text variable="URL"/>
    <text value="&gt;"/>
  </group>
</group>
<!-- Result: "retrieved: <http://example.com>" not "retrieved: <: http://example.com: >" -->
```

## Display (lines 2243-2340)

Controls block structure in bibliography entries.

| Value | Description |
|-------|-------------|
| `"block"` | Full-width block |
| `"left-margin"` | Left-aligned block (fixed width if followed by right-inline) |
| `"right-inline"` | Continues after left-margin block |
| `"indent"` | Indented block |

Used for complex layouts like:
- Citation number + reference text
- Author block + year + title blocks
- Annotated bibliographies

## Quotes (lines 2342-2349)

`quotes="true"` on `cs:text` wraps output in quotation marks.

Uses locale terms: `open-quote`, `close-quote`, `open-inner-quote`, `close-inner-quote`.

Interaction with `punctuation-in-quote` locale option controls comma/period placement.

## Strip-periods (lines 2351-2356)

`strip-periods="true"` removes periods from rendered text.

**Apply to**: `cs:date-part` (month only), `cs:label`, `cs:text`

Useful for abbreviations: "eds." → "eds" when periods unwanted.

## Text-case (lines 2358-2423)

| Value | Effect |
|-------|--------|
| `"lowercase"` | all lowercase |
| `"uppercase"` | ALL UPPERCASE |
| `"capitalize-first"` | First char of first word capitalized |
| `"capitalize-all"` | First char of each word capitalized |
| `"sentence"` | Sentence case (deprecated) |
| `"title"` | Title case |

**Apply to**: `cs:date`, `cs:date-part`, `cs:label`, `cs:name-part`, `cs:number`, `cs:text`

### Title Case Rules (lines 2392-2406)

For English items:

1. **Uppercase strings**: First char of each word kept upper, rest lowercased
2. **Mixed/lowercase strings**: First char of each lowercase word capitalized

Stop words lowercased unless first/last or after colon.

Stop words defined in: `stop-words.json`

Hyphenated parts treated as separate words.

### Language Detection (lines 2408-2423)

Title case only applies to English items:

- If `default-locale` is "en-*" or unset: items assumed English unless
  `language` field has non-"en" value
- If `default-locale` is non-English: items assumed non-English unless
  `language` field starts with "en"

## Bibliography Options (lines 1754-1888)

### Whitespace (lines 1757-1786)

| Attribute | Description | Default |
|-----------|-------------|---------|
| `hanging-indent` | Hanging indent on entries | false |
| `second-field-align` | Align after first field | none |
| `line-spacing` | Line height multiplier | 1 |
| `entry-spacing` | Extra space between entries | 1 |

`second-field-align` values:
- `"flush"` - first field flush with margin
- `"margin"` - first field in margin

### Reference Grouping (lines 1788-1888)

`subsequent-author-substitute`: Replace repeated author names.

```xml
<bibliography subsequent-author-substitute="---">
```

`subsequent-author-substitute-rule`:

| Value | Description |
|-------|-------------|
| `"complete-all"` | All names match → replace entire list |
| `"complete-each"` | All names match → replace each name |
| `"partial-each"` | Matching names replaced (from start) |
| `"partial-first"` | Only first name replaced if matches |

## Page Range Formats (lines 1901-1910, 3128-3197)

`page-range-format` on `cs:style`:

| Value | Example |
|-------|---------|
| `"expanded"` | 321–328 |
| `"minimal"` | 321–8 |
| `"minimal-two"` | 321–28 |
| `"chicago"` / `"chicago-15"` | Complex rules (see spec) |
| `"chicago-16"` | Updated Chicago rules |

Uses `page-range-delimiter` term (default en-dash).

## Range Delimiters (lines 2153-2161)

- Citation-number ranges: en-dash
- Year-suffix ranges: en-dash
- Locator variable: hyphens → en-dash (always)
- Page variable: hyphens → en-dash (only if `page-range-format` set)

## Hyphenation Option (lines 1893-1899)

`initialize-with-hyphen` on `cs:style`:
- `true` (default): "Jean-Luc" → "J.-L."
- `false`: "Jean-Luc" → "J. L."

## Links (Appendix VI, lines 3198-3212)

Processor should automatically link:
- `url`: as-is
- `doi`: prepend "https://doi.org/"
- `pmid`: prepend "https://www.ncbi.nlm.nih.gov/pubmed/"
- `pmcid`: prepend "https://www.ncbi.nlm.nih.gov/pmc/articles/"

Only the identifier should be in the link anchor, not surrounding text.

## Implementation Notes

1. **Affixes outside formatting**: Remember affixes don't inherit formatting
   from their element

2. **Delimiter scoping**: Each delimiting element creates a new scope

3. **Title case complexity**: Requires stop word list and language detection

4. **Page range algorithms**: Chicago rules are non-trivial to implement

5. **Display blocks**: Need to track and align across bibliography entries

6. **Whitespace attributes**: Apply to bibliography container, not individual entries
