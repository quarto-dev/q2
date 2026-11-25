---
title: 'crossref: a custom cite prefix replaces the kind in HTML'
date: 2026-11-25
description: 'A crossref written as [Fig. @fig-a] rendered as "Figure 1" in HTML because the native renderer ignored the cite prefix slot. Use the prefix in place of the kind, as Q1 and the pandoc shim do.'
status: in-progress
---

**Braid:** bd-rkl1red0 · **Issue:** <https://github.com/quarto-dev/q2/issues/1318>

## Overview

`[Fig. @fig-a]` rendered as `Figure 1` in HTML. `crossref_resolve.rs`
already keeps the citation prefix as the `cite_prefix` slot of the
`CrossrefResolvedRef` node, and the pandoc shim (docx and other pandoc
formats) already uses it. The native renderer, `render_resolved_ref` in
`crates/quarto-core/src/transforms/crossref_render.rs`, ignored it and
always used the kind.

Now the slot's inlines replace the kind, followed by a non-breaking space
and the number (`Fig.` + `\u{a0}1`), matching Q1's `refs.lua`. Markup in
the prefix (`[*see* @sec-x]`) survives. `sec_ref_text` now returns the
label and number separately so that `sec` refs get the same treatment.

The docx half of the issue (a doubled space) was already fixed on `main`:
pampa trims the trailing space from the citation prefix.

## Work items

- [x] Unit test `resolved_ref_uses_cite_prefix_in_place_of_kind` (fails before the fix)
- [x] Use the `cite_prefix` slot in `render_resolved_ref`; split `sec_ref_text`'s result
- [x] End-to-end HTML render of the issue's repro
