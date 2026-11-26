---
title: 'typst: copy bibliography and csl files into output-dir before typst compile'
date: 2026-11-25
description: 'A Typst project render with an output-dir and citeproc false failed, because typst compile resolves the bibliography file next to the .typ in the output directory and nothing copied it there. The resource collector now queues those copies.'
status: in-progress
---

**Braid:** bd-ofy6olsm
**Issue:** <https://github.com/quarto-dev/q2/issues/1011>

## Overview

With `citeproc: false`, the `.typ` carries `#bibliography(("references.bib"))`
(and `#set bibliography(style: ...)` for `csl:`), and `typst compile` resolves
those paths relative to the `.typ`. In a project with `output-dir: _site` the
`.typ` is written into `_site/`, but the copy flush that runs before the
compile only holds image copies: `ResourceCollectorTransform`
(`crates/quarto-core/src/transforms/resource_collector.rs`) walks the AST, and
`bibliography:` lives in the metadata. The compile fails with Q-21-3.

The fix: for Typst renders whose page directory differs from the source
directory, the collector also queues a copy of each local `bibliography:` and
`csl:` file. The source resolves through `config_paths`; the destination is the
path as the `.typ` spells it, joined onto the page directory. Rooted paths (a
leading `/` is project-root-relative under typst's `--root`) and destinations
outside the output root are skipped.

Out of scope: out-of-place Typst renders (a preview or temp request
directory), where the collector returns early and images climb back to the
source instead of being copied.

## Work items

- [x] Failing `ProjectPipeline` test:
      `crates/quarto-core/tests/integration/pandoc_typst_bibliography_output_dir.rs`
      (front-matter bibliography; `_quarto.yml` bibliography with a document
      in a subdirectory).
- [x] Queue the copies in `ResourceVisitor::collect_typst_compile_files`.
- [x] quarto-core tests, clippy, `xtask lint`, and the issue's reproduction
      with `q2 render`.
