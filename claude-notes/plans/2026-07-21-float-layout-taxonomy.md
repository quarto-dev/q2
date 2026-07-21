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
- [x] Q4 (Carlos, 2026-07-21): table-float DOM change ships **in the same
      PR** — Q2 is 0.x; no backwards-compat obligation on its own output yet.
      kv names approved as-is.

### Phase 1 — Figure floats (TDD) — DONE
- [x] Failing tests first (5 new crossref_render unit tests: shape, align,
      collision, uncaptioned, table shape), then implementation in
      `render_float_ref_target` (format-gated via
      `ctx.format.identifier.is_html_based()`) + pampa writer figcaption
      synthesis (`read_qf_caption_kvs`, 4 new integration tests)
- [x] `test_build_transform_pipeline_phase_ordering` green; full workspace
      10365/10365; **zero snapshot churn** (phase5 fixture is title-only)
- [x] revealjs auto-stretch taught the float wrapper shape
      (`is_float_figure_div`; `count_figure_images`/`figure_image_mut`
      descend the aria wrapper; id transfers from the outer div)
- [x] e2e-caught guard: only genuine float kinds (`fig`/`tbl`/`lst`) get the
      float DOM — `sec`/`demo`/custom FloatRefTargets keep the legacy
      pass-through (regression test `section_ref_target_is_not_float_wrapped`;
      the whole 10k-test suite had NOT caught sections being swallowed into
      `quarto-float-sec` figures — only the kitchen-sink e2e render did)

### Phase 2 — Table/listing floats (TDD) — DONE
- [x] Tables render shape 1 (`figure.quarto-float-tbl` + aria wrapper +
      synthesized figcaption); same-PR per Q4 decision
- [ ] Listing (`lst`) float DOM is wired (`listing` class + left align) but
      has no e2e fixture exercising `#lst-` floats yet — add one

### Phase 3 — Standalone captioned figures (TDD) — PENDING
- [ ] Shape 2 wrapper (`quarto-figure quarto-figure-<align>`) for
      non-crossref `![caption](img)` figures

### Phase 4 — Preview React renderer parity — DONE
- [x] Figure.tsx figcaption synthesis mirroring the pampa writer (4 new
      integration tests); preview-renderer suites 538 + 565 green;
      hub-client tsc/vite build green

### Phase 5 — Verification + handoff
- [x] Snapshot churn: none. Full workspace green. e2e render inspected
      (kitchen-sink + revealjs fixtures; float DOM verbatim, sections clean)
- [ ] Preview inspection needs the WASM rebuild chain (npm run build:wasm →
      build-q2-preview-spa → cargo build --bin q2) before `q2 preview` shows
      the new DOM
- [ ] Notify bd-9fz5fweg (CSS port) that fig/tbl float DOM exists; file the
      layout-engine sub-strand (shape 3 acceptance)
- Discovered: bd-hb9a9ik8 — figcaption drops the "Figure 1: " prefix in real
  renders (pre-existing; prefix_caption only handles Paragraph captions)

## Notes
- Layout panels (shape 3) are explicitly out of scope — the layout
  mini-engine gets its own strand, filed at Phase 5.
- Expected snapshot churn: any fixture with crossref figures; phase5
  baseline doc.html should NOT shift (title-only fixture).
- Coordinate with the PN-CSS agent: this strand must not touch
  `_bootstrap-rules.scss`/`highlight.scss`/`title-block.scss` or the phase5
  hashes (their territory) — and it shouldn't need to.
