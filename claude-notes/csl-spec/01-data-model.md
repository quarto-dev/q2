# CSL Data Model

Reference: `external-sources/csl-spec/specification.rst` lines 83-182, 2615-3212

## File Types (lines 83-112)

Three CSL file types:
1. **Independent styles** (`.csl`) - contain formatting instructions
2. **Dependent styles** (`.csl`) - alias to an independent style (metadata only)
3. **Locale files** (`locales-xx-XX.xml`) - localization data per language dialect

## Style Structure (lines 124-182)

```xml
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text">
  <info>...</info>           <!-- metadata, required first -->
  <locale xml:lang="en">     <!-- optional locale overrides -->
    ...
  </locale>
  <macro name="author">      <!-- reusable formatting, optional -->
    ...
  </macro>
  <citation>                 <!-- required -->
    <sort>...</sort>         <!-- optional -->
    <layout>...</layout>     <!-- required -->
  </citation>
  <bibliography>             <!-- optional -->
    <sort>...</sort>
    <layout>...</layout>
  </bibliography>
</style>
```

### Root Element Attributes (lines 130-149)

| Attribute | Values | Description |
|-----------|--------|-------------|
| `class` | `"in-text"`, `"note"` | In-text citations vs footnotes |
| `version` | `"1.0"` | CSL version |
| `default-locale` | locale code | Default language (e.g. "en-US") |

## Item Types (Appendix III, lines 2615-2796)

Common types used in bibliographies:

| Type | Description |
|------|-------------|
| `article` | Preprint, working paper (not journal) |
| `article-journal` | Journal article |
| `article-magazine` | Magazine article |
| `article-newspaper` | Newspaper article |
| `book` | Book (authored or edited) |
| `chapter` | Chapter in edited book |
| `paper-conference` | Published conference paper |
| `report` | Technical report, white paper |
| `thesis` | Dissertation, thesis |
| `webpage` | Website/webpage |

Other types: `bill`, `broadcast`, `classic`, `collection`, `dataset`, `document`,
`entry`, `entry-dictionary`, `entry-encyclopedia`, `event`, `figure`, `graphic`,
`hearing`, `interview`, `legal_case`, `legislation`, `manuscript`, `map`,
`motion_picture`, `musical_score`, `pamphlet`, `patent`, `performance`,
`periodical`, `personal_communication`, `post`, `post-weblog`, `regulation`,
`review`, `review-book`, `software`, `song`, `speech`, `standard`, `treaty`

## Variables (Appendix IV, lines 2798-3127)

### Standard Variables (lines 2801-2954)

Text content variables:

| Variable | Description |
|----------|-------------|
| `title` | Primary title |
| `title-short` | Abbreviated title (deprecated, use `form="short"`) |
| `container-title` | Journal/book/album title |
| `collection-title` | Series title |
| `abstract` | Abstract |
| `note` | Descriptive text |
| `annote` | Short annotation |
| `publisher` | Publisher name |
| `publisher-place` | Publisher location |
| `event-title` | Conference/event name |
| `event-place` | Event location |
| `archive` | Archive name |
| `archive-place` | Archive location |
| `archive_location` | Location within archive |
| `genre` | Type/subtype (e.g. "Doctoral dissertation") |
| `medium` | Format (e.g. "CD", "DVD") |
| `source` | Database/catalog source |
| `status` | Publication status ("forthcoming", "in press") |
| `language` | ISO 639-1 code (e.g. "en", "de-DE") |
| `DOI` | Digital Object Identifier |
| `ISBN` | Book ISBN |
| `ISSN` | Serial ISSN |
| `PMID` | PubMed ID |
| `PMCID` | PubMed Central ID |
| `URL` | Web address |
| `citation-key` | Input data identifier (like BibTeX key) |
| `citation-label` | Formatted label for label styles |
| `year-suffix` | Disambiguation suffix (a, b, c...) |

### Number Variables (lines 2956-3019)

Numeric content (may contain ranges, prefixes, suffixes):

| Variable | Description |
|----------|-------------|
| `page` | Page range in container |
| `page-first` | First page only |
| `volume` | Volume number |
| `issue` | Issue number |
| `edition` | Edition number |
| `chapter-number` | Chapter/track number |
| `number` | Generic number (report number, etc.) |
| `number-of-pages` | Total pages |
| `number-of-volumes` | Total volumes |
| `citation-number` | Position in bibliography (generated) |
| `locator` | Cite-specific pinpoint (e.g. "p. 5") |
| `first-reference-note-number` | Note number of first cite |
| `version` | Version number |
| `section` | Section identifier |

**Numeric content rules** (line 1491-1497): Content is "numeric" if it solely
consists of numbers, optionally with prefixes/suffixes ("D2", "2b", "L2d"),
separated by comma, hyphen, or ampersand ("2, 3", "2-4", "2 & 4").

### Date Variables (lines 3021-3040)

| Variable | Description |
|----------|-------------|
| `issued` | Publication date |
| `accessed` | Access date (for URLs) |
| `event-date` | Event date |
| `original-date` | Original publication date |
| `submitted` | Submission date |
| `available-date` | Online-first date |

Date structure typically includes:
- `year` (required)
- `month` (optional, 1-12 or season 13-16)
- `day` (optional, 1-31)
- `circa` flag for approximate dates

### Name Variables (lines 3042-3127)

| Variable | Description |
|----------|-------------|
| `author` | Author(s) |
| `editor` | Editor(s) |
| `translator` | Translator(s) |
| `container-author` | Author of container (e.g. book author for chapter) |
| `collection-editor` | Series editor |
| `director` | Director (film, etc.) |
| `interviewer` | Interviewer |
| `reviewed-author` | Author of reviewed work |
| `composer` | Composer |
| `illustrator` | Illustrator |
| `original-author` | Original creator |

See [03-names.md](03-names.md) for name structure details.

## Special Variable: `year-suffix`

The `year-suffix` variable (line 2953-2954) is generated by the processor during
disambiguation. It's an alphabetic suffix (a, b, c, ..., z, aa, ab, ...) added
to distinguish works by the same author in the same year.

- Assignment follows bibliography order
- After "z": "aa", "ab", ..., "az", "ba", etc.
- Can be rendered explicitly with `<text variable="year-suffix"/>`
- By default, appended to first rendered date

## Variable Forms (lines 750-754)

Some variables support `form` attribute:
- `"long"` (default) - full form
- `"short"` - abbreviated form

If short form unavailable, falls back to long form.

Applies to: `title`, `container-title`, name variables with `cs:name form="short"`
