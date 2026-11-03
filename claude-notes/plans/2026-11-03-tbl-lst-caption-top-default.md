---
title: 'crossref: table and listing captions default to the top'
date: 2026-11-03
description: 'Float caption location falls back to the Quarto 1 per-category default (tables and listings on top) instead of bottom for every reference type.'
---

**Strand:** bd-fqvvmh2s · **Issue:** <https://github.com/quarto-dev/q2/issues/1030>

## Overview

Current behaviour: `CrossrefRenderTransform`
(`crates/quarto-core/src/transforms/crossref_render.rs`) reads the caption
location from the float's `cap-location` / `<ref>-cap-location` attribute and
otherwise falls back to `bottom` for every reference type. A `: Caption.
{#tbl-a}` table therefore renders its `figcaption` below the table, with
`quarto-float-caption-bottom`, while the table itself carries `caption-top`.

New behaviour: the fallback follows Quarto 1's per-category default
(`categories.all` in `quarto-cli/src/resources/filters/mainstateinit.lua`):
`tbl` and `lst` put the caption on top; `fig` and custom float types put it
below. A new helper `default_caption_location` holds the mapping. Explicit
attributes still win.

Out of scope: reading `cap-location` / `tbl-cap-location` from document
metadata. That remains with the cap-location feature, as the existing code
comment says.

## Work items

- [x] Unit test for `default_caption_location`, plus a `top` assertion on the
  existing table-float shape test (both fail before the fix)
- [x] Add `default_caption_location` and use it as the fallback
- [x] `cargo nextest run -p quarto-core`
- [x] `cargo xtask verify` (hub/trace-viewer skipped), host clippy; Windows
  cross clippy blocked by C build scripts (aws-lc-sys, tree-sitter, wasmtime)
- [x] Render this note with `q2 render --strict`
