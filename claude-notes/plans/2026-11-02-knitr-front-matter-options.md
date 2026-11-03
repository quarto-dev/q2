---
title: 'knitr: forward the front-matter knitr map to R (bd-kw0i6h1x)'
date: 2026-11-02
description: 'build_format_config never filled KnitrFormatConfig.metadata, so the knitr opts_chunk and opts_knit options in the front matter were silently dropped; forward the knitr key as format$metadata$knitr.'
braid:
  strand: bd-kw0i6h1x
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1009> · **Strand:** bd-kw0i6h1x ·
**Branch:** `braid/bd-kw0i6h1x-knitr-front-matter-options`.

## Overview

Current behaviour: `execute.R` (`knitr_options`) merges
`format$metadata$knitr$opts_chunk` and `opts_knit` into knitr's options,
as Quarto 1 does. But `build_format_config` in
`crates/quarto-core/src/engine/knitr/mod.rs` builds the config from the
format and the `execute:` scope only and leaves `metadata` as `None`, so
`format$metadata$knitr` is `NULL` in R and a front-matter
`knitr: opts_chunk: comment: "#>"` has no effect.

New behaviour: `build_format_config` reads the `knitr` key from the
merged document metadata already carried on `ExecutionContext.metadata`
and sends it as `metadata.knitr`. Only that key is forwarded: the R
scripts also read `crossref`, `r-options` and `fig-responsive` from
`format$metadata`, and forwarding those is a separate change.

## Work items

- [x] Failing unit test: `build_format_config` forwards `knitr:` as `metadata.knitr`
- [x] Fill `metadata` with the `knitr` key in `build_format_config`
- [x] Update the `ExecutionContext.metadata` doc comment (knitr now reads it)
- [x] Verify: nextest, clippy (host and Windows cross), plan render
