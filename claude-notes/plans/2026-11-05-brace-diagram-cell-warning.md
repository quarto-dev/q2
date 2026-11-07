---
title: "Warn on Quarto 1 brace-form diagram cells"
date: 2026-11-05
description: "Q-2-58: a suppressible warning for mermaid and dot cells written with braces, which q2 renders as code blocks."
---

Strand: bd-p7ionkmv. Issue: <https://github.com/quarto-dev/q2/issues/1067>.

## Overview

A Quarto 1 `{mermaid}` or `{dot}` cell renders as a code block whose class is
the literal `{mermaid}`, with no diagnostic. Now a new transform,
`brace-diagram-cell-diagnostics`
(`crates/quarto-core/src/transforms/brace_diagram_cell_diagnostics.rs`), warns
`Q-2-58` at the class, naming the fix (drop the braces for Mermaid; Graphviz is
not available yet for dot).

It runs after engine execution, so a cell an engine claims is already replaced
and never warns. One code covers both languages, so a project whose engine
takes the brace form can set `diagnostics: {Q-2-58: off}`.

The class value keeping its braces is unchanged; the issue calls it unintended,
but changing it would alter engine cell detection (`engine_cell_lang`).

## Work items

- [x] Transform plus unit tests, registered as B1 in `pipeline.rs`
- [x] Catalog entry, `docs/errors/markdown/Q-2-58.qmd`, docs nav, diagrams guide link
- [x] Verify (nextest, clippy, Windows clippy, strict doc renders)
