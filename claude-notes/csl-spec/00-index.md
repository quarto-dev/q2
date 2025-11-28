# CSL 1.0.2 Specification Notes - Index

This directory contains notes summarizing the CSL 1.0.2 specification for reference
when working on quarto-citeproc. The goal is to understand **why** citeproc does
what it does, not to use the spec as a requirements document.

**Source**: `external-sources/csl-spec/specification.rst`

## Note Files

| File | Topic | Spec Lines |
|------|-------|------------|
| [01-data-model.md](01-data-model.md) | Variables, types, file structure | 83-182, 2615-3212 |
| [02-rendering-elements.md](02-rendering-elements.md) | Layout, Text, Date, Number, Label, Group, Choose | 719-1565 |
| [03-names.md](03-names.md) | Names element (complex), name-parts, et-al, substitute | 972-1386 |
| [04-disambiguation.md](04-disambiguation.md) | Disambiguation algorithm (critical!) | 1584-1677 |
| [05-sorting.md](05-sorting.md) | Sort keys, variable/macro sorting | 2055-2165 |
| [06-localization.md](06-localization.md) | Terms, locale fallback, ordinals, dates | 400-720, 2035-2054 |
| [07-formatting.md](07-formatting.md) | Formatting attrs, affixes, delimiter, display, quotes | 2163-2420 |

## Quick Reference: Key Concepts

### Style Classes (line 133-135)
- `"in-text"` - author-date, numeric, label styles
- `"note"` - footnote/endnote styles

### Rendering Element Hierarchy
```
cs:style
  cs:info
  cs:locale* (optional overrides)
  cs:macro* (reusable formatting)
  cs:citation
    cs:sort? (optional)
    cs:layout (required)
  cs:bibliography?
    cs:sort? (optional)
    cs:layout (required)
```

### Rendering Elements
- `cs:text` - output text/variable/term/macro
- `cs:date` - output formatted date
- `cs:number` - output formatted number
- `cs:names` - output name lists (complex!)
- `cs:label` - output term for variable
- `cs:group` - conditional grouping
- `cs:choose` - if/else-if/else conditionals

### Variable Categories (Appendix IV)
- **Standard**: title, container-title, publisher, DOI, URL, etc.
- **Number**: page, volume, issue, edition, citation-number, locator, etc.
- **Date**: issued, accessed, event-date, original-date, etc.
- **Name**: author, editor, translator, etc.

### Name Parts (line 1104-1118)
- `family` - surname
- `given` - given names (full or initialized)
- `suffix` - e.g. "Jr.", "III"
- `non-dropping-particle` - kept when surname only (Dutch "van")
- `dropping-particle` - dropped when surname only (German "von")

### Disambiguation Methods (in order, line 1588-1594)
1. Expand names (add initials or full given names)
2. Show more names (reduce et-al abbreviation)
3. Render with `disambiguate="true"` condition
4. Add year-suffix (a, b, c...)

### Cite Positions (line 1506-1544)
- `first` - first cite to this item
- `subsequent` - any later cite
- `ibid` - immediately follows same item
- `ibid-with-locator` - ibid but with different/new locator
- `near-note` - within `near-note-distance` of previous cite

## Cross-Reference: Spec Section -> Notes

| Spec Section | Lines | Notes File |
|--------------|-------|------------|
| Introduction | 31-53 | (basic context) |
| File Types | 83-123 | 01-data-model |
| Styles - Structure | 124-399 | 01-data-model |
| Locale | 400-484 | 06-localization |
| Locale Files - Structure | 485-720 | 06-localization |
| Rendering Elements | 719-725 | 02-rendering-elements |
| Layout | 726-743 | 02-rendering-elements |
| Text | 744-771 | 02-rendering-elements |
| Date | 773-934 | 02-rendering-elements |
| Number | 935-970 | 02-rendering-elements |
| Names | 972-1386 | 03-names |
| Label | 1387-1424 | 02-rendering-elements |
| Group | 1426-1457 | 02-rendering-elements |
| Choose | 1459-1566 | 02-rendering-elements |
| Citation-specific Options | 1581-1752 | 04-disambiguation |
| Bibliography-specific Options | 1754-1888 | 07-formatting |
| Global Options | 1890-2011 | 03-names (particles), 07-formatting |
| Inheritable Name Options | 2013-2033 | 03-names |
| Locale Options | 2035-2053 | 06-localization |
| Sorting | 2055-2151 | 05-sorting |
| Range Delimiters | 2153-2161 | 07-formatting |
| Formatting | 2163-2420 | 07-formatting |
| Appendix I - Categories | 2425-2453 | (metadata only) |
| Appendix II - Terms | 2455-2613 | 06-localization |
| Appendix III - Types | 2615-2796 | 01-data-model |
| Appendix IV - Variables | 2798-3127 | 01-data-model |
| Appendix V - Page Range Formats | 3128-3197 | 07-formatting |
| Appendix VI - Links | 3198-3212 | (processor behavior) |
