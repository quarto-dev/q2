---
title: "List-table row attributes from a leading empty span"
date: 2026-11-25
description: "Accept pandoc-ext/list-table row attributes (GH #1336) and keep them through the HTML and qmd writers."
---

Strand: `bd-cp2kje5u`. Issue: <https://github.com/quarto-dev/q2/issues/1336>.

## Overview

pandoc-ext/list-table says: "If the first block element of a table row
contains (only) an empty Span, it is removed and its attributes are
transferred to the table row."

**Before:** `validate_list_table_div` in
`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` required each row
to be exactly one bullet list. A row written as `- []{.myrow}` followed by
its cells had two blocks, so it was rejected with Q-2-35. Separately, the
HTML writer (`write_table_row` in `crates/pampa/src/writers/html.rs`) and
the qmd writer (`write_list_table` in `crates/pampa/src/writers/qmd.rs`)
ignored `Row.attr`.

**After:**

- A new helper, `row_attr_span`, recognizes a `Plain`/`Para` block that holds
  only an empty `Span`. Validation skips it, and `transform_list_table_div`
  moves its attr (and attr sources) onto `Row.attr`. When a row has such a
  span, its source range is a hull over the span and the cells, which keeps
  the attr sources inside the row (tiling containment).
- `write_table_row` emits the row's id, classes, and key-values on `<tr>`.
  The stripe class (`header`/`odd`/`even`) stays first.
- `write_list_table` writes a `[]{...}` line before the cells of a row that
  has attributes. `table_can_use_pipe_format` now returns false for such
  tables, because pipe tables can't express row attributes.

## Work items

- [x] Failing integration tests: `tests/integration/test_list_table_row_attrs.rs`
- [x] Reader: `row_attr_span`, validation, transform
- [x] HTML writer: emit `Row.attr` on `<tr>`
- [x] qmd writer: emit `[]{attrs}`, no pipe format for tables with row attributes
- [x] Roundtrip fixture `tests/roundtrip_tests/qmd-json-qmd/list_table_row_attr.qmd`
- [x] `cargo nextest run -p pampa`, clippy, `cargo xtask lint`
