# `_quarto-rules.scss` selector inventory (bd-eias3e39)

**Audit strand:** bd-eias3e39 · **Epic:** bd-4doe9lvt · **Date:** 2026-07-21

Source: `external-sources/quarto-cli/src/resources/formats/html/_quarto-rules.scss`
(774 lines, 144 depth-0 rule blocks — full list at
`claude-notes/plans/quarto-rules-scss-audit-investigation/top-level-selectors.tsv`).
Rows below are **per family** (~29), per design discussion with Carlos; the TSV
is the per-selector appendix.

## Methodology

1. **SCSS presence**: token grep over `resources/scss/bootstrap/*.scss` and
   `resources/scss/html/templates/*.scss` (dist/ and themes/ excluded), then
   rule-level read of hits to distinguish "same rule" from "same token,
   different rule".
2. **DOM emission**: token grep over `crates/pampa/src` and
   `crates/quarto-core/src` (tests excluded), plus source reads of
   `transforms/crossref_render.rs`, `transforms/code_block_render.rs`,
   `transforms/mermaid.rs`, `transforms/footnotes.rs`, `template.rs`.
3. **Ground truth**: end-to-end render of
   `claude-notes/plans/quarto-rules-scss-audit-investigation/kitchen-sink.qmd`
   via `cargo run --bin q2 -- render …` (2026-07-21, main @ `04882745`);
   emitted HTML inspected directly. Per design discussion, grep + kitchen-sink
   is the accepted evidence bar for BLOCKED verdicts (no per-feature fixture
   attempts for unimplemented features).

**Categories:** `AP` already-present · `PN` port-now (DOM emitted, rule
missing) · `BE` blocked-on-emitter · `ID` intentionally-dropped.

## Key structural findings

- Q2 renders crossref figures as **native** `<figure id="fig-N"><img/><figcaption>`
  with **no class taxonomy** — no `.quarto-figure` wrapper, no
  `.quarto-float-caption-*`, no alignment variants
  (`crossref_render.rs::render_float_ref_target`, confirmed in fixture).
  Tables/listings render as plain `Div` + caption paragraph (deliberate
  "avoids needing a CSS class taxonomy right away" comment at
  `crossref_render.rs:232`). The whole figure/float/layout family is therefore
  blocked on a *taxonomy decision*, not on individual emitters.
- Q2's `_bootstrap-rules.scss` is largely a port of Q1's **`_quarto.scss`**
  (page layout), and already contains **dead CSS** for DOM Q2 doesn't emit:
  `.quarto-layout-cell[data-ref-parent]`, `.tippy-content > *`,
  `.code-annotation-*` (~70 lines), `#quarto-embedded-source-code-modal`,
  `.panel-input`, `.layout-sidebar`, title-block `.code-tools-button`. Not
  this epic's scope (different source file), but the pattern to avoid.
- **Task lists are broken, not just unstyled**: `- [ ] todo` renders as
  `<li><span></span> todo item</li>` / `<li><span>x</span> done item</li>` —
  no `<input type="checkbox">`, no `ul.task-list` class. Filed as a pampa bug
  (see strand table). The revealjs SCSS layer styles `.task-list`, so reveal
  presumably assumed the DOM exists — it doesn't for HTML output.
- Engines are further along than the epic assumed: both
  `crates/quarto-core/src/engine/jupyter/` and `…/knitr/` exist, and knitr's
  `hooks.R` already emits `cell-output-display` / `code-overflow` tokens. So
  engine-output styling (ANSI, gt, knitsql, widgets) is **one port strand
  verified against executed fixtures**, not far-future blocked work.

## Inventory

| # | Family | Q1 lines | Q2 SCSS | Q2 DOM emitted? | Verdict | Strand |
|---|--------|----------|---------|-----------------|---------|--------|
| 1a | `.hidden`, `.visually-hidden` | 12–30 | absent | user-authored classes pass through; no Q2 a11y emitter uses them yet | **PN** (cheap, live for authored content) | misc |
| 1b | `.top-right`, `.zindex-bottom` | 3–7, 28–30 | `.top-right` only as compound in dead color-toggle rules | Q1-JS-driven (color toggle, back-to-top); no Q2 emitter | **BE** (client-JS features) | backlog |
| 2 | `figure.figure` | 34–36 | absent | Bootstrap `.figure` class not emitted, marginal authored use | **ID** — bootstrap's own `.figure` styling suffices if authored | — |
| 3 | Layout panels: `.quarto-layout-{panel,row,cell,valign-*}`, `.panel-caption`, `.table-caption p` | 38–103 | fragments in `_bootstrap-rules` (dead) | **no** — `::: {layout-ncol=2}` renders as `<div data-layout-ncol="2">` passthrough (fixture) | **BE** (float/layout taxonomy) | blocked-floats |
| 4a | `.quarto-figure*` wrappers, alignment variants, `figcaption.quarto-float-caption-*`, `div[id^="tbl-"]` positioning | 105–130, 140–145, 151 | absent | **no** — native `<figure>` without classes; `<div id="tbl-t">` *is* emitted but the rule exists for anchorjs | **BE** (float/layout taxonomy) | blocked-floats |
| 4b | `figure > p:empty`, `figure > p:first-child` | 132–138 | absent | Q2 figures have no `<p>` children (img is direct child) | **ID** — DOM shape differs; rules can't match | — |
| 5 | anchorjs (`.anchorjs-link` rules) | 155–177 | absent | no anchor.js in Q2 | **BE** (anchor-links feature) | backlog |
| 6a | `#title-block-header` base | 179–183 | **ported** (bd-btjkyylx, `title-block.scss`) | yes | **AP** | — |
| 6b | `.abstract` margin, `.abstract-title` weight | 185–191 | covered in `title-block.scss` (Q2 emits `.block-title` instead of `.abstract-title` and styles it) | yes | **AP** (adapted) | — |
| 6c | `#title-block-header a`, `.author/.date/.doi` margins | 193–201 | absent | yes — `.date` emitted; links possible in title meta | **PN** | title-block |
| 6d | `.quarto-title-block > div` flex + button | 203–223 | button CSS present (dead) | no — code-tools button not emitted | **BE** (code-tools) | backlog |
| 7 | Tables base: `tr.header > th > p`, `table` margins, `caption` padding | 226–241 | absent | yes — `<table class="caption-top table">`, `<caption>`, `<tr class="header">` all in fixture | **PN** (`.table-caption` half of line 236 is BE) | tables |
| 8 | `figure.quarto-float-tbl` captions | 243–253 | absent | no — tables aren't figure-wrapped | **BE** (float/layout taxonomy) | blocked-floats |
| 9 | `.utterances` | 256–259 | absent | no comments feature | **BE** | backlog |
| 10 | `iframe` margin | 262–264 | absent | raw-HTML iframes pass through | **PN** (cheap) | misc |
| 11 | `details`/`summary` rules | 267–282 | absent | raw-HTML `<details>` passes through (fixture); code-fold emitter is an explicit Phase-3 TODO in `code_block_render.rs:182` | **PN** (live for authored content; code-fold will land on it) | misc |
| 12 | `div.code-copy-outer-scaffold` | 285–287 | **present** (`copy-code.scss:58`) | yes (`code_block_render.rs`) | **AP** | — |
| 13a | inline `code:not(.sourceCode)` pre-wrap (`p`, `dd`) | 291–294 | only the `td` variant exists (`_bootstrap-rules` ~1294) | yes | **PN** | code |
| 13b | bare `code { white-space: pre }` + `@media print` pre-wrap | 299–306 | only `code.sourceCode` scoped (`highlight.scss:30`) | yes | **PN** | code |
| 13c | `pre > code { display:block }` | 307–309 | **present** (`highlight.scss`) | yes | **AP** | — |
| 13d | `$code-white-space` themable var | 311–313 | hard-coded `pre` | yes | **PN** (port the var) | code |
| 13e | line-anchor `::before` rule | 315–317 | absent | line-numbers emission unverified | audit-in-strand | code |
| 13f | `pre.code-overflow-{wrap,scroll}` | 319–325 | absent | no — option unimplemented (only knitr `hooks.R` mentions it) | **BE** (code-overflow option) | backlog |
| 13g | `code a:any-link` / `:hover` | 328–335 | absent | authored links in code pass through | **PN** (cheap) | code |
| 14 | `ul.task-list` + `input[type=checkbox]` margin | 338–340, 697–699 | revealjs layer only | **broken** — see key findings | **BE on bug fix** | task-list bug |
| 15a | `.footnote-back` margin | 352–354 | absent | **yes** (`transforms/footnotes.rs`; fixture: `<a class="footnote-back" role="doc-backlink">`) | **PN** | misc |
| 15b | tippy rules (`[data-tippy-root]`, `.tippy-content*`) | 344–350, 356–358 | partial dead CSS in `_bootstrap-rules:1650` | no tippy JS in Q2 | **BE** (hover footnotes) | backlog |
| 16 | `.quarto-embedded-source-code` | 361–363 | modal CSS present (dead) | no code-tools feature | **BE** | backlog |
| 17 | `.quarto-unresolved-ref` | 366–368 | absent | Q2 emits `<a class="quarto-xref">?fig-nope?</a>` instead — visible failure without the class | **PN with emitter tweak** (add class or restyle `quarto-xref`) | misc |
| 18 | `.quarto-cover-image` | 371–375 | absent | no books/cover injection | **BE** | backlog |
| 19 | Engine output: `.widget-subarea`, `.cell-output-display` overflow, `.knitsql-table`, `div.ansi-escaped-output` + ~36 ANSI color classes, `table.gt_table` (7 rules) | 378–387, 447–587, 601–647 | absent | engine-produced DOM; jupyter + knitr engines exist, knitr hooks already emit `cell-output-display` | **one engine-output strand**, ported against executed fixtures | engine-output |
| 20 | `.panel-input`, `.layout-sidebar`, `.tab-content > .page-columns.active` | 389–414 | partial dead CSS | no OJS/shiny/tabsets | **BE** | backlog |
| 21 | `div.sourceCode > iframe` (code-preview) | 417–433 | absent | no code-preview feature | **BE** | backlog |
| 22 | `a { text-underline-offset: 3px }` | 436–438 | absent | trivially yes | **PN** | misc |
| 23 | `.callout pre.sourceCode` padding | 442–444 | absent | yes — callouts + sourceCode both emitted (fixture) | **PN** | code |
| 24 | `:root { --quarto-* }` vars | 589–598 | absent | `:root` always matches; consumed by gt rules + downstream | **PN** (infra; port with engine-output or standalone) | vars-print |
| 25 | `div.columns` / `div.column` | 653–662 | absent | **yes** — authored `::: {.columns}` renders `<div class="columns">` (fixture) | **PN** | misc |
| 26 | code-annotation rules | 665–693 | **present** (`_bootstrap-rules:2249–2325`) | no emitter — inverse case: CSS ported ahead of feature | **BE** (CSS already staged) | backlog |
| 27 | Mermaid theming (`$mermaid-*`, `:root --mermaid-*`) | 701–742 | absent | mermaid feature landed 2026-07 (`transforms/mermaid.rs`, `<pre class="mermaid">`) but zero theming CSS | **own strand** (per design discussion) | mermaid |
| 28 | `@media print` block (11pt root, hide `#quarto-sidebar`/`#TOC`/`.nav-page`, `.fixed-top`, caption color) | 744–764 | only `.nav-page` + page-columns pieces | TOC/sidebar DOM emitted when enabled (`template.rs`, `sidebar_render.rs`) | **PN** (partial) | vars-print |
| 29 | `body.quarto-light .dark-content` / `body.quarto-dark .light-content` | 768–774 | absent | `body.quarto-light` always emitted (`template.rs:715`); `.dark-content` authored | **PN** (light half live now; dark half activates with dark-mode feature) | misc |

## Verdict counts (by family)

AP 4 · PN 13 (incl. partials) · BE 11 · ID 2 · special: task-list bug,
mermaid own-strand, engine-output own-strand.

## Strand key → filed strands

All children of epic bd-4doe9lvt except where noted
(`braid dep tree bd-4doe9lvt` is authoritative):

- **code** — bd-u5yvsdgw — Code CSS parity (13a,b,d,e,g + 23)
- **tables** — bd-dxgcpl02 — Table base CSS parity (7)
- **misc** — bd-28iqotrt — Misc element CSS parity (1a, 10, 11, 15a, 17, 22, 25, 29)
- **vars-print** — bd-ih6jrf39 — `:root` vars + print block (24, 28)
- **title-block** — bd-iq08mmnh — Title-block remainder (6c)
- **engine-output** — bd-18410csp — Engine output styling (19)
- **mermaid** — bd-sehm2rha — Mermaid theming (27), related bd-5m4ga0s1
- **blocked-floats** — bd-9fz5fweg — Figures/floats/layout CSS (3, 4a, 8),
  `blocks`-dep on bd-hcp8m3ve
- **taxonomy feature** — bd-hcp8m3ve — Float/layout DOM class taxonomy
  (feature, `related` to the epic, not a child — it's emitter work, not CSS)
- **task-list bug** — bd-obkvhlam — pampa task-list rendering (14),
  discovered-from bd-eias3e39 (not an epic child)
- **backlog** — bd-q36vnfdp — CSS blocked on unimplemented Q1 features (1b, 5,
  6d, 9, 13f, 15b, 16, 18, 20, 21, 26); split per-feature when an emitter
  gets planned

## Port conventions (from design discussion)

- Destination: the thematically-right **existing** Q2 layer (no mirrored
  `_quarto-rules.scss` counterpart file — Q1 drift is not expected to warrant
  mechanical re-diffing). Each ported rule gets a provenance comment
  `// ported from _quarto-rules.scss:<lines> (<strand>)` so future agents can
  audit coverage mechanically.
- Each port strand follows the bd-btjkyylx template: failing
  compile-output assertion in `crates/quarto-sass/src/compile.rs` first →
  port → re-capture `phase5-single-doc-baseline/expected_hashes.txt` with a
  dated comment → end-to-end `q2 render` grep of emitted CSS.
