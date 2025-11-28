# Names Element

Reference: `external-sources/csl-spec/specification.rst` lines 972-1386, 1912-2033

The `cs:names` element is the most complex rendering element. It handles
name lists (authors, editors, etc.) with many formatting options.

## Basic Structure (lines 972-997)

```xml
<names variable="author editor" delimiter="; ">
  <name/>                    <!-- name formatting -->
  <et-al/>                   <!-- "et al." formatting -->
  <label prefix=" (" suffix=")"/>  <!-- role label -->
  <substitute>               <!-- fallback if empty -->
    <names variable="editor"/>
    <text macro="title"/>
  </substitute>
</names>
```

**`variable` attribute**: Space-separated list of name variables.
Each is rendered independently in order, except for special editor-translator
handling.

**Editor-translator collapsing** (lines 980-987): When `variable="editor translator"`
and both contain identical names, only one is rendered. If `cs:label` present,
uses "editortranslator" term instead of separate "editor"/"translator" terms.

## Name Parts (lines 1104-1118)

Personal names have up to five parts:

| Part | Description | Example |
|------|-------------|---------|
| `family` | Surname | "Gogh" |
| `given` | Given names | "Vincent" or "V." |
| `suffix` | Name suffix | "Jr.", "III" |
| `non-dropping-particle` | Kept with surname alone | "van" (Dutch) |
| `dropping-particle` | Dropped with surname alone | "von" (German) |

**Non-dropping vs dropping**:
- "Vincent van Gogh" → surname only: "van Gogh" (non-dropping kept)
- "Alexander von Humboldt" → surname only: "Humboldt" (dropping dropped)

## cs:name Attributes (lines 1004-1166)

### Name List Delimiters

| Attribute | Description | Default |
|-----------|-------------|---------|
| `delimiter` | Between names | ", " |
| `and` | Before last name | none |

`and` values: `"text"` (uses "and" term) or `"symbol"` (uses "&")

### Et-al Abbreviation

| Attribute | Description |
|-----------|-------------|
| `et-al-min` | Min names to trigger abbreviation |
| `et-al-use-first` | Names to show before "et al." |
| `et-al-subsequent-min` | `et-al-min` for subsequent cites |
| `et-al-subsequent-use-first` | `et-al-use-first` for subsequent |
| `et-al-use-last` | Show "..., Last" after truncation |

**et-al-use-last** (lines 1090-1102): When true, renders:
"A, B, C, … Z" instead of "A, B, C, et al."

Requires original list to have ≥2 more names than truncated list.

### Delimiter Before Et-al (lines 1016-1042)

`delimiter-precedes-et-al`:
- `"contextual"` (default) - delimiter only if ≥2 names shown
- `"after-inverted-name"` - delimiter only if preceding name inverted
- `"always"` - always use delimiter
- `"never"` - never use delimiter (use space)

### Delimiter Before Last (lines 1044-1071)

`delimiter-precedes-last` (only relevant when `and` is set):
- `"contextual"` (default) - delimiter for ≥3 names ("A, B, and C")
- `"after-inverted-name"` - delimiter if preceding inverted
- `"always"` - always ("A, and B")
- `"never"` - never ("A, B and C")

### Name Form and Order

| Attribute | Description |
|-----------|-------------|
| `form` | "long" (default), "short", "count" |
| `name-as-sort-order` | "first" or "all" - inverted display |
| `sort-separator` | Delimiter for inverted names (default ", ") |

**form="short"**: Only family + non-dropping-particle ("van Gogh")

**form="count"**: Returns number of names (for sorting)

**name-as-sort-order**: Inverts name order
- "first" - only first name inverted
- "all" - all names inverted
- Only affects scripts with given-family order (Latin, Greek, Cyrillic, Arabic)

### Initialization

| Attribute | Description |
|-----------|-------------|
| `initialize-with` | String added after initials (e.g. ".") |
| `initialize` | "true" (default) or "false" |

When `initialize-with` is set, given names become initials.
When `initialize="false"` but `initialize-with` set, existing initials get
the suffix but full names stay full.

Example: With `initialize-with="."` and `initialize="false"`:
"James T Kirk" → "James T. Kirk"

## Name-part Order (lines 1169-1286)

Display order depends on:
- `form` attribute
- `name-as-sort-order` attribute
- `demote-non-dropping-particle` global option
- Script of the name

### Display Order (Latin scripts, form="long")

**Normal order**:
1. given
2. dropping-particle
3. non-dropping-particle
4. family
5. suffix

→ "Vincent van Gogh III"

**Inverted, demote="never" or "sort-only"**:
1. non-dropping-particle
2. family
3. given
4. dropping-particle
5. suffix

→ "van Gogh, Vincent, III"

**Inverted, demote="display-and-sort"**:
1. family
2. given
3. dropping-particle
4. non-dropping-particle
5. suffix

→ "Gogh, Vincent van, III"

### Sorting Order

**demote="never"**:
1. non-dropping-particle + family
2. dropping-particle
3. given
4. suffix

**demote="sort-only" or "display-and-sort"**:
1. family
2. dropping-particle + non-dropping-particle
3. given
4. suffix

### Asian Scripts (lines 1259-1282)

Names in Chinese, Japanese, Korean scripts:
- Always family-given order
- No particles
- `name-as-sort-order` has no effect

### Non-personal Names (lines 1284-1286)

Institutional names lack name-parts. Sorted as-is, but English articles
("a", "an", "the") at start are stripped for sorting.

## Name-part Formatting (lines 1288-1314)

```xml
<names variable="author">
  <name>
    <name-part name="family" text-case="uppercase"/>
    <name-part name="given" font-style="italic"/>
  </name>
</names>
```

- `name="given"` - affects given + dropping-particle
- `name="family"` - affects family + non-dropping-particle
- `suffix` name-part cannot be formatted

## Et-al Element (lines 1316-1337)

Customize et-al rendering:

```xml
<names variable="author">
  <et-al term="and others" font-style="italic"/>
</names>
```

- `term` - "et-al" (default) or "and others"
- Formatting attributes apply to the term

## Substitute Element (lines 1339-1367)

Fallback when name variables are empty:

```xml
<names variable="author">
  <substitute>
    <names variable="editor"/>
    <names variable="translator"/>
    <text macro="title"/>
  </substitute>
</names>
```

**Rules**:
- Must be last child of `cs:names`
- First non-empty result is used
- Substituted variables are **suppressed** in rest of output (no duplication)
- Shorthand `<names variable="..."/>` in substitute inherits parent's `cs:name`/`cs:et-al`

## Label in Names (lines 1369-1385)

```xml
<names variable="editor">
  <name/>
  <label prefix=" (" suffix=")"/>  <!-- "John Doe (editor)" -->
</names>
```

Position relative to `cs:name` determines label position in output.

Extra `form` values available: "verb", "verb-short"
(e.g. "edited by" for editor term)

## Inheritable Name Options (lines 2013-2033)

These attributes can be set on `cs:style`, `cs:citation`, `cs:bibliography`
and inherited by all `cs:names`/`cs:name` within:

From `cs:name`:
- `and`, `delimiter-precedes-et-al`, `delimiter-precedes-last`
- `et-al-min`, `et-al-use-first`, `et-al-use-last`
- `et-al-subsequent-min`, `et-al-subsequent-use-first`
- `initialize`, `initialize-with`
- `name-as-sort-order`, `sort-separator`
- `name-form` (= `form`), `name-delimiter` (= `delimiter`)

From `cs:names`:
- `names-delimiter` (= `delimiter`)

Lower-level settings override higher-level.

## Name Particles - Global Option (lines 1912-2010)

`demote-non-dropping-particle` on `cs:style`:

| Value | Display (inverted) | Sort |
|-------|-------------------|------|
| `"never"` | "van Gogh, Vincent" | under "V" |
| `"sort-only"` | "van Gogh, Vincent" | under "G" |
| `"display-and-sort"` (default) | "Gogh, Vincent van" | under "G" |

## Summary: Why Names Are Complex

1. **Five name parts** with different handling rules
2. **Two particle types** with language-specific conventions
3. **Script detection** affects ordering (Latin vs Asian)
4. **Multiple display modes** (long/short, inverted/normal)
5. **Et-al abbreviation** with subsequent-cite variants
6. **Disambiguation** may expand names (see 04-disambiguation.md)
7. **Substitution** with inheritance
8. **Inheritance** across style/citation/bibliography levels
