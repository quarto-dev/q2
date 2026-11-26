---
title: 'hub: sync .bib and .csl files in project mode'
date: 2026-11-25
description: 'Project file discovery skipped bibliography and citation-style files, so the hub never synced them and documents with citations failed to render through it.'
status: in-progress
---

**Braid:** bd-zfss7xpk
**Issue:** <https://github.com/quarto-dev/q2/issues/1103>

## Overview

`q2 hub --project <dir>` decides which files to sync with `classify()` in
`crates/quarto-project-files/src/discovery.rs`. That function synced `.qmd`,
`.md`, config files, `.tsx`, `.scss`, binary resources, and `_extensions/`,
but not `.bib` or `.csl`. A document with `bibliography: refs.bib` therefore
failed to render through the hub (and through `q2 mcp`), with
`Bibliography file '.../refs.bib' not found`.

The fix adds `bib` and `csl` to the `TextSource` arm, next to `tsx` and
`scss`, so both are synced as text. The MCP render step
(`ts-packages/quarto-hub-mcp/src/render.ts`) already writes every synced
text file to disk, and the language server's disk layer uses the same
classification, so nothing else needs to change.

Two `session_runtime` tests in `quarto-core` used `refs.bib` as their
example of a file the classification leaves out. They now use `data.csv`.

## Work items

- [x] Flip the `refs.bib` assertions in the `classify` tests and add `.csl` (fails first)
- [x] Add `bib` / `csl` to the `TextSource` arm and update the doc comments
- [x] Move the `session_runtime` tests to an unclassified `data.csv`
- [x] Verify (`cargo xtask verify --skip-hub-build`, clippy, Windows clippy, lint, notes render)
- [x] PR
