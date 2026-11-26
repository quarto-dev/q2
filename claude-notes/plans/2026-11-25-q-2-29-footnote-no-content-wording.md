---
title: "Q-2-29: stop saying 'indented' for a footnote label with no text"
date: 2026-11-25
description: "Reword Q-2-29 to a neutral title, since three different inputs reach the same parser state."
---

# Q-2-29 wording (GH #947)

Strand: bd-ujjohhgs. Issue: <https://github.com/quarto-dev/q2/issues/947>.

## Overview

A footnote definition `[^1]:` with no text on its line reports Q-2-29
"Indented Footnote Content Not Supported", whether the next line is
indented, unindented, or absent. All three inputs fail in the same
parser state (4385, `_close_block`), so a regex guard would be needed to
tell them apart. The issue allows a neutral shared title instead, which
is what this change does: "Footnote Definition Has No Content On Its
Line", with hints for `[^ref]: Text.` and the `::: ^ref` div form.

Code locations:

- `crates/pampa/resources/error-corpus/Q-2-29.json` (title, message,
  note; new cases `next-line-not-indented` and `empty-label`)
- `crates/pampa/resources/error-corpus/_autogen-table.json` (regenerated)
- `crates/quarto-error-catalog/error_catalog.json`
- `docs/errors/markdown/Q-2-29.qmd`, plus cross-references in Q-2-26
  and Q-2-30

## Work items

- [x] Test: `crates/pampa/tests/integration/test_footnote_no_content.rs`
  (fails on the old table)
- [x] Reword the corpus entry and add the two cases
- [x] Regenerate `_autogen-table.json` (only Q-2-29 entries changed)
- [x] Update the catalog and the docs page
