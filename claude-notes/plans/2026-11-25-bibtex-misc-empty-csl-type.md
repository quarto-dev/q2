---
title: "citeproc: BibTeX misc entries get an empty CSL type"
date: 2026-11-25
description: "Map BibTeX misc, set, xdata and unknown entry types to an empty CSL type, matching Pandoc, so they no longer render as preprints."
---

Strand: `bd-dgvmu72x`. Issue: <https://github.com/quarto-dev/q2/issues/1311>

## Overview

`biblatex_entry_to_reference` (`crates/pampa/src/citeproc_filter.rs`) mapped
`EntryType::Misc | Set | XData | Unknown(_)` to the CSL type `article`. CSL
styles render `article` as a preprint, so a BibTeX `@misc` entry came out as
`Doe, Jane. 2023. “A Web Page”. Preprint. https://example.org.`

Pandoc gives these entries no CSL type (`pandoc -f bibtex -t csljson` writes
`"type": ""`), so they render as generic documents:
`Doe, Jane. 2023. A Web Page. https://example.org.` The fix maps them to `""`.

## Work items

- [x] Map the four entry types to `""` in `biblatex_entry_to_reference`.
- [x] Update the `misc2025` and `unknown2026` assertions in
      `test_load_bibtex_macros_crossrefs_and_types`.
- [x] Add the render-level test `bibtex_misc_renders_as_generic_document`
      (fails before the fix with the issue's exact output).
