# Localization

Reference: `external-sources/csl-spec/specification.rst` lines 400-720, 2035-2054, 2455-2613

Localization provides language-specific terms, date formats, and options.

## Locale Sources (lines 400-413)

Two sources of localization data:

1. **Locale files** (`locales-xx-XX.xml`) - default data per language
2. **In-style `cs:locale`** - style-specific overrides

In-style locales placed after `cs:info`, before `cs:citation`.

```xml
<style>
  <info>...</info>
  <locale xml:lang="en">
    <terms>
      <term name="editortranslator">ed. &amp; trans.</term>
    </terms>
  </locale>
  <citation>...</citation>
</style>
```

## Locale Fallback (lines 430-483)

When looking up a localizable unit (term, date format, option), sources
are checked in priority order:

### For dialect "de-AT" (Austrian German):

**A. In-style `cs:locale` elements:**
1. `xml:lang="de-AT"` (exact dialect match)
2. `xml:lang="de"` (language match)
3. No `xml:lang` (universal override)

**B. Locale files:**
4. `locales-de-AT.xml` (exact dialect)
5. `locales-de-DE.xml` (primary dialect fallback)
6. `locales-en-US.xml` (ultimate fallback)

Fallback stops when unit is found (even if empty string).

### Primary vs Secondary Dialects (lines 440-458)

| Primary | Secondary(s) |
|---------|--------------|
| de-DE | de-AT, de-CH |
| en-US | en-GB |
| es-ES | es-CL, es-MX |
| fr-FR | fr-CA |
| pt-PT | pt-BR |
| zh-CN | zh-TW |

## Terms (lines 559-592)

Terms are localized strings.

```xml
<terms>
  <term name="page">
    <single>page</single>
    <multiple>pages</multiple>
  </term>
  <term name="page" form="short">
    <single>p.</single>
    <multiple>pp.</multiple>
  </term>
</terms>
```

### Term Forms (lines 573-579)

| Form | Example for "editor" |
|------|---------------------|
| `"long"` (default) | "editor", "editors" |
| `"short"` | "ed.", "eds." |
| `"verb"` | "edited by" |
| `"verb-short"` | "ed." |
| `"symbol"` | "§", "§§" (for section) |

### Form Fallback (lines 581-583)

If requested form undefined:
- `"verb-short"` → `"verb"` → `"long"`
- `"symbol"` → `"short"` → `"long"`
- `"short"` → `"long"`

If no form available after fallback → empty string.

## Ordinal Terms (lines 594-642)

For rendering numbers as ordinals ("1st", "2nd", etc.).

### Basic Ordinals (lines 601-620)

| Term | Matches | Default match |
|------|---------|---------------|
| `ordinal` | Default suffix | all |
| `ordinal-00` to `ordinal-09` | Last digit | last-digit |
| `ordinal-10` to `ordinal-99` | Last two digits | last-two-digits |

**Match attribute**:
- `"last-digit"` (default for 00-09)
- `"last-two-digits"` (default for 10-99)
- `"whole-number"` (exact match only)

**Priority**: `ordinal-10` to `ordinal-99` take precedence over `ordinal-00` to `ordinal-09`.

### Long Ordinals (lines 635-642)

Terms `long-ordinal-01` to `long-ordinal-10` for "first" through "tenth".

Numbers >10 fall back to regular ordinals.

### Ordinal Replacement Rule (lines 630-633)

**Important**: Redefining any ordinal term in `cs:locale` replaces ALL
ordinal terms. They're treated as a set.

## Gender-specific Ordinals (lines 644-681)

Some languages use gendered ordinals (French: "1er" vs "1re").

### Specifying Gender

On target terms (nouns):
```xml
<term name="edition" gender="feminine">édition</term>
<term name="month-01" gender="masculine">janvier</term>
```

On ordinal terms:
```xml
<term name="ordinal-01" gender-form="feminine">re</term>
<term name="ordinal-01" gender-form="masculine">er</term>
```

### Gender Matching

- Number variable ordinals: match gender of variable's term
- Day ordinals: match gender of month term

Fallback to neuter (no `gender-form`) if gendered variant undefined.

## Localized Date Formats (lines 683-709)

Two formats defined per locale:

```xml
<date form="numeric">  <!-- e.g. "12-15-2005" -->
  <date-part name="month" form="numeric" prefix="-"/>
  <date-part name="day" prefix="-"/>
  <date-part name="year"/>
</date>

<date form="text">  <!-- e.g. "December 15, 2005" -->
  <date-part name="month" suffix=" "/>
  <date-part name="day" suffix=", "/>
  <date-part name="year"/>
</date>
```

**Restriction** (lines 699-700): Affixes not allowed on `cs:date` when
defining localized formats (only on `cs:date-part`).

## Locale Options (lines 711-717, 2035-2053)

Two locale-specific options, set on `cs:style-options`:

### limit-day-ordinals-to-day-1 (lines 2038-2046)

When `true`: Only day 1 uses ordinal form ("1er janvier", "2 janvier").
Default: `false` (all days ordinal if requested).

### punctuation-in-quote (lines 2048-2053)

Controls placement of comma/period relative to closing quote:
- `false` (default): punctuation outside ("word",)
- `true`: punctuation inside ("word,")

## Standard Terms (Appendix II, lines 2455-2613)

### Category Terms

For each item type in Appendix III, there's a corresponding term.
For each name/number variable, there's a corresponding term.

### Locators (lines 2473-2504)

`act`, `appendix`, `article-locator`, `book`, `canon`, `chapter`,
`column`, `elocation`, `equation`, `figure`, `folio`, `issue`, `line`,
`note`, `opus`, `page`, `paragraph`, `part`, `rule`, `scene`, `section`,
`sub-verbo`, `supplement`, `table`, `timestamp`, `title-locator`,
`verse`, `version`, `volume`

### Months (lines 2506-2520)

`month-01` through `month-12`

### Seasons (lines 2550-2556)

`season-01` (Spring), `season-02` (Summer), `season-03` (Autumn), `season-04` (Winter)

### Punctuation (lines 2538-2548)

`open-quote`, `close-quote`, `open-inner-quote`, `close-inner-quote`,
`page-range-delimiter`, `colon`, `comma`, `semicolon`

### Miscellaneous (lines 2558-2613)

Common terms: `accessed`, `and`, `and others`, `anonymous`, `at`,
`available at`, `by`, `circa`, `cited`, `et-al`, `forthcoming`, `from`,
`henceforth`, `ibid`, `in`, `in press`, `internet`, `letter`, `loc-cit`,
`no date`, `no-place`, `no-publisher`, `on`, `online`, `op-cit`,
`original-work-published`, `personal-communication`, `retrieved`,
`review-of`, `scale`, `special-issue`, `special-section`, `ad`, `bc`,
`bce`, `ce`, etc.

## Implementation Notes

1. **Fallback chain**: Must implement full fallback (in-style → locale files → en-US)

2. **Empty terms valid**: Empty string is a valid term definition that stops fallback

3. **Ordinal atomicity**: Ordinal terms replaced as a group

4. **Gender propagation**: Gender from noun terms must reach ordinal selection

5. **Default locale**: `default-locale` on `cs:style` sets output language

6. **Language detection**: Item's `language` field affects text-case behavior
