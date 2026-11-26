---
title: "pampa: warn on a list-table cell that starts with a non-empty colspan/rowspan span"
date: 2026-11-25
description: "A list-table cell reads colspan and rowspan only from an empty leading span. A span with content kept the attributes silently; it now warns Q-2-61 and suggests the empty-span form."
status: in-progress
---

**Braid:** bd-mzgvwa2h
**Issue:** <https://github.com/quarto-dev/q2/issues/1338>

## Overview

`extract_cell_attrs` (`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`)
takes a cell's `colspan`/`rowspan`/`align` from an *empty* span at the
start of the cell (`[]{colspan=2} text`). A span with content
(`[text]{colspan=2}`) stays an ordinary span, so the merge silently
doesn't happen and HTML gets `<span colspan="2">`.

New behaviour: `desugar_div` (`crates/pampa/src/pandoc/desugar.rs`)
warns Q-2-61 for each such cell of a valid list-table, pointing at the
span and suggesting `[]{colspan=2} text`. The table is built as before.
Because `desugar_div` is shared by the reader and the structural
desugaring set, both paths warn.

## Work items

- [x] `find_misplaced_cell_span_attrs` helper in postprocess.rs
- [x] Q-2-61 warning in `desugar_div`
- [x] Q-2-61 entry in `error_catalog.json` and `docs/errors/markdown/Q-2-61.qmd`
- [x] Unit tests (desugar.rs) and reader tests
      (`tests/integration/test_list_table_cell_span.rs`)
