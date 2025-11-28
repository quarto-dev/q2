# Disambiguation

Reference: `external-sources/csl-spec/specification.rst` lines 1584-1677

Disambiguation is the process of making cites uniquely identify their
bibliographic entries. This is one of the most complex parts of CSL processing.

## When Is Disambiguation Needed? (line 1587)

A cite is **ambiguous** when it matches multiple bibliographic entries.

Example: Two works by "Doe" in 2007 both cited as "(Doe 2007)" - ambiguous!

**Note** (lines 1675-1676): Uncited entries in bibliography can make cites
ambiguous. Processor should include invisible cites for uncited entries
during disambiguation.

## The Four Methods (lines 1588-1594)

Disambiguation methods are tried **in this exact order**:

1. **Expand names** - add initials or full given names
2. **Show more names** - reduce et-al abbreviation
3. **Disambiguate condition** - render with `disambiguate="true"`
4. **Add year-suffix** - append a, b, c, etc.

Each method is only used if enabled by style attributes.

## Method 1: Expand Names (lines 1599-1649)

Enabled by: `disambiguate-add-givenname="true"`

Configured by: `givenname-disambiguation-rule`

### Name Expansion Steps (lines 1617-1626)

When `initialize-with` is set and `initialize="true"` (default):

1. **Step (a)**: Show initials
   - Form "short" → "long" (e.g. "Doe" → "J. Doe")

2. **Step (b)**: Show full given names
   - Set `initialize="false"` (e.g. "J. Doe" → "John Doe")

When `initialize-with` is NOT set:
- Show full given names directly (e.g. "Doe" → "John Doe")

### Disambiguation Rules (lines 1629-1649)

| Rule | Scope | Target Names | Expansion |
|------|-------|--------------|-----------|
| `"by-cite"` (default) | Disambiguate cites | Ambiguous names in ambiguous cites | Stop after first success |
| `"all-names"` | Disambiguate cites AND names | All ambiguous names, all cites | Full expansion |
| `"all-names-with-initials"` | Disambiguate cites AND names | All ambiguous names, all cites | Initials only |
| `"primary-name"` | Disambiguate cites AND names | First name only | Full expansion |
| `"primary-name-with-initials"` | Disambiguate cites AND names | First name only | Initials only |

**Key distinction**:
- `"by-cite"` only expands names in ambiguous cites
- Other rules also expand names in unambiguous cites (global name disambiguation)

Example of global name disambiguation:
- "(Doe 1950; Doe 2000)" - different years, unambiguous cites
- With `"all-names"`: "(Jane Doe 1950; John Doe 2000)" - names still expanded

## Method 2: Show More Names (lines 1651-1661)

Enabled by: `disambiguate-add-names="true"`

Names hidden by et-al abbreviation are added one by one until disambiguation
succeeds or no more names can help.

### Interaction with Method 1 (lines 1654-1657)

When both methods enabled:
1. First try expanding rendered names (Method 1)
2. If still ambiguous, add hidden names one by one
3. Added names are also expanded if it helps

## Method 3: Disambiguate Condition (lines 1664-1665)

Uses `cs:choose` with `disambiguate="true"` condition.

```xml
<choose>
  <if disambiguate="true">
    <text variable="title"/>  <!-- Only rendered if needed -->
  </if>
</choose>
```

This is tried after Methods 1 and 2 have been exhausted.

## Method 4: Year-Suffix (lines 1667-1673)

Enabled by: `disambiguate-add-year-suffix="true"`

- Adds alphabetic suffix: a, b, c, ..., z, aa, ab, ...
- Assignment follows bibliography order
- **Always succeeds** (final fallback)

### Year-Suffix Placement

By default, appended to first rendered date via `cs:date`.

Can be explicitly placed with `<text variable="year-suffix"/>`.

**Scope rule** (line 1672-1673): If `year-suffix` is explicitly rendered in
`cs:citation` scope, it's suppressed in `cs:bibliography` unless also
explicitly rendered there (and vice versa).

## Bibliography Entry Disambiguation (lines 1659-1662)

Important subtlety: disambiguation must also ensure bibliography entries
can be uniquely cited.

If Methods 1 and 2 are used for cite disambiguation, they also act on
bibliography entries to ensure the detail distinguishing cites is visible
in the bibliography.

Example: If "(J. Doe 2007)" and "(B. Doe 2007)" are disambiguated cites,
the bibliography entries should show "J. Doe" and "B. Doe" too.

## Cite Grouping (lines 1678-1697)

After disambiguation, cites with identical rendered names are grouped together.

Example: "(Doe 1999; Smith 2002; Doe 2006)" → "(Doe 1999, 2006; Smith 2002)"

**Enabling**: Set `cite-group-delimiter` or `collapse` on `cs:citation`.

**Timing**: Grouping happens after sorting and disambiguation.

**Comparison**: Based on output of first `cs:names` element (including substitutions).

### cite-group-delimiter (lines 1692-1697)

Specifies delimiter between cites in a group.

```xml
<citation collapse="year" cite-group-delimiter=",">
```

Result: "(Doe 1999,2001; Jones 2000)"

## Cite Collapsing (lines 1699-1743)

Collapsing compresses cite groups or numeric ranges.

### Collapse Values (lines 1707-1729)

| Value | Effect | Example |
|-------|--------|---------|
| `"citation-number"` | Collapse numeric ranges | "[1, 2, 3, 5]" → "[1–3, 5]" |
| `"year"` | Suppress repeated names | "(Doe 2000, Doe 2001)" → "(Doe 2000, 2001)" |
| `"year-suffix"` | Also suppress repeated years | "(Doe 2000a, 2000b)" → "(Doe 2000a, b)" |
| `"year-suffix-ranged"` | Also collapse suffix ranges | "(Doe 2000a, b, c)" → "(Doe 2000a–c)" |

### Collapse Delimiters (lines 1731-1743)

| Attribute | Description |
|-----------|-------------|
| `year-suffix-delimiter` | Delimiter between year-suffixes (default: layout delimiter) |
| `after-collapse-delimiter` | Delimiter after collapsed group (default: layout delimiter) |

## Implementation Order

Based on the spec, the processing order should be:

1. **Render cites** with current settings
2. **Identify ambiguous cites** (cites matching multiple entries)
3. **Apply Method 1** (name expansion) if enabled
4. **Apply Method 2** (add names) if enabled
5. **Apply Method 3** (disambiguate condition) if enabled
6. **Apply Method 4** (year-suffix) if enabled
7. **Update bibliography entries** to reflect disambiguation
8. **Sort cites** (per cs:sort in cs:citation)
9. **Group cites** by rendered names
10. **Collapse** cite groups/ranges

## Key Points for Implementation

1. **Order matters**: Methods must be tried 1→2→3→4

2. **Incremental**: Each method tries minimal changes first

3. **Sets of ambiguous cites**: Work on all cites sharing the same
   rendered form simultaneously

4. **Bibliography sync**: Disambiguation changes must propagate to
   bibliography entries

5. **Year-suffix assignment**: Based on bibliography order, not cite order

6. **Uncited entries**: Include in disambiguation even if not cited

7. **Multi-pass potential**: May need multiple passes as expanding one
   name could create new ambiguities
