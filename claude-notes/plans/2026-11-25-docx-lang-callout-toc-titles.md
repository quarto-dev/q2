---
title: "docx: callout titles and the TOC title follow lang"
date: 2026-11-25
description: "GH #1315: localize docx callout titles and the docx TOC title"
---

Strand: bd-shlov07h. Issue: <https://github.com/quarto-dev/q2/issues/1315>

## Overview

With `lang: nl`, docx output kept the English callout titles ("Note") and
TOC title ("Table of Contents"), while HTML localized both.

- The vendored `modules/callouts.lua` reads `param("callout-<type>-title")`
  at the top level of `QUARTO_FILTER_PARAMS`. Q2 sent the terms only nested
  under `language`. `insert_language` in
  `crates/quarto-core/src/pandoc_filters/params.rs` now also copies Q1's
  `languageFilterParams` set (`code-summary`, `toc-title-document`, every
  `callout-*` term) to the top level.
- Pandoc's docx writer titles the TOC from `toc-title` metadata.
  `PandocWriteStage::prepare` (`crates/quarto-core/src/stage/stages/pandoc_write.rs`)
  now fills a missing `toc-title` from `toc-title-document` for docx when
  `toc` is on, as Q1's `pandoc.ts` does.

## Work items

- [x] Top-level language filter params, with a unit test (`nl` fixture)
- [x] Default docx `toc-title`, with unit tests
- [x] Repro render: docx shows "Inhoudsopgave" and "Opmerking"
- [x] Verification (nextest, clippy, Windows cross clippy, xtask lint, notes render)
