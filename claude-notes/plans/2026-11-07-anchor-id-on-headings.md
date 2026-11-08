---
title: "Heading anchor links: emit data-anchor-id on section headings"
---

Strand: bd-d5gss9pz (discovered from bd-5kf2dnw4, which ported
anchor-sections).

## Overview

Reported discrepancy: since anchor-sections landed, q2 renders

```html
<section id="overview" class="section level2">
<h2 class="anchored">Overview</h2>
```

where Quarto 1 renders

```html
<section id="overview" class="level2">
<h2 class="anchored" data-anchor-id="overview">Overview</h2>
```

AnchorJS takes a heading's anchor from its `id`, then its
`data-anchor-id`, and otherwise urlifies the text and de-duplicates it
against every id in the document. `overview` belongs to the `<section>`,
so the link becomes `#overview-1`. That fragment only exists after
AnchorJS runs and disagrees with the TOC.

### Root cause

- `pampa::transforms::sectionize` moves the heading id to the section
  Div, just as Pandoc's `--section-divs` does in Q1.
- Q1 copies it back in the Bootstrap postprocessor
  (`src/format/html/format-html-bootstrap.ts:534-544`). For every
  `section[class^="level"]`, it sets `data-anchor-id=<section.id>` on
  the first `h2`, `h3`, … `h6` descendant. Sections are visited in
  document order, so a nested section's own pass overwrites anything an
  ancestor wrote. In the end, each section's own heading carries the
  section's id.
- The q2 port (`crates/quarto-core/src/transforms/anchor_sections.rs`)
  covered `format-html.ts` (the `anchored` class) and the AnchorJS init,
  but not this postprocessor step.

A related omission: `resources/js/quarto-toc/quarto-toc.js` left out
Q1's TOC-click anchor reveal (`quarto.js:120-140`). When a TOC link is
clicked, Q1 finds `[data-anchor-id="<fragment>"]` and adds
`reveal-anchorjs-link` until mouseout. The port skipped it because
there was no `[data-anchor-id]` to find. The CSS rule
(`_bootstrap-rules.scss:1905`) is already ported.

## Design

In `AnchorSectionsTransform`, during the same block walk: for a `Div`
whose classes include `section` and whose id is non-empty, if its first
block is a `Header` of level 2–6, set the attribute `data-anchor-id`
to the section id. The HTML writer emits `data-*` keys unchanged.

- **Leading header only.** sectionize always puts the section's own
  header first, so this gives the same final result as Q1's
  document-order overwrite, without the overwrite. A level-1 section
  gets nothing on its `h1`, and its nested `h2` gets its own id from its
  own section. That matches Q1.
- **Independent of `anchor-sections` and `.no-anchor`.** Q1 sets the
  attribute on every Bootstrap page whatever those options say, so q2
  does too. The DOM matches Q1's, and the attribute does nothing without
  AnchorJS. In the transform, the data-anchor-id walk therefore runs
  before the `anchors_enabled` early return, still gated on HTML
  render/preview and non-minimal pages (the Bootstrap condition).
- **Empty id.** Skipped. Q1 would write `data-anchor-id=""`, which would
  make AnchorJS link to a bare `#`. sectionize gives headings
  auto-generated ids, so this only happens if a section really has no
  id.
- **Idempotent.** The attribute is overwritten in place, never
  appended.
- **Render + preview.** Both profiles, as with the class, to keep the
  `dom-parity` smoke tests honest.

TOC reveal: inside `bind()` in `quarto-toc.js`, attach Q1's click
listener to each TOC link and remove it in the teardown, which the
MutationObserver rebind needs. Look the heading up per click (the
preview re-renders the body) with `CSS.escape` on the fragment. Update
the "deliberate deviations" header comment.

## Work items

- [x] Reproduce on main (`q2 render` of a two-heading doc: no
      `data-anchor-id`)
- [x] Unit tests in `anchor_sections.rs` (4 of 6 new tests failed
      before the fix; the 2 negative ones passed throughout): leading
      h2/h3 get the section id; h1 section untouched; nested sections
      each get their own; `.no-anchor` heading and `anchor-sections:
      false` still get it; empty id skipped; idempotent; leading block
      only; PDF / minimal untouched
- [x] Implement in `anchor_sections.rs` (one pre-order `walk_blocks`
      shared by both passes), update module docs (item 4)
- [x] Smoke-all `crates/quarto/tests/smoke-all/anchor-sections/data-anchor-id.qmd`
      with `dom-parity: true`. Native runner passes and fails on a
      deliberately wrong selector. WASM parity passes, so the preview
      DOM carries the attribute too.
- [x] Port the TOC-click reveal into `quarto-toc.js` and update its
      header. Decodes the fragment before comparing (Q1 compared it raw).
      Playwright `chrome.spec.ts` › "clicking a TOC link reveals the
      heading anchor" passes in Chromium and Firefox and fails with the
      old JS.
- [x] Fallout in existing tests (all expected, none in product code):
      - three lmd insta snapshots
        (`crates/quarto/tests/integration/snapshots/lmd/{blog,desugar,project}.main.html.snap`)
        differ only by the new attribute (checked by diffing with it
        stripped);
      - `include_heading_id_dedup.rs`'s `ids_with_base` scanned for the
        substring `id="`, which also matched `data-anchor-id="`; it now
        requires ` id="`;
      - `toc_markup.rs` precondition asserts the exact `<h2>` markup.
      `search/capture.rs`'s `restore_heading_ids` uses lol_html, so it
      is unaffected.
- [x] Appendix headings: q2's `section`-class match also covers
      `section#footnotes`. Q1's `[class^="level"]` misses it, so Q1's
      Footnotes anchor is an invented `#footnotes-1`. Kept as a
      deliberate improvement and documented in the module docs.
- [x] Workspace nextest: 19179/19180 pass. The one failure,
      `engine::knitr::capture_tests::capture_classifies_pieces` (error
      class `rlang_error` vs `simpleError`), comes from local R packages
      (evaluate 1.0.5 / knitr 1.51 / rlang 1.2.0). This change touches no
      knitr code.
- [x] `cargo xtask verify --skip-hub-build --skip-rust-tests`: all steps pass. Windows clippy cross-check cannot build `aws-lc-sys` from macOS; the change has no platform-specific code.
- [ ] PR, CI green
