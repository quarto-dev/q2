---
title: 'Jupyter: honor eval false'
date: 2026-11-17
description: 'The jupyter engine ran cells marked eval false; skip them as Quarto 1 does, keeping the code and no output.'
---

**Strand:** bd-0pjyqhhp · **Issue:** <https://github.com/quarto-dev/q2/issues/1247>

## Overview

`execute_blocks_inner` in `crates/quarto-core/src/engine/jupyter/text_execute.rs`
resolves each cell's options (cell over the document's `execute:` scope) but
reads only `error`. Every planned cell goes to the kernel, so a cell with
`eval: false`, set on the cell or the document, still runs, and a failing one
aborts the render. The only use of `eval` was to exempt such cells from the
unrunnable-language report (`cell_eval_disabled`).

Quarto 1 (`src/resources/jupyter/notebook.py`, `cell_execute`) checks
`cell_options.get("eval", eval_default)` and skips the kernel call when it is
false. The cell keeps its source and has no outputs, so it presents as code
with no output.

New behavior: when the resolved `eval` is false, the executor does not send the
cell to the kernel and records an empty, successful result for it. The
presenter then shows the code (subject to `echo`) and nothing else.

## Work items

- [x] Tests in `crates/quarto-core/tests/integration/engine_error_policy.rs`:
  cell-level and document-level `eval: false` on a raising cell render, show
  the code, and carry no output; fail before the fix.
- [x] Skip execution for resolved `eval: false` in `execute_blocks_inner`.
- [x] Verify: quarto-core tests, clippy (host + Windows), xtask lint, notes
  render, end-to-end `q2 render` of the issue's two repros.
