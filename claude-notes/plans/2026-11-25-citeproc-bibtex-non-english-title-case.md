---
title: 'citeproc: keep BibTeX title case for non-English entries'
date: 2026-11-25
description: 'BibTeX titles were converted to sentence case whatever their language, so proper nouns in Dutch titles lost their capitals. As in Pandoc, only English titles are converted now; the language comes from the entry, then the document lang, then English.'
status: in-progress
---

**Braid:** bd-jtnle3px

**Issue:** <https://github.com/quarto-dev/q2/issues/1306>

## Overview

Current behaviour: `format_bibtex_title_case` in
`crates/pampa/src/citeproc_filter.rs` converts every BibTeX title,
container title, and series to sentence case, whatever the language. With
`lang: nl`, "Onderweg in Nederland 2025" renders as "Onderweg in nederland
2025".

New behaviour: as in Pandoc's BibTeX reader, only English titles are
converted. `biblatex_entry_to_reference` takes the language from the entry's
`langid`, `hyphenation`, or `language` field, then the document `lang`
(passed through `load_bibliography`), and then assumes English.
`is_english_language` accepts BCP 47 tags (`en`, `en-US`) and the babel
English names (`english`, `british`, ...). Non-English titles keep their
case, and braced text still becomes a `nocase` span.

## Work items

- [x] Unit tests: `test_load_bibtex_non_english_keeps_title_case`,
      `test_is_english_language`.
- [x] Pass the document `lang` into `load_bibliography` and
      `biblatex_entry_to_reference`; add the `english` flag to
      `format_bibtex_title_case`.
- [x] Run the pampa tests, clippy, `xtask lint`, and the note render.
