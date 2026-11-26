---
title: 'list-table: honor header-cols as body row-head columns'
date: 2026-11-25
description: 'A list-table ignored header-cols and passed it through as a table attribute. The reader now stores it as the body row-head column count, and the HTML and qmd writers honor it.'
status: in-progress
---

**Braid:** bd-0r0d9u5s · **Issue:** <https://github.com/quarto-dev/q2/issues/1335>

## Overview

In pandoc-ext/list-table, `header-cols=N` (default 0) makes the first N
columns of the table body row heads, which HTML renders as `<th>`.

Before: `transform_list_table_div`
(`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`) parsed only
`header-rows`. `header-cols` stayed in the table attributes (written as
`data-header-cols`), `TableBody.rowhead_columns` was hardcoded to 0, and
`write_table_row` (`crates/pampa/src/writers/html.rs`) wrote `<th>` only
for head rows.

After: the reader consumes `header-cols` into `rowhead_columns`. The HTML
writer writes `<th>` for body cells that start in a column below it. The
qmd writer (`crates/pampa/src/writers/qmd.rs`) writes `header-cols` back
on a list-table, and doesn't pick the pipe-table form for a table with
row heads, so the value survives a qmd round trip. Previously it
round-tripped as an ordinary attribute.

## Work items

- [x] Tests: JSON desugar (`test_json_div_transforms.rs`), HTML writer
      unit tests, qmd sugar round trip (`incremental_writer_tests.rs`)
- [x] Reader: parse and consume `header-cols`
- [x] HTML writer: `<th>` for body row-head cells
- [x] qmd writer: write `header-cols`, avoid the pipe form
- [x] Verify (pampa tests, clippy, Windows cross clippy, lints, notes)
