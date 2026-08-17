# Tabsets (panel-tabset) are not implemented — tab titles leak into the TOC (bd-toc-tabset-titles-zq93gjvf)

**Date:** 2026-08-17
**Braid:** bd-toc-tabset-titles-zq93gjvf (feature, p2, label `html`)
**Branch:** `braid/bd-toc-tabset-titles-zq93gjvf-panel-tabset-support` (in the main checkout, off `main` @ `a29b22ca`, per user)
**Status:** Implemented 2026-08-17 — all phases complete, verified end-to-end (see § E2E evidence). Pending review/merge.

## Triage verdict

**Ready to design.** The strand is fresh (filed today, re-verified against current HEAD), unusually well-researched (root cause, Q1 reference implementation, verbatim target markup, and a committed minimal repro are all in the description), and the codebase has a directly analogous pattern to mirror (the callout CustomNode pair). The open questions are scoping/policy choices, not unknowns.

## Issue context

Q2 has **no tabset support at all**: a `::: {.panel-tabset}` Div passes through as a plain `<div class="panel-tabset">` with its Headers rendered as ordinary stacked headings. Two consequences:

1. No tab UI — all tab contents stacked and visible, nothing clickable.
2. **TOC pollution** (the discovery symptom): the tab-title Headers survive to TOC time, so every tab name appears in the table of contents. In the repro, Q1's TOC has 2 entries; q2's has 4.

The TOC collector is *not* at fault — `collect_toc_entries` (crates/pampa/src/toc.rs:341) correctly recurses into non-section Divs. Q1 has no TOC-exclusion logic either; its tabset filter simply consumes the Headers before the TOC is built. The fix reproduces that ordering, not a TOC special case.

**Grouped tabsets are in scope, not a nice-to-have** — explicit requirement from the Connect docs port: `group="language"` etc. syncs every same-group tabset on the page and persists the choice in localStorage. ~166 of ~185 tabsets in the Connect docs are grouped. Real-world impact: ~115 of 352 Connect-docs pages have tabsets; this is the single largest chrome-sweep noise source in that port (120 of 123 differing pages in the 0.21.0 triage).

## Dependency graph

**Empty** — no edges in this skein. The `discovered-from` context lives in the separate connect-docs porting skein (`br-toc-tabset-titles-e47klspv`), which the description already distills. No incoming `blocks` pressure inside q2; the urgency comes from the Connect docs port externally.

## What the code looks like today

Spot-checked at HEAD (`60cc579e`, same commit the strand was verified against today):

- The only `panel-tabset` in quarto-core is a negative test in `crates/quarto-core/src/transforms/callout.rs:831` (asserting the class is not a callout). Nothing to fix, only to add — confirmed.
- **The pattern to mirror is the callout pair**: `CalloutTransform` (Div → `CustomNode("Callout")`, `TransformPhase::Normalization`) at `crates/quarto-core/src/pipeline.rs:1232`, immediately followed by `CalloutResolveTransform` (CustomNode → structured Divs/RawInlines) at `:1233`. Both run **before** `SectionizeTransform` (`:1317`) and `TocGenerateTransform` (`:1371`) — a tabset pair in the same neighborhood consumes the tab-title Headers before sectionize/TOC ever see them, fixing the TOC for free.
- **JS-shipping pattern exists**: `BootstrapJsStage` (`crates/quarto-core/src/stage/stages/bootstrap_js.rs`) and especially `ClipboardJsStage` (`clipboard_js.rs`) — embed the file from `resources/js/<name>/` via `include_bytes!`, store a Project-scoped `js:<key>` artifact (gated on `!is_minimal_html` + a metadata/content predicate), and `ApplyTemplateStage` emits `<script>` tags in sorted-key order. `bootstrap.bundle.min.js` already ships on every Bootstrap-themed page, so basic tab switching (`data-bs-toggle="tab"`) needs **no new JS** once the markup exists. Only the grouped-sync module is a new JS asset.
- Q1 reference implementation read in full:
  - `external-sources/quarto-cli/src/resources/filters/customnodes/panel-tabset.lua` (368 lines; ~half is Lua proxy-metatable machinery q2 doesn't need). Parse: find first Header inside the Div, its level defines tab boundaries; each same-level Header starts a tab (title = header inlines, `active` = header has `.active` class, default active = first tab). Render: `<ul class="nav nav-tabs" role="tablist">` built from RawInlines + title inlines, then a `tab-content` Div (the original attr with `panel-tabset` class swapped for `tab-content`... note: Q1 actually emits an *outer* `panel-tabset` div wrapping nav + `tab-content` — see captured markup) holding one `tab-pane` Div per tab. Ids: `tabset-<N>-<M>` with a page-global counter N.
  - Grouped-sync module: Q1's `site_libs/quarto-html/tabsets/tabsets.js` (95 lines, captured at `claude-notes/plans/tabset-panel-tabset-investigation/q1-tabsets-sync-reference.js`). It is an ES module whose `init()` is called by Q1's `quarto.js` (`import * as tabsets` / `tabsets.init()`); q2 has no quarto.js, so the port should self-initialize (mirror `code-copy-init.js`). Sync key is the tab's `innerHTML` value; group comes from `div[data-group]`; persistence key `quarto-persistent-tabsets-data`.
- **Repro captured locally** at `claude-notes/plans/tabset-panel-tabset-investigation/` (`index.qmd`, `_quarto.yml`, `q1-target-markup.html` with the exact Q1 TOC + tabset markup to match). Original lives in the external q2-connect-docs repo; copied per the external-fixtures policy.

Symptom confirmed current: strand description re-verified failing on q2 0.20.0/0.21.0/0.22.0 and notes origin/main `60cc579e` (today's HEAD) has no tabset work; grep confirms.

## Design decisions (aligned with user, 2026-08-17)

1. **Resolve timing:** mirror the callout pair exactly — parse + resolve back-to-back in Normalization, at the pipeline.rs:1232 neighborhood. "To the extent that the callout structure can be mirrored, we should."
2. **Format scope:** Bootstrap HTML only. Minimal-HTML and non-HTML formats keep today's passthrough (stacked headings; TOC pollution persists there). A full cross-format AST-cleanliness story waits for the first large non-HTML q2 format.
3. **Revealjs:** follow-up strand, filed as `discovered-from` this one — **bd-revealjs-tabsets** (see § Follow-ups).
4. **JS gating:** ship the sync module **always alongside bootstrap JS** (same gate as `BootstrapJsStage`, i.e. `!is_minimal_html`). No content-based flag needed — the module is small and harmless without tabsets.
5. **`tabset-margin-container`:** deferred.
6. **SCSS / visual parity:** in scope — tabs must look like Q1's. A dedicated chrome-devtools visual-parity phase is part of this plan (Phase 5); chase Q1 tabset-specific SCSS if the rendered result diverges.

## Phases

### Phase 0 — Test plan (TDD: failing tests first)

- [x] Repro + grouped variant as test fixtures. *(Done as inline qmd bodies in the test file, matching `bootstrap_js_pipeline.rs` conventions, rather than a `tests/fixtures/tabsets/` directory — the fixtures are single documents, and the website-fixture directory pattern is for multi-file projects.)*
- [x] New integration test `crates/quarto-core/tests/integration/tabset_pipeline.rs`, registered (alphabetized) in `tests/integration/main.rs`, driving the end-to-end render path (`render_page_in_project`-style helper, realistic config — not `HtmlRenderConfig::default()`). Assertions:
  - TOC has exactly 2 entries (`Real heading`, `Another real heading`; no `Tab Alpha`/`Tab Beta`),
  - output contains the captured Q1 markup shape: outer `div.panel-tabset` wrapping `ul.nav.nav-tabs[role=tablist]` (nav-link ids `tabset-1-1-tab` …, `data-bs-toggle="tab"`, aria attrs) + `div.tab-content` with `div.tab-pane` panes,
  - first tab is `active` on both nav-link and pane; an explicit `.active` header wins over first-tab default,
  - grouped variant emits `data-group="language"` on the outer div,
  - page `<head>`/scripts include the tabsets JS alongside bootstrap JS.
- [x] Unit tests colocated in the transform files: parse (tab boundaries by first-Header level, deeper headers stay inside tab content, nested tabsets, `.active` class, no-Header degenerate case warns + passes through), resolve (markup, per-document id counter, aria wiring).
- [x] Run the integration test and **verify it fails** at HEAD.

### Phase 1 — `PanelTabsetTransform` (parse)

- [x] `crates/quarto-core/src/transforms/panel_tabset.rs`: Div with class `panel-tabset` → `CustomNode("Tabset")`. Slots: per-tab title Inlines + content Blocks; `plain_data`: level, actives, `group` (from the Div's `group` attribute). Recurse into nested blocks first (as callout.rs does) so nested tabsets work.
- [x] Degenerate case (no Header inside): warn (mirror Q1's warning) and leave the Div untouched.
- [x] Register in `build_transform_pipeline` immediately after `CalloutTransform`/`CalloutResolveTransform` (pipeline.rs:1232-1233), phase `Normalization`; keep the phase-ordering test green.

### Phase 2 — `PanelTabsetResolveTransform` (render)

- [x] `panel_tabset_resolve.rs`: CustomNode → the captured Q1 markup (`q1-target-markup.html` is the contract): outer `Div.panel-tabset` (+`data-group` when present) containing `Plain[RawInline <ul>…nav…]` with title inlines spliced between RawInlines, then `Div.tab-content` of `Div.tab-pane` panes with `tabset-N-M` ids.
- [x] Per-document tabset counter (reset per render — idempotence tests must stay green).
- [x] Self-gate to Bootstrap HTML (`ctx.format.is_html_based()` + not minimal); non-HTML/minimal formats: transform doesn't run → passthrough per decision 2. (Check whether the *parse* half should also self-gate so passthrough keeps the original Div; simplest consistent choice: gate both halves identically.)

### Phase 3 — Tabsets sync JS

- [x] Port `q1-tabsets-sync-reference.js` to `resources/js/tabsets/tabsets.js`, converted from ES module to self-initializing script (mirror `resources/js/clipboard/code-copy-init.js`).
- [x] New `TabsetsJsStage` mirroring `BootstrapJsStage`/`ClipboardJsStage`: `include_bytes!`, Project-scoped artifact `js:tabsets`, gate = same predicate as `BootstrapJsStage` (`!is_minimal_html`), path `tabsets.js` / `quarto/tabsets.js`. (`js:bootstrap` < `js:tabsets` sorts correctly for load order.)
- [x] Register the stage where the other JS stages live; unit tests mirroring `bootstrap_js_pipeline.rs`.

### Phase 4 — End-to-end verification

- [x] `cargo run --bin q2 -- render` on the repro; diff `_site/index.html` TOC + tabset markup against `_site-q1`. Record invocation + output snippet here.
- [x] Render a grouped page (e.g. the Connect docs upgrade page fixture shape) and inspect `data-group` output.
- [x] Browser check (chrome-devtools): tabs click-switch via bootstrap; same-group tabsets sync; choice persists in localStorage (`quarto-persistent-tabsets-data`) across reload.
- [x] Full workspace: `cargo nextest run --workspace` green (12253 passed) after one intentional baseline recapture (see §E2E evidence). Full `cargo xtask verify` run at phase end. No `.snap` files changed.

### Phase 5 — Visual parity (chrome-devtools)

- [x] Side-by-side Q1 vs q2 render of the repro in the browser; computed styles on `.panel-tabset .nav-tabs` / `.nav-link.active` / `.nav-link` / `.tab-pane` are **identical on every checked property** (border widths/colors, radius, padding, font-size, colors, margins). Screenshots visually indistinguishable in the tabset region.
- [x] No divergence — stock Bootstrap SCSS (already vendored) covers tabsets fully; no new SCSS needed.

### Phase 6 — Docs

- [x] User-facing tabsets page at `docs/guides/authoring/tabsets.qmd` (usage, `.active`, `group=` sync; includes a live demo tabset), registered in the docs sidebar; rendered with q2 and inspected (TOC clean, nav-tabs + tabsets.js present).
- [x] The two parse warnings (no-tabs-found; leading-content-dropped) ship **without** `Q-*` catalog codes, matching existing codeless-warning precedent (e.g. `categories_sidebar.rs`); no error-docs pages required.

## E2E evidence (2026-08-17)

Invocation: `cargo run --bin q2 -- render <scratch>/tabset-e2e` (the repro
project copied from the investigation dir). Observed `_site/index.html`:

- TOC `<nav id="TOC">` contains exactly `Real heading` + `Another real
  heading` (no tab titles) — matches Q1's committed render.
- Tabset markup matches the captured Q1 contract byte-shape:
  `<ul class="nav nav-tabs" role="tablist">` … `<a class="nav-link active"
  id="tabset-1-1-tab" data-bs-toggle="tab" data-bs-target="#tabset-1-1"
  role="tab" aria-controls="tabset-1-1" aria-selected="true" href="">Tab
  Alpha</a>` … `<div class="tab-content">` with `tab-pane` panes carrying
  `role="tabpanel"`/`aria-labelledby`.
- Scripts: `site_libs/quarto/bootstrap.bundle.min.js` and
  `site_libs/quarto/tabsets.js` both present; both files on disk.

Grouped check (two `group="language"` tabsets, real render + Chrome via
devtools MCP): initial state one active tab per tabset with inactive panes
`display:none`; clicking the second tabset's "R" tab switched **both**
tabsets (bootstrap toggle + sync), localStorage
`quarto-persistent-tabsets-data` = `{"language":"R"}`; after reload the R
tab remained selected in both. Output inspected directly.

Workspace suite: 12253 passed / 0 failed. One intentional baseline update:
`crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`
doc.html hash re-captured because every Bootstrap-themed page now ships the
tabsets.js script tag (recapture note added in the fixture; verified the
only delta is that one `<script>` line; styles.css hash unchanged). No
`.snap` snapshot files changed.

## Post-implementation decision: q2-preview exclusion

The tabset pair is **excluded from the q2-preview transform pipeline**
(`Q2_PREVIEW_TRANSFORM_EXCLUDED`). The resolve half builds its nav from
split RawInlines (Q1's exact technique, correct for the string-concatenating
HTML writer), but q2-preview's React `RawInline` component renders each
fragment via its own `dangerouslySetInnerHTML` span — unbalanced fragments
auto-close and the tab structure collapses. Excluding both halves keeps the
hub preview at the pre-tabset passthrough (stacked headings). This fits the
deny-list's stated criterion ("opt out only when the output is HTML-only")
and the mermaid-render/crossref-render precedent. The WASM `format: html`
preview pipeline (full HTML string) is unaffected and renders tabs
statically (bootstrap-js is excluded from WASM, so no click behavior there —
pre-existing policy shared with callout-collapse/code-copy).

## Follow-ups

- **Revealjs tabsets** — filed as **bd-y5j0m776** (`discovered-from` this strand), p3.
- **q2-preview React Tabset component** — filed as **bd-47afd5ro**
  (`discovered-from` this strand), p2: keep the parse half in preview and
  render `CustomNode("Tabset")` with a real React component.

## Risks / tradeoffs (draft)

- **Preview parity.** The transform pair lives in `build_transform_pipeline`, shared with preview/WASM — good. But the `TabsetsJsStage` is a *stage*; need to confirm the preview pipeline picks up the JS artifact (same question the clipboard/bootstrap stages already answered — follow their preview story). The preview-pipeline shape contract (`claude-notes/designs/transform-pipeline-phases.md`) must be respected.
- **Id determinism.** `tabset-N-M` uses a page-global counter — fine per render, but idempotence tests (`tests/integration/idempotence.rs`) may care about re-render stability; counter must reset per document.
- **Sync-key fragility (inherited from Q1).** Grouping syncs by tab-title `innerHTML`; q2's inline rendering of titles must byte-match across pages for sync to work (it should, but worth an e2e check on a real Connect docs pair).
- **`.active` markup subtlety.** Q1 strips the `active` class from the pane's attr handling in `paneAttribs` (heading attr forwarded); check we don't double-emit `active` on both nav link and pane incorrectly.
- Q1's outer markup keeps `class="panel-tabset"` on the wrapper div AND uses `tab-content` inside (the Lua reads as if it renames the class, but the committed render shows an outer `panel-tabset` div wrapping `<ul>` + `<div class="tab-content">`). **Match the committed render, not a reading of the Lua** — the captured `q1-target-markup.html` is the contract.
