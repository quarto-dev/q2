# Sorting

Reference: `external-sources/csl-spec/specification.rst` lines 2055-2151

Sorting controls the order of cites within citations and entries in bibliographies.

## Basic Structure (lines 2058-2072)

```xml
<citation>
  <sort>
    <key variable="author"/>
    <key variable="issued" sort="descending"/>
  </sort>
  <layout>...</layout>
</citation>

<bibliography>
  <sort>
    <key macro="author" names-min="3" names-use-first="3"/>
    <key variable="issued"/>
  </sort>
  <layout>...</layout>
</bibliography>
```

**Default order** (lines 2061-2062): Without `cs:sort`, items appear in
citation order (order they were cited in the document).

## Sort Keys (lines 2064-2080)

Each `cs:key` specifies a sort criterion:

| Attribute | Description |
|-----------|-------------|
| `variable` | Sort by variable value |
| `macro` | Sort by macro output |
| `sort` | "ascending" (default) or "descending" |

### Key Priority (lines 2074-2080)

Keys are evaluated in sequence:
1. Primary sort on first key
2. Secondary sort (for ties) on second key
3. Tertiary sort on third key
4. etc.

Sorting stops when order is determined or keys exhausted.

### Empty Values (line 2079-2080)

Items with empty sort key values are placed **at the end** for both
ascending and descending sorts.

## Sorting by Variable (lines 2101-2126)

When using `<key variable="..."/>`:

### Text Variables

Returns string value without rich text markup.

### Name Variables (lines 2108-2110)

Returned as name list string with:
- `form="long"`
- `name-as-sort-order="all"`

### Date Variables (lines 2112-2122)

Returned in YYYYMMDD format:
- Missing parts replaced with zeros (e.g. "20001200" for "December 2000")
- Less specific dates sort before more specific in ascending order
- Negative years sorted inversely ("100BC" before "50BC" before "50AD")
- Seasons ignored for sorting
- Date ranges: start date primary, end date secondary

Examples:
- "2000, May 2000, May 1st 2000" (ascending, less → more specific)
- "2000, 2000–2002" (single date before range with same start)

### Number Variables (lines 2124-2126)

Returned as integers (numeric form).
Non-numeric text values returned as strings.

## Sorting by Macro (lines 2128-2151)

When using `<key macro="..."/>`:

Returns string output the macro would generate, without rich text markup.

### Name Handling in Macros (lines 2135-2137)

- `cs:label` elements excluded from sort key
- `name-as-sort-order="all"` applied
- "et-al" and "and others" terms excluded

### Advantages of Macro Sorting (lines 2139-2143)

1. **Substitution**: Empty author → editor → title
2. **Et-al abbreviation**: Can use et-al settings or override
3. **Short form sorting**: `form="short"` on `cs:name`
4. **Count sorting**: `form="count"` returns number of names

### Date/Number in Macros (lines 2145-2151)

Number variables via `cs:number` and date variables: same as direct variable.

**Exception for dates**: Macros only return date-parts that would be rendered
(respecting `date-parts` attribute or listed `cs:date-part` elements).

## Et-al Override Attributes (lines 2068-2072)

On `cs:key`, can override et-al settings for macros:

| Attribute | Overrides |
|-----------|-----------|
| `names-min` | `et-al-min` / `et-al-subsequent-min` |
| `names-use-first` | `et-al-use-first` / `et-al-subsequent-use-first` |
| `names-use-last` | `et-al-use-last` |

## Practical Examples

### Sort bibliography by author, then year

```xml
<bibliography>
  <sort>
    <key macro="author"/>
    <key variable="issued"/>
  </sort>
  ...
</bibliography>
```

### Sort cites by citation number (for numeric styles)

```xml
<citation>
  <sort>
    <key variable="citation-number"/>
  </sort>
  ...
</citation>
```

### Sort by year descending, then author

```xml
<bibliography>
  <sort>
    <key variable="issued" sort="descending"/>
    <key macro="author"/>
  </sort>
  ...
</bibliography>
```

### Sort by number of authors

```xml
<macro name="author-count">
  <names variable="author">
    <name form="count"/>
  </names>
</macro>

<bibliography>
  <sort>
    <key macro="author-count"/>
    <key macro="author"/>
  </sort>
  ...
</bibliography>
```

## Implementation Notes

1. **Case-insensitive**: Sorting is case-insensitive (line 2068)

2. **Strip markup**: Sort keys are plain text without formatting

3. **Two contexts**: Sorting happens in both `cs:citation` and `cs:bibliography`

4. **Timing**: Citation sorting happens after disambiguation but before
   cite grouping/collapsing

5. **Name sorting order**: Use the name sorting order (see 03-names.md),
   not display order

6. **Stable sort**: Items with equal keys should maintain relative order
