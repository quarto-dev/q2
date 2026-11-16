---
title: "Engines: run code cells in the document's directory"
date: 2026-11-13
description: "GH #1202. Code cells ran in the project root; Quarto 1 runs them in the document's directory unless project execute-dir is project."
---

Strand: bd-tkqk1vwz · Issue: <https://github.com/quarto-dev/q2/issues/1202>

## Overview

**Before.** The engine-execution stage built every `ExecutionContext` with
`cwd` set to the project root (`crates/quarto-core/src/stage/stages/engine_execution.rs`).
Jupyter and TS-protocol engines start their kernels there, so a page in
`sub/` that opens `data.csv` failed. knitr ignored `cwd` but passed the
project root (its own `determine_working_dir`) as knitr's `root.dir`, with
the same effect. `project: execute-dir` was never read.

**After.** Matches Quarto 1 (`render-files.ts`: `cwd = flags.executeDir ||
dirname(source)`; `command/render/project.ts`: `execute-dir: project` sets
`executeDir` to the project dir). A new `resolve_execute_dir` returns the
document's directory, or the project root when `_quarto.yml` sets
`project: execute-dir: project`. knitr now passes `ctx.cwd` as its root
dir; the Rscript process still runs in the project dir (or the document's
dir inside an renv), as in Quarto 1's `callR`.

**Out of scope.** `q2 render --execute-dir` is declared but not passed to the
render pipeline (nor are its `--execute-*` siblings); filed as
bd-rhdc3jb8.

## Work items

- [x] `resolve_execute_dir` helper and unit tests (`engine_execution.rs`)
- [x] Use it for `ExecutionContext.cwd` in the engine-execution stage
- [x] knitr: pass `ctx.cwd` as knitr's root dir (`engine/knitr/mod.rs`)
- [x] Integration test `tests/integration/execute_dir.rs` (file-backed fixture engine)
- [x] Manual check of the issue's repro with Python and R, default and `execute-dir: project`
- [x] File bd-rhdc3jb8 for the unwired `--execute-dir` flag
