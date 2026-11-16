---
title: "Project discovery: skip engine environment directories like Quarto 1"
date: 2026-11-16
description: "GH #1222: q2 render rendered .qmd vignettes installed under renv/library/."
---

Strand: bd-xx608rer · Issue: <https://github.com/quarto-dev/q2/issues/1222>

## Overview

**Current behaviour.** Project discovery
(`crates/quarto-core/src/project/discovery.rs`, `is_excluded_component`)
skips `_`/`.`-prefixed components and `node_modules`, but nothing else.
renv installs R packages under `renv/library/`, and some ship `.qmd`
vignettes, so `q2 render` rendered them as project pages.

**Quarto 1.** `engineIgnoreDirs()` (`src/execute/engine.ts`) is
`node_modules` plus every engine's `ignoreDirs()`: knitr returns
`renv`, `packrat`, `rsconnect` (`src/execute/rmd.ts`) and jupyter
returns `venv`, `env` (`src/execute/jupyter/jupyter.ts`). These are
applied as `**/<dir>/**` during the input walk and subtracted from
explicit `project.render` lists too
(`src/project/project-context.ts`), whichever engines a project uses.

**New behaviour.** Discovery skips all six names at any depth, as a
hard filter (an explicit pattern naming such a file gets `Q-5-13`,
like every other hard exclusion).

## Work items

- [x] Failing unit tests: default discovery and explicit-pattern cases
- [x] Add `IGNORED_DIR_NAMES` and use it in `is_excluded_component`
- [x] Update the module docs and `docs/guides/projects/render-list.qmd`
- [x] Verify (nextest, clippy, Windows cross clippy, xtask lint, notes render)
