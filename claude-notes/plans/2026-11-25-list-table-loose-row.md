---
title: 'pampa: write a loose list-table row with blank lines between its cells'
date: 2026-11-25
description: 'The qmd writer wrote every list-table row tight, so a cell holding a paragraph re-read as plain text. Loose rows now get a blank line between cells.'
status: in-progress
---

**Braid:** bd-9rlinewv
**Issue:** <https://github.com/quarto-dev/q2/issues/1158>

## Overview

Each list-table row is one inner bullet list, so the CommonMark
loose/tight rule (#1098) decides whether a cell's text reads as `Para` or
`Plain`. `write_list_table` in `crates/pampa/src/writers/qmd.rs` never put
a blank line between a row's cells, so a row with a `Para` cell came back
tight and the `Para` became `Plain`.

Now the writer checks each row's cells with the existing `list_is_tight`
helper (as `write_bulletlist` does for its items), and writes a blank line
between the cells of a loose row. Tight rows are written as before.
`list_is_tight` now takes an iterator of block slices so the row check
doesn't copy cell content.

## Work items

- [x] Round-trip fixture `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/list_table_loose_row.qmd` (fails before the fix)
- [x] Blank line between cells of a loose row in `write_list_table`
- [x] pampa tests, clippy (host and Windows), `xtask lint`, notes render
