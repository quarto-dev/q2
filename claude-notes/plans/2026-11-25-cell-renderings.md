---
title: 'cell-renderings: choose light/dark cell outputs in HTML, dashboard and Reveal.js'
date: 2026-11-25
description: 'Port of Quarto 1 cell-renderings.lua as a native transform, so a renderings light/dark cell gets light-content and dark-content classes in HTML and dashboard, and keeps only the brand-mode output in Reveal.js.'
status: in-progress
---

**Braid:** bd-2jg82psz ·
**Issue:** <https://github.com/quarto-dev/q2/issues/1130>

## Overview

A cell with `#| renderings: [light, dark]` arrives as a `.cell` div with
`renderings='["light","dark"]'` (forwarded by the presenter,
`presenter/options.rs` `forwarding`) and one `cell-output-display` div per
name. Typst already picks the light output, because its Pandoc filter chain
runs Quarto 1's `quarto-post/cell-renderings.lua`. The native HTML pipeline had
no port, so HTML and dashboard showed both outputs unmarked and Reveal.js kept
both on the slide.

New `CellRenderingsTransform` (`crates/quarto-core/src/transforms/cell_renderings.rs`),
registered just before `code-block-render` in `build_transform_pipeline`:

- HTML and dashboard profiles: keep both outputs, add `light-content` /
  `dark-content`.
- Reveal.js profiles: keep the output for `brand-mode` (default `light`).
- Pandoc profiles: excluded (bucket B4); typst keeps the Lua filter.

## Work items

- [x] Transform + unit tests (html, dashboard, revealjs light/dark, mismatched count, missing dark, Pandoc no-op)
- [x] Register in the pipeline, `PANDOC_TRANSFORM_EXCLUDED`, bucket table, exact-name-list tests
- [x] End-to-end check with `q2 render` for html, revealjs and dashboard
