---
title: 'docs: document list-table attributes and fix the Q-2-35 advice'
date: 2026-11-25
description: 'The Tables guide was a TBD stub, and the Q-2-35 page told authors to use the same number of cells in every row, which merged cells contradict. Document the list-table and cell attributes and correct the error page.'
status: in-progress
---

**Braid:** bd-cquh3j83

**Issue:** <https://github.com/quarto-dev/q2/issues/1324>

## Overview

Current behaviour: `docs/guides/authoring/tables.qmd` says only "TBD".
The list-table attributes (`header-rows`, `aligns`, `widths`, read by
`transform_list_table_div`) and the per-cell attributes (`colspan`,
`rowspan`, `align`, read from a leading empty span by `extract_cell_attrs`,
both in `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`) are not
documented anywhere. `docs/errors/markdown/Q-2-35.qmd` says rows need
consistent cell counts, but `validate_list_table_div` checks only the
div → rows list → cells list nesting.

New behaviour: the Tables guide documents list tables (structure, caption,
table attributes, cell attributes, merged cells) and pipe tables. The Q-2-35
page describes what the check really rejects and allows merged cells.

## Work items

- [x] Write the Tables guide.
- [x] Correct the Q-2-35 page.
- [x] Check the documented behaviour with a sample render.
- [x] `q2 render --strict` on the changed docs pages and this note.
