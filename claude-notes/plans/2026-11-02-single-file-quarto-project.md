---
title: "quarto.project paths in a single-file render"
date: 2026-11-02
description: Give Lua filters the document's directory as quarto.project outside a project, as Quarto 1 does.
---

Strand: bd-1aqlupdj · Issue: https://github.com/quarto-dev/q2/issues/1013

## Overview

`host_project` (`crates/quarto-core/src/lmd/dialect.rs`) returned `None`
when `ProjectContext::is_single_file` was set, so a Lua filter in a
single-file render saw `quarto.project.directory`, `offset` and
`output_directory` as `nil`. The comment claimed that was Quarto 1's
behaviour; it is not.

Quarto 1 falls back to `singleFileProjectContext` and exports its
directory as `QUARTO_PROJECT_DIR`, which `quarto.project.directory` reads
(`render-shared.ts`, `init.lua`). Checked with Quarto 1.12 on
`sub/index.qmd`, rendered from both the parent directory and `sub/`:
`directory` and `output_directory` are the absolute path of `sub`, and
`offset` is `.`.

The single-file `ProjectContext` already has `dir` and `output_dir` set
to the document's directory, so the fix drops the early return.
`HostContext::project` stays an `Option`. `None` now means only "no host"
(pampa's own tests and the conformance suite).

## Work items

- [x] Flip the CLI integration test (`lua_host_context_cli.rs`) to Q1's
      values, with the document in a subdirectory of the cwd, and watch it
      fail
- [x] Drop the single-file early return in `host_project`, update its unit
      test
- [x] Fix the stale "nil outside a project" comments in `pampa`
- [x] Verify: nextest (quarto-core, pampa, quarto), clippy (host + Windows
      cross), strict render of this note
