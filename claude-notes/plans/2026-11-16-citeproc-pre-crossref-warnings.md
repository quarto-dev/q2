---
title: "Citeproc in .pre: no Q-23-2 for crossref keys"
date: 2026-11-16
description: "An explicit filters list with citeproc warned Citation Key Not Found for every crossref key"
---

Strand: bd-tcgo16qh · Issue: <https://github.com/quarto-dev/q2/issues/1226>

## Overview

**Current behaviour.** An explicit `filters: [citeproc]` puts Q2's citeproc
filter in `.pre`, so it runs in `UserFiltersStage::pre()`, before
`AstTransformsStage` resolves crossrefs. Crossref keys such as `@sec-one`
still reach citeproc as plain `Cite` items, and its unknown-key check
(`pampa::citeproc_filter::apply_citeproc_filter`) warns `Q-23-2` for each.
The crossref itself renders correctly later. Books hit this every time,
because they need the explicit filter (#1157). A single document hits it
too when it uses `filters: [citeproc]`. Without that list, citeproc defaults
to `.post`, which runs after crossref resolution, so no warning appears.

**New behaviour.** Before running a filter list that includes citeproc,
`UserFiltersStage` records the span of every `Cite` item whose id the
document's `RefTypeRegistry` classifies as a crossref
(`crossref_cite_spans`). It then drops the `Q-23-2` warnings reported at
those spans. A genuinely unknown bibliographic key still warns. An
unresolved crossref (`@fig-nope`) still gets crossref's own "unresolved
crossref" warning.

Code: `crates/quarto-core/src/stage/stages/user_filters.rs`.

## Work items

- [x] Failing test: `pre_citeproc_does_not_warn_for_crossref_keys` in
  `crates/quarto-core/tests/integration/unknown_citation_keys.rs`
- [x] Filter crossref-key `Q-23-2` warnings in `UserFiltersStage`
- [x] Check the issue's book repro end-to-end (no warnings, crossref
  renders as "Chapter 1")
- [x] Verify (nextest, clippy, Windows cross clippy, xtask lint, notes render)
