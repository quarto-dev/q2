---
title: "Citeproc: resolve citations inside footnote definitions"
date: 2026-11-16
description: "GH #1225: a citation in a [^1] footnote stayed unresolved in a book project."
---

Strand: bd-xrbkjix5 · Issue: <https://github.com/quarto-dev/q2/issues/1225>

## Overview

**Current behaviour.** A `[^1]: ...` footnote is parsed as a
`Block::NoteDefinitionPara` (or `NoteDefinitionFencedBlock` for the fenced
form), and stays that way until the footnotes transform turns it into an
`Inline::Note`. Citeproc's block walkers in `crates/pampa/src/citeproc_filter.rs`
(`collect_citations_from_block`, `transform_block`) descend into `Inline::Note`
but fall through `_ => {}` on the two note-definition blocks. When citeproc
runs before the footnotes transform — `filters: [citeproc]` puts it in the
`.pre` bucket, which the issue's book project needs because of #1157 — the
footnote's `Cite` is never collected or replaced, and renders as `@xie2020`.

**New behaviour.** Both walkers descend into `NoteDefinitionPara` (inlines)
and `NoteDefinitionFencedBlock` (blocks), so a footnote citation resolves
the same way as a body-text one, whatever the filter order.

## Work items

- [x] Unit tests: collect and transform citations in both note-definition
      blocks (fail before the fix).
- [x] Add the two arms to `collect_citations_from_block` and
      `transform_block`.
- [x] Reproduce the issue's book project and confirm the footnote resolves.
- [x] Checks: pampa nextest, clippy (host + Windows cross), xtask lint,
      note render.
