---
title: 'HTML: pad short table rows with empty cells'
date: 2026-11-25
description: 'A table row with fewer cells than the table has columns ended early in HTML output. Pad it with empty cells, as Pandoc does, counting colspan and rowspan coverage.'
status: in-progress
---

**Braid:** bd-zh4zae0d
**Issue:** <https://github.com/quarto-dev/q2/issues/1337>

## Overview

Current behaviour: `write_table_row` in `crates/pampa/src/writers/html.rs`
writes only the cells a row has, so a short row (`| d |` under a
three-column header) ends early.

New behaviour: after the last cell, the row is padded with empty cells
(`<td>`, or `<th>` in header rows) for every column up to
`colspec.len()` that no rowspan from an earlier row still covers. The
`pending` rowspan tracking from GH #1327 already provides that coverage.
Each padded cell takes its column's alignment, as in Pandoc.

## Work items

- [x] Unit test `short_rows_are_padded_with_empty_cells` next to the
      span-alignment tests in `html.rs` (fails before the fix)
- [x] Pad rows in `write_table_row`
- [x] `cargo nextest run -p pampa`, clippy (host and Windows), `xtask lint`,
      note render
