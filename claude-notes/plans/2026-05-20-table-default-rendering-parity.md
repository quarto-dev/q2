---
title: 'Default table rendering parity with Quarto 1'
date: 2026-05-20
---

**Issue:** [bd-hir7j](../../.beads/) — *Default table rendering parity with Quarto 1 (render + preview)*
**Created:** 2026-05-20
**Status:** Proposed
**Reporter context:** carlos.scheidegger asked for render+preview tables to match Quarto 1's defaults.

## Goal

Make a default `q2 render` (and `q2 preview`) of a document containing a
`::: list-table` (or any markdown table) produce visually-equivalent
HTML output to `quarto render` from Quarto 1. The fixture used for the
diagnosis lives at `~/Desktop/daily-log/2026/05/20/tables.qmd` and is
reproduced at the end of this plan.

This work is the *defaults* — themes, custom widths, captions, and grid
tables are out of scope unless one of them is the reason a default
breaks.

## Evidence gathered

Three renders of the same source compared in Chrome DevTools:

1. **Q1** — `quarto render` (Quarto 1, v99.9.9): emits proper Bootstrap
   table.
2. **Q2 render** — `q2 render`: minimal `<table>`, no Bootstrap CSS.
3. **Q2 preview** — `q2 preview` SPA iframe: same minimal DOM as Q2
   render, plus a different CSS bundle that also doesn't style tables.

Reference screenshots: `/var/folders/.../T/q1-render.png`,
`q2-render.png`, `q2-preview.png` (captured 2026-05-20; ephemeral).

### Q1 (target) emits

```html
<body class="fullcontent quarto-light">
...
<table class="caption-top table">
  <thead>
    <tr class="header">
      <th><code>qmd</code> syntax</th>
      <th>Output</th>
    </tr>
  </thead>
  <tbody>
    <tr class="odd"><td>…</td><td>…</td></tr>
    <tr class="even"><td>…</td><td>…</td></tr>
  </tbody>
</table>
```

Stylesheet bundle: Bootstrap (`bootstrap-*.min.css`), quarto-html
(`quarto.css`, `tippy.css`, syntax highlighting).

Computed styles on the table:
- `width: 799px` (stretches to content column)
- `margin: 8.5px 0`
- header `<th>`: `font-weight: 700`, `padding: 8.5px`, `border-bottom: 1px solid #909294`
- body `<td>`: `padding: 8.5px`, `border-top: 1px solid #d3d3d4`
- `caption-side: top`

### Q2 render (current) emits

```html
<body class="fullcontent">
...
<table>
  <colgroup><col><col></colgroup>
  <tbody>
    <tr><td><code>qmd</code> syntax</td><td>Output</td></tr>
    <tr><td>…</td><td>…</td></tr>
    <tr><td>…</td><td>…</td></tr>
  </tbody>
</table>
```

Stylesheet bundle: a single `tables_files/styles.css` (project-shipped
default, see `crates/quarto-core/resources/styles.css` referenced from
`crates/quarto-core/src/resources.rs:51`). No Bootstrap.

Computed styles on the table:
- `width: 393px` (collapses to content)
- `margin: 0`
- every cell: browser default (`padding: 1px`, no borders)
- `caption-side: bottom` (browser default)
- no header row exists

### Q2 preview (current) emits

DOM is identical to Q2 render. CSS bundle is a different file
(`assets/q2-preview-*.css`) but produces the same un-styled result. The
preview iframe goes through the same `crates/pampa/src/writers/html.rs`
emitter, so any markup fix here propagates to both.

## Defect list (each becomes a sub-task)

### D1 — `list-table` first row not promoted to `<thead>`

`transform_list_table_div` in
`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:304` reads a
`header-rows` attribute (default `0`) and only promotes that many rows.
Q1 promotes the first row to `<thead>` by default for list-tables. We
need to decide whether to:

- **(a) Match Q1**: default `header-rows: 1` when the attribute is
  absent.
- **(b) Match Pandoc reST list-table**: keep default `0`, document the
  difference.

The user said *match Q1*. **Choose (a)**. Add a regression test that
list-table with no `header-rows` attr produces `<thead>` with `<th>`.

**Files:**
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:308-313`
  (change default from `0` to `1`)
- Add fixture + snapshot under `crates/pampa/tests/` covering both
  the default case and an explicit `header-rows: 0` opt-out.

### D2 — `<table>` missing Bootstrap `table` / `caption-top` classes

Q1's HTML emission adds `class="caption-top table"` to every table.
This is what unlocks Bootstrap's `.table` styling. Q2's HTML writer
(`crates/pampa/src/writers/html.rs:1367`) emits whatever classes are on
`table.attr` — which for a list-table div with `list-table` filtered
out is the empty set.

**Decision needed:** does the class-injection happen in
- (i) a post-AST transform stage (a Quarto-flavored equivalent of
  TS Quarto's `quarto-bootstrap-table.lua` filter), or
- (ii) the HTML writer itself, gated on output format?

Recommend **(i)** so the qmd writer (round-trip) doesn't see a Quarto
addition. New stage in `crates/quarto-core/src/stage/stages/`:
`html_table_class_transform.rs`. Adds `table` and `caption-top` to
every `Block::Table` attr's class list (deduped).

**Tests:** snapshot of rendered HTML containing the classes; unit test
of the transform that asserts class idempotency (running twice doesn't
duplicate).

### D3 — Empty `<colgroup>` emitted when no widths are set

`crates/pampa/src/writers/html.rs:1383` writes a `<colgroup>` whenever
`table.colspec` is non-empty, even if every entry is
`(Alignment::Default, ColWidth::Default)`. Q1 omits the colgroup in
that case. Suppress when every col is default.

**Tests:** snapshot delta on the bare-list-table fixture.

### D4 — Body row striping (`class="odd"`, `class="even"`) missing

Pandoc's reference HTML writer adds these. Q1 inherits them. Q2's
`write_table_row` (`crates/pampa/src/writers/html.rs:1490`) doesn't.
Add the alternating class to body rows only (head rows get
`class="header"` instead — see D5).

**Tests:** snapshot of a 3-row table verifying odd/even alternation.

### D5 — Header `<tr>` missing `class="header"`

Same writer, same row function. Head rows should emit `<tr class="header">`.

**Tests:** covered by the D4 snapshot.

### D6 — `<body>` missing `quarto-light` / `quarto-dark` class

Q1 sets `quarto-light` (or `quarto-dark`) on `<body>` so theme-conditional
CSS works. Q2's template (`crates/quarto-core/src/template.rs:136` area
talks about body-class logic mirroring TS Quarto) emits only
`fullcontent`. Add the color-mode class based on the active theme
config (default `quarto-light`).

**Tests:** template test asserting the body class for the default
config.

### D7 — Default render stylesheet has no Bootstrap table rules

Even after D1–D6, the table won't *look* right unless the CSS bundle
includes Bootstrap's `.table` and `.caption-top` rules. The project
already ships `resources/scss/bootstrap/` (per CLAUDE.md: SCSS resources
are local). The default HTML format needs to compile/load these into
the per-document `_files/libs/bootstrap/bootstrap-*.min.css` that Q1
emits, *or* equivalent compiled CSS.

This is the largest sub-task and the one most likely to overlap with
existing work — see "Related work" below. Scope here is *just* what
the default render needs to make tables match: a Bootstrap-derived
stylesheet linked from the rendered HTML.

**Tests:** integration test that renders `tables.qmd` and asserts
the produced HTML links a stylesheet containing the `.table` selector.

### D8 — Preview CSS bundle missing the same table rules

The preview SPA (`crates/quarto-preview/` plus the embedded React app
that ships `assets/q2-preview-*.css`) needs the same Bootstrap-table
rules. Two options:

- **(a) Single source**: serve the same stylesheet the render
  pipeline produces. Requires the preview server to know about the
  output bundle for the current document.
- **(b) Bundle separately**: include the same SCSS at build time in
  the preview UI's CSS.

Recommend **(a)** so render/preview can't drift. This is also where
the existing `k-giyy` (WASM-CSS-styling) work intersects — see Related
work.

**Tests:** Chrome DevTools-driven regression in
`hub-client/tests/e2e/` (or wherever preview e2e lives) that loads
`tables.qmd` and asserts computed `font-weight: 700` on `<th>`.

## Related work

- `k-giyy` — *Investigate style differences between WASM and CLI
  rendering.* Same underlying gap from the WASM side. D8 should
  coordinate with it; ideally the artifact-replacement approach
  described in `claude-notes/plans/2025-12-27-wasm-css-styling.md`
  delivers D8 for free.
- `bd-ulgr` — *Design JS dependency handling for Quarto 2 HTML output
  (Bootstrap JS and beyond).* The JS analog of D7. Tables don't need
  JS, so this work is parallel but not blocking.
- `claude-notes/plans/2025-12-05-list-table-implementation.md` — the
  original list-table land. D1 amends a default chosen there.

## Phasing

**Phase 1 — Markup parity (D1–D6).** Cheap, contained, no CSS
pipeline changes. After this phase the *DOM* of a Q2-rendered table
matches Q1's. Visual styling will still be wrong because Bootstrap
isn't loaded, but `diff` of the HTML body will be small.

**Phase 2 — Render-side CSS (D7).** Land a Bootstrap-derived
stylesheet in the default HTML format. After this phase, `q2 render`
of `tables.qmd` looks like Q1.

**Phase 3 — Preview-side CSS (D8).** Make `q2 preview` match. May be
absorbed by `k-giyy`.

## Tests (overall, per CLAUDE.md TDD)

Before *any* code change in each phase:

1. Write a failing test exercising the gap. For D1–D6, snapshot
   tests in `crates/pampa/tests/` (markup-only). For D7, an
   `end-to-end-verification`-grade test that runs the binary via
   `render_document_to_file` and inspects the produced HTML (per
   CLAUDE.md "End-to-end verification before declaring success").
   For D8, a Chrome-DevTools test against a real preview server.
2. Confirm the test fails in the expected way.
3. Implement.
4. Confirm the test passes.
5. Run `cargo nextest run --workspace` to verify no regressions, then
   `cargo xtask verify --skip-hub-build` (or full verify if Phase 3
   touches WASM).
6. Drive the binary on `tables.qmd` (`cargo run --bin q2 -- render
   tables.qmd`) and visually confirm vs. Q1 before closing each
   sub-task.

## Source fixture (for posterity)

```
---
title: Table test
---

::: list-table

* * `qmd` syntax
  * Output

* * ```qmd
    <https://quarto.org>
    ```
  * <https://quarto.org>

* * ```qmd
    [Quarto](https://quarto.org)
    ```
  * [Quarto](https://quarto.org)

:::
```

## Checklist

### Phase 1 — markup parity
- [x] **bd-fyb4z** D1: list-table default `header-rows: 1` + tests
      *(closed 2026-05-20, commit 87b5f236)*
- [x] **bd-2c8rg** D2: `quarto-bootstrap-table` transform stage adds
      `table` + `caption-top` classes + tests
      *(closed 2026-05-20, commit 987bdc9f; transform runs in HTML
      pipeline finalization phase and also in q2-preview)*
- [x] **bd-hixmy** D3: suppress empty colgroup + tests
      *(closed 2026-05-20, commit a91f114a)*
- [x] **bd-12fpz** D4/D5: emit `odd`/`even` body-row classes and
      `header` head-row class + tests (bundled — same code path)
      *(closed 2026-05-20, commit c02a405a)*
- [x] **bd-elgxx** D4/D5 react: same row classes in the preview's
      React Table component (preview doesn't go through the pampa
      writer) *(closed 2026-05-20, commit cad7778c)*
- [x] **bd-mtzry** D6: emit `quarto-light` on `<body>` + template tests
      *(closed 2026-05-20, commit 21c8ec04; dark-mode detection deferred
      until the pipeline supports light/dark theme configs)*
- [x] **bd-tkamn** D6 react: same `quarto-light` append in the preview
      shell's body-class effect (PreviewDocument.tsx)
      *(closed 2026-05-20, commit fa21b5c5)*

### Phase 2 — render CSS
- [x] **bd-dy97y** D7: Bootstrap-derived default stylesheet loaded by
      HTML format
      *(already done in the codebase prior to this epic: the
      `CompileThemeCssStage` ships the full Bootstrap 5.3.1 bundle as
      `_files/styles.css` for default renders. The original gap that
      made the table look unstyled was 100% markup-side — the
      `<table>` was missing the `caption-top table` class hooks
      (D2). Once those landed, the existing CSS styled the table
      identically to Q1: computed `padding: 8.5px`, `font-weight:
      700`, `caption-side: top`, etc. — confirmed via Chrome DevTools
      2026-05-20. Closing without code; verification by inspection.)*
- [x] D7 follow-up: visual re-verify in Chrome
      *(done 2026-05-20: q2 render of the daily-log fixture is
      visually identical to Quarto 1 — see
      `/var/folders/.../T/q2-render-after-phase1.png` vs `q1-render-current.png`.)*
- [x] Edge case for D7: `theme: none` opt-out ships only the minimal
      244-line `crates/quarto-core/resources/styles.css` (no `.table`
      rules). This is correct — the user explicitly opted out of
      Bootstrap. The D2 classes still emit but are inert under
      `theme: none`. No action needed.

### Phase 3 — preview CSS
- [x] **bd-g18wu** D8: align preview-side CSS bundle
      *(already done in the codebase prior to this epic: the
      q2-preview SPA's bundled CSS at
      `q2-preview-spa/dist/assets/q2-preview-*.css` already contains
      Bootstrap's `.table` and `.caption-top` rules. After bd-elgxx
      and bd-tkamn landed the markup, the preview's computed table
      styles match Q1 byte-for-byte: 799px width, 8.5px padding,
      font-weight 700, caption-side top — same as render. The
      `coordinate-with-k-giyy` worry in the original plan was based
      on the wrong premise.)*
- [x] D8 follow-up: visual re-verify in `q2 preview`
      *(done 2026-05-20: preview renders the daily-log fixture
      visually identical to render and to Q1 — see
      `/var/folders/.../T/q2-preview-after-d6-react.png`.)*

### Wrap-up
- [ ] Update `docs/` user-facing notes if any new defaults are
      observable to users (e.g. the header-rows default change).
- [ ] Close parent beads issue with link to the merged work.
