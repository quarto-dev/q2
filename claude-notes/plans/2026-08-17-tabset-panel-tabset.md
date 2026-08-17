# Tabsets (panel-tabset) are not implemented — tab titles leak into the TOC (bd-toc-tabset-titles-zq93gjvf)

**Date:** 2026-08-17
**Braid:** bd-toc-tabset-titles-zq93gjvf (feature, p2, label `html`)
**Branch:** written on `main` @ `60cc579e` (investigation only — implementation branch/worktree TBD by user)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).**
  - Copy the repro into `crates/quarto-core/tests/fixtures/` (integration-test layout rules apply: new test file goes in `tests/integration/` + `main.rs` registration).
  - Failing integration test: render the fixture end-to-end (`render_page_in_project`-style helper), assert (a) TOC has exactly 2 entries, (b) output contains `nav nav-tabs` / `tab-pane` markup matching the captured Q1 shape, (c) grouped variant emits `data-group`.
  - Unit tests for parse (Div → CustomNode: tab boundaries, nested tabsets, `.active` class, no-Header degenerate case) and resolve (CustomNode → markup, id assignment, aria attributes).
- **Phase 1 — `PanelTabsetTransform`** (Div.panel-tabset → `CustomNode("Tabset")`, Normalization phase, placed alongside `CalloutTransform` at pipeline.rs:1232ff). Slots for per-tab title inlines + content blocks; plain_data for level/actives/group.
- **Phase 2 — `PanelTabsetResolveTransform`** (CustomNode → Bootstrap nav-tabs markup per the captured target). Page-global tabset counter for `tabset-N-M` ids. `data-group` on the outer div when `group` attr present.
- **Phase 3 — Grouped-tabset sync JS.** Port tabsets.js to `resources/js/tabsets/` as a self-initializing script; new `TabsetsJsStage` mirroring `ClipboardJsStage` (artifact `js:tabsets`), gated on the page actually containing a tabset (flag set by the transform, mechanism TBD — see Q4).
- **Phase 4 — End-to-end verification** against the repro *and* a grouped page from the Connect docs; browser check that clicking tabs works and group sync + localStorage persistence behave. Snapshot-change report per CLAUDE.md policy.
- **Phase 5 — Docs.** `docs/` user-facing page for tabsets (usage, groups).

## Open design questions for the user

1. **Resolve timing.** Mirror the callout pair exactly (resolve immediately after parse, both in Normalization, pipeline.rs:1232-1233)? Tab *content* stays ordinary Blocks inside the resolved `tab-pane` Divs, so crossrefs/floats inside tabs still get processed by later phases either way. The alternative — resolve late in Finalization like other presentation transforms — seems unnecessary here since callouts already set the precedent for early resolve. My recommendation: mirror callouts (parse at :1232 neighborhood, resolve immediately after), because the Header consumption *must* happen before sectionize/TOC regardless.
2. **Non-Bootstrap / non-HTML formats.** Q1 has renderers for non-Bootstrap HTML ("tabby") and degrades to stacked headers elsewhere. Scope this strand to Bootstrap HTML only, with minimal-HTML and non-HTML formats keeping today's passthrough (stacked headings)? Note passthrough means the TOC pollution persists in those formats — acceptable? Alternative: parse everywhere, and have a fallback resolve that re-emits the headers *without* TOC pollution... which requires TOC-aware handling; simplest honest scope is Bootstrap-HTML-only.
3. **Revealjs.** Q1 supports tabsets in reveal. In scope here or a follow-up strand? (Connect docs port doesn't need reveal.) My recommendation: follow-up strand, filed as discovered-from.
4. **JS gating.** Ship `js:tabsets` (the sync module) (a) always alongside bootstrap JS, (b) only when the page has any tabset, or (c) only when a *grouped* tabset exists? Q1 ships it unconditionally via quarto.js. Cross-page consistency argues for (b) at least — the module is harmless without groups. How the flag flows from transform to stage needs a mechanism (RenderContext field? scan the rendered AST in the stage like other stages do?).
5. **`tabset-margin-container`.** Q1 emits an empty `<div class="tabset-margin-container">` sibling for margin-content hoisting. Strand says deferrable — confirm we defer (q2 margin-content handling is AST-side anyway)?
6. **SCSS.** Q1's Bootstrap SCSS already styles `.nav-tabs`/`.tab-pane` (vendored in `resources/scss/`), so no new styles expected — but if the rendered repro looks off, is matching Q1's tabset-specific SCSS (if any) in scope?

## Risks / tradeoffs (draft)

- **Preview parity.** The transform pair lives in `build_transform_pipeline`, shared with preview/WASM — good. But the `TabsetsJsStage` is a *stage*; need to confirm the preview pipeline picks up the JS artifact (same question the clipboard/bootstrap stages already answered — follow their preview story). The preview-pipeline shape contract (`claude-notes/designs/transform-pipeline-phases.md`) must be respected.
- **Id determinism.** `tabset-N-M` uses a page-global counter — fine per render, but idempotence tests (`tests/integration/idempotence.rs`) may care about re-render stability; counter must reset per document.
- **Sync-key fragility (inherited from Q1).** Grouping syncs by tab-title `innerHTML`; q2's inline rendering of titles must byte-match across pages for sync to work (it should, but worth an e2e check on a real Connect docs pair).
- **`.active` markup subtlety.** Q1 strips the `active` class from the pane's attr handling in `paneAttribs` (heading attr forwarded); check we don't double-emit `active` on both nav link and pane incorrectly.
- Q1's outer markup keeps `class="panel-tabset"` on the wrapper div AND uses `tab-content` inside (the Lua reads as if it renames the class, but the committed render shows an outer `panel-tabset` div wrapping `<ul>` + `<div class="tab-content">`). **Match the committed render, not a reading of the Lua** — the captured `q1-target-markup.html` is the contract.
