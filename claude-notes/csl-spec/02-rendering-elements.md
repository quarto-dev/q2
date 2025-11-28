# Rendering Elements

Reference: `external-sources/csl-spec/specification.rst` lines 719-1565

Rendering elements specify what bibliographic data to include and how to format it.

## Layout (lines 726-742)

`cs:layout` is the required child of `cs:citation` and `cs:bibliography`.

```xml
<citation>
  <layout prefix="(" suffix=")" delimiter=", ">
    <text variable="citation-number"/>
  </layout>
</citation>
```

**Attributes**:
- `prefix`, `suffix` - affixes around entire output
- `delimiter` - separator between cites (in citation) or elements (in bibliography)
- Formatting attributes apply to all output

## Text (lines 744-771)

`cs:text` outputs text content. Must have exactly one of these attributes:

| Attribute | Description |
|-----------|-------------|
| `variable` | Render a variable (e.g. `variable="title"`) |
| `macro` | Call a macro (e.g. `macro="author"`) |
| `term` | Render a term (e.g. `term="and"`) |
| `value` | Render literal text (e.g. `value="In: "`) |

**Additional attributes for `variable`**:
- `form="short"` - use short form if available

**Additional attributes for `term`**:
- `plural="true"` - use plural form
- `form` - "long" (default), "short", "verb", "verb-short", "symbol"

**Allowed attributes**: affixes, display, formatting, quotes, strip-periods, text-case

## Date (lines 773-934)

`cs:date` outputs dates. Requires `variable` attribute.

### Localized Date Format (lines 780-797)

```xml
<date variable="issued" form="numeric"/>  <!-- e.g. "12-15-2005" -->
<date variable="issued" form="text"/>     <!-- e.g. "December 15, 2005" -->
```

**Attributes**:
- `form` - "numeric" or "text"
- `date-parts` - "year-month-day" (default), "year-month", "year"

Can override localized format's date-part attributes with child `cs:date-part`
elements (but not change order or which parts are shown).

### Non-localized Date Format (lines 799-808)

Without `form`, construct date manually:

```xml
<date variable="issued" delimiter=" ">
  <date-part name="month" suffix=" "/>
  <date-part name="day" suffix=", "/>
  <date-part name="year"/>
</date>
```

Order of `cs:date-part` elements determines display order.

### Date-part (lines 813-853)

| name | form values |
|------|-------------|
| `day` | "numeric" (default), "numeric-leading-zeros", "ordinal" |
| `month` | "long" (default), "short", "numeric", "numeric-leading-zeros" |
| `year` | "long" (default), "short" (2-digit) |

### Date Ranges (lines 855-879)

Default range delimiter is en-dash. Custom delimiter set via `range-delimiter`
on `cs:date-part`. The delimiter comes from the largest differing date part.

```xml
<!-- "1-4 May 2008", "May–July 2008", "May 2008/June 2009" -->
<date-part name="day" range-delimiter="-"/>
<date-part name="month"/>
<date-part name="year" range-delimiter="/"/>
```

### AD/BC (lines 881-886)

- Positive years < 4 digits: "AD" appended (e.g. "79" → "79AD")
- Negative years: "BC" appended (e.g. "-2500" → "2500BC")

### Seasons (lines 888-908)

Season codes 13-16 (or 1-4 depending on data format) map to terms:
- `season-01` = Spring
- `season-02` = Summer
- `season-03` = Autumn
- `season-04` = Winter

### Approximate Dates (lines 910-933)

Approximate dates test true for `is-uncertain-date` condition in `cs:choose`.

## Number (lines 935-970)

`cs:number` outputs number variables. Requires `variable` attribute.

```xml
<number variable="edition" form="ordinal"/>
```

**Extraction rules** (lines 942-951):
- Numbers separated by hyphen: spaces stripped ("2 - 4" → "2-4")
- Comma-separated: one space after comma ("2,3" → "2, 3")
- Ampersand-separated: spaces around ("2&3" → "2 & 3")

**Form attribute** (lines 953-963):
- `"numeric"` (default) - "1", "2", "3"
- `"ordinal"` - "1st", "2nd", "3rd" (uses ordinal terms)
- `"long-ordinal"` - "first", "second", "third" (terms, 1-10 only)
- `"roman"` - "i", "ii", "iii"

**Important** (lines 965-967): Numbers with prefixes/suffixes are never
ordinalized or romanized. "2E" stays "2E".

## Label (lines 1387-1424)

`cs:label` outputs the term matching a variable. The term is only rendered if
the variable is non-empty.

```xml
<group delimiter=" ">
  <label variable="page"/>      <!-- "page" or "pages" -->
  <text variable="page"/>       <!-- "5-7" -->
</group>
<!-- Result: "pages 5-7" -->
```

**Attributes**:
- `variable` - must be "locator", "page", or a number variable
- `form` - "long" (default), "short", "symbol"
- `plural` - "contextual" (default), "always", "never"

**Plural detection** (lines 1415-1419): Content is plural if it contains multiple
numbers, or for `number-of-pages`/`number-of-volumes` if > 1.

## Group (lines 1426-1457)

`cs:group` groups rendering elements with implicit conditional behavior.

```xml
<group delimiter=" " prefix="(" suffix=")">
  <text variable="edition"/>
  <label variable="edition"/>
</group>
<!-- Renders "(3rd ed.)" if edition exists, nothing if empty -->
```

**Suppression rule** (lines 1433-1436): Group is suppressed if:
1. At least one element calls a variable (directly or via macro), AND
2. ALL called variables are empty

This allows descriptive terms to appear only when data exists.

**Nesting** (lines 1451-1453): Inner groups evaluated first. Non-empty nested
group counts as non-empty variable for outer group suppression.

**Delimiter behavior** (lines 2228-2230): Delimiters from ancestor elements
are NOT applied within a delimiting element's output. Each delimiting element
(`cs:date`, `cs:names`, `cs:name`, `cs:group`, `cs:layout`) manages its own
delimiter scope.

## Choose (lines 1459-1565)

`cs:choose` provides if/else-if/else conditionals.

```xml
<choose>
  <if type="book">
    <text variable="title" font-style="italic"/>
  </if>
  <else-if type="article-journal">
    <text variable="title" quotes="true"/>
  </else-if>
  <else>
    <text variable="title"/>
  </else>
</choose>
```

### Conditions (lines 1485-1551)

| Attribute | Tests |
|-----------|-------|
| `disambiguate` | "true" - only if needed for disambiguation |
| `is-numeric` | Variable contains numeric content |
| `is-uncertain-date` | Date is approximate |
| `locator` | Locator type matches (e.g. "page", "chapter") |
| `position` | Cite position matches (see below) |
| `type` | Item type matches |
| `variable` | Variable is non-empty |

**Position values** (lines 1506-1544):
- `"first"` - first cite to this item
- `"subsequent"` - any cite after first
- `"ibid"` - immediately follows same item cite
- `"ibid-with-locator"` - ibid with different/new locator
- `"near-note"` - within `near-note-distance` notes

Position implications:
- `ibid-with-locator` true → `ibid` true
- `ibid` true → `subsequent` true
- `near-note` true → `subsequent` true

### Match Attribute (lines 1556-1564)

Controls how multiple conditions/values combine:
- `"all"` (default) - all must be true
- `"any"` - at least one must be true
- `"none"` - none can be true

```xml
<if type="book thesis" match="any">  <!-- book OR thesis -->
<if variable="author editor" match="all">  <!-- author AND editor -->
<if type="book" match="none">  <!-- NOT book -->
```

### Delimiter in Choose (line 1566)

Unlike `cs:group` and `<text macro="...">`, delimiters from ancestor elements
ARE applied within `cs:choose` output.
