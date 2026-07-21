# Float/layout DOM class taxonomy (bd-hcp8m3ve)

**Date:** 2026-07-21
**Strand:** bd-hcp8m3ve (feature; unblocks bd-9fz5fweg CSS port)
**Design:** `claude-notes/designs/float-layout-class-taxonomy.md` (the contract)
**Status:** Design drafted — pending alignment on the doc's four open
questions. **Do not start implementation until Carlos signs off.**

## Overview

Q2 emits classless native `<figure>` for crossref figures and passes layout
divs through untouched; the whole figures/floats/layout CSS family is blocked
on a class-taxonomy decision. Decision made: **Q1-verbatim class names**
(minimizes CSS + DOM churn for Q1→Q2 projects). The design doc pins the exact
Q1 DOM shapes (measured from `floatreftarget.lua` / `layout/html.lua`) and
the Q2 mechanism: transform-side construction in `render_float_ref_target`
(Finalization), native nodes only, figcaption metadata carried as stripped
`data-qf-*` kvs on the Figure attr, synthesized identically by the pampa HTML
writer and the preview React renderer.

## Work items

### Phase 0 — Alignment
- [x] Q1 DOM inventory (floatreftarget.lua, layout/html.lua, renderHtmlFigure)
- [x] Design doc with mechanism options + recommendation
- [x] Carlos sign-off on Q1–Q3 (2026-07-21): **drop the figcaption uuid**
      (it's a namespace-collision guard, not a regex hack — sole consumer is
      aria-describedby; Q2 emits `<float-id>-caption` with an AST-wide
      collision check, disambiguating only on real collision); **emit bare
      `quarto-float`** on the outer div; **`data-qf-*` kv scheme accepted**
      (5 kvs: ref-type, caption-location, caption-id, uncaptioned, subfloat —
      full table in the design doc)
- [ ] Q4 still open: table-float DOM change (Phase 2) in the same PR as
      figure floats, or staged separately?

### Phase 1 — Figure floats (TDD)
- [ ] Failing tests: pampa writer snapshot for the full shape-1 DOM;
      crossref_render unit tests for outer-div/figure/figcaption classes,
      fig-align variants, uncaptioned figcaption
- [ ] Implement in `render_float_ref_target` + writer figcaption synthesis
- [ ] `test_build_transform_pipeline_phase_ordering` stays green

### Phase 2 — Table/listing floats (TDD)
- [ ] Replace Div+caption-paragraph with shape 1 (`figure.quarto-float-tbl`)
- [ ] Listings: `listing` class + forced left align

### Phase 3 — Standalone captioned figures (TDD)
- [ ] Shape 2 wrapper (`quarto-figure quarto-figure-<align>`)

### Phase 4 — Preview React renderer parity
- [ ] Figure.tsx figcaption synthesis; writer↔React parity test

### Phase 5 — Verification + handoff
- [ ] Snapshot churn itemized per policy; full workspace + `xtask verify`
- [ ] e2e render + preview inspection
- [ ] Notify bd-9fz5fweg (CSS port) that its DOM exists; file the
      layout-engine sub-strand (shape 3 acceptance)

## Notes
- Layout panels (shape 3) are explicitly out of scope — the layout
  mini-engine gets its own strand, filed at Phase 5.
- Expected snapshot churn: any fixture with crossref figures; phase5
  baseline doc.html should NOT shift (title-only fixture).
- Coordinate with the PN-CSS agent: this strand must not touch
  `_bootstrap-rules.scss`/`highlight.scss`/`title-block.scss` or the phase5
  hashes (their territory) — and it shouldn't need to.
