---
title: 'HTML: write table alignment as text-align on each cell'
date: 2026-11-24
description: 'The HTML writer put column alignment only on the obsolete col align attribute, which browsers ignore. Emit a text-align style on each th/td instead, as pandoc does.'
---

**Braid:** bd-ykosl1eh · **Issue:** https://github.com/quarto-dev/q2/issues/1327

## Overview

Current behaviour: `write_block`'s `Block::Table` arm in
`crates/pampa/src/writers/html.rs` writes a `<colgroup>` of
`<col align="…">` whenever any column has a non-default alignment, and
`write_table_row` writes `align="…"` on a cell only when the cell itself
has a non-default alignment. Both attributes are obsolete in HTML5, and
browsers ignore `<col align>`, so every column renders left-aligned.

New behaviour (pandoc / Quarto 1): each `<th>`/`<td>` gets
`style="text-align: …;"`, using the cell's own alignment and falling back
to the alignment of the column it starts in. Finding that column tracks
`col_span` within a row and `row_span` across the rows of a section. The
`<colgroup>` is written only when some column has a non-default width
(width output itself is #879 and out of scope), so an alignment-only
table no longer gets a colgroup of bare `<col />` elements.

## Work items

- [x] Unit tests: pipe-table-like (column alignment) and list-table-like
      (cell alignment), plus colspan / rowspan column tracking.
- [x] Emit `text-align` on cells; drop `<col align>`; colgroup gated on widths.
- [x] Update bd-hixmy colgroup unit tests and the lmd `desugar.main.html.snap`.
- [x] Verify: pampa + quarto nextest, clippy (host + Windows), xtask lint, notes render.
