# toc-location option (left/right/body); unlocks banner toc-left class (bd-e2kpwy7n)

**Date:** 2026-08-14
**Braid:** bd-e2kpwy7n
**Branch:** investigated on `main` (worktree `.worktrees/bd-nn2fou8h-execute-visibility`, reused after its strand merged)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

User-stated scope note: `external-sources/quarto-cli` is context, not a
contract — what matters is a mechanism that renders TOCs in alternative
locations (`left` first among them), not byte-for-byte Q1 parity.

## Triage verdict

**Ready to design.** The gap is confirmed at HEAD, the Q1 mechanism is fully
mapped, the q2 insertion points are identified, and the SCSS + template hooks
are already ported and inert. The remaining work is choosing the q2-native
placement mechanism (design questions below).

## Issue context

Q2 has no `toc-location` option — the TOC always renders in the right margin
sidebar (`#quarto-margin-sidebar`). Filed 2026-07-17 as a P2 feature,
follow-up from the title-block parity epic (the banner `toc-left`
header-class hook was ported inert). On 2026-08-14 it gained real-world
impact data from the Posit Connect docs port: `api/index.html` is a 1.8 MB
OpenAPI reference whose only navigation is a 201-entry TOC with
`toc-location: left` — the port's worst page in chrome comparisons, and the
only page site-wide where Q1 emits `#quarto-sidebar` and q2 emits none.

Committed minimal repro (external):
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/toc-location-left/`
(re-verified unimplemented at 0.19.0/0.20.0/0.21.0 and HEAD `3ac596e0`).
Local copy of the same fixture:
`claude-notes/plans/toc-location-investigation/repro/`.

## Dependency graph

- **discovered-from**: bd-y71ga2l8 (closed) — title-block parity Phase 7
  (docs + follow-up strands). The title-block work ported the banner
  `banner-header-class` template hook verbatim but left it inert because
  its only Q1 producer derives `toc-left` from `toc-location`, which q2
  lacked. This strand is the missing producer.
- No incoming `blocks` edges; no children. Priority pressure comes from the
  Connect-docs port (origin strand br-toc-location-left-q7hl5jgj in that
  repo's skein), not from the q2 graph.

## What the code looks like today

The file references in the strand are current at HEAD:

- `crates/quarto-core/src/template.rs:213-235` — `FULL_HTML_TEMPLATE`
  hardcodes the TOC into `<div id="quarto-margin-sidebar">` on the right;
  `#quarto-content` classes are baked in (no `toc-left` variable).
- `crates/quarto-core/src/template.rs:337` — the inert banner hook:
  `$if(quarto-template-params.banner-header-class)$` on
  `#title-block-header`.
- `crates/quarto-core/src/transforms/title_banner.rs:41` — documents the
  deliberately-unported `toc-left` producer.
- `crates/quarto-core/src/transforms/toc_render.rs` — renders
  `navigation.toc` → `rendered.navigation.toc` (inner `<ul>` only; entries
  carry `nav-link` + `data-scroll-target` unconditionally).
- `crates/quarto-core/src/transforms/sidebar_render.rs` — renders the
  website sidebar → `rendered.navigation.sidebar` (a complete
  `nav#quarto-sidebar` fragment via `quarto_navigation::sidebar_to_html`)
  and writes `rendered.navigation.body-classes` = `"nav-sidebar
  floating|docked"`; skips entirely when `navigation.sidebar` is absent.
- `render_with_compiled_template` (`template.rs:697-733`) — body-class
  precedence: user override → sidebar body-classes → empty-when-TOC →
  `fullcontent`.
- SCSS is **already ported** in `resources/scss/bootstrap/_bootstrap-rules.scss`:
  `.page-columns.toc-left` grids (wide/mid/narrow), `.sidebar.toc-left`
  placement, `#quarto-sidebar-toc-left` responsive hiding, sticky rules.
- Preview: `ts-packages/preview-renderer/src/q2-preview/chromeSlots.tsx`
  `TocSlot` hardcodes `#quarto-margin-sidebar` too (shape contract with the
  template).
- No `toc-location` schema entry anywhere in q2.

Symptom re-confirmed at HEAD (2026-08-14, `main` after PR #530):
`cargo run --bin q2 -- render .` on the local repro puts `nav#TOC` inside
`#quarto-margin-sidebar` and emits zero `#quarto-sidebar` (or
`#quarto-sidebar-toc-left`) elements — output inspected, see
`toc-location-investigation/repro/README.md`.

Pre-flight note: `cargo xtask verify --skip-hub-build` at HEAD had exactly
one failure, `quarto-preview::integration
config_endpoint::config_reports_embedded_asset_manifest_hashes` — a stale
local `q2-preview-spa/dist/` (built before `f366cb5d` introduced
`spa-manifest.json`). `cargo xtask build-q2-preview-spa` regenerated the
manifest and the test passes; unrelated to this strand.

## How Q1 does it (mechanism summary, from external-sources/quarto-cli 1.10.15)

Full agent report notes below are the source for these line refs.

- **Option**: `toc-location` enum `["body", "left", "right", "left-body",
  "right-body"]`, default `right`, HTML-doc formats only
  (`src/resources/schema/document-toc.yml:36-52`).
- **Mechanism**: templates emit a `div#quarto-toc-target` placeholder; a DOM
  postprocessor (`format-html-bootstrap.ts:342-412`) moves `nav[role=doc-toc]`
  into it (adding `.toc-active`, `nav-link`, `data-scroll-target`,
  collapse classes). q2 must re-express this as template/transform logic
  (no-DOM-postprocessor rule).
- **Two layout regimes for `left`:**
  - *Standalone/article path* (`before-body-article.ejs`): `#quarto-content`
    gets class `toc-left`; a `div#quarto-sidebar-toc-left.sidebar.toc-left`
    holds the TOC; grid comes from `.page-columns.toc-left` mixins gated on
    `body:not(.floating):not(.docked)`. The (empty) `#quarto-margin-sidebar`
    is still emitted and later gets `zindex-bottom`.
  - *Website path* (`nav-before-body.ejs` + `sidebar.ejs`): the TOC target
    goes **inside `nav#quarto-sidebar`** — merged after nav items when a
    sidebar exists, or as the sole content of a synthesized
    `nav#quarto-sidebar … floating` when no sidebar is configured. Body gets
    `floating`/`docked`, and the `body.floating` grid (NOT `toc-left`)
    provides the left column. This is the repro's shape.
- **`body`**: no target emitted → the TOC stays where Pandoc put it in
  `main`, and gets *none* of the scroll-spy decorations (plain list, no
  `.toc-active`).
- **`left-body` / `right-body`**: sidebar TOC plus a plain clone
  (`id="TOC-body"`, `.toc-actions` stripped) left in the body.
- **Banner**: `format-html-title.ts:169-175` sets
  `banner-header-class: toc-left` when `toc-location === "left"` (exactly —
  `left-body` misses it, a latent Q1 inconsistency); the class makes the
  relocated banner header use the `toc-left` grid so its `column-body`
  aligns.
- **About pages** force `right`; **manuscripts** default to `left`.

## Proposed phases (draft)

Skeleton only — contents firm up after the design discussion.

- **Phase 0 — Test plan (TDD).** Failing tests through
  `render_document_to_file` / project render: (a) website repro shape —
  `toc-location: left` in a website project yields TOC inside a left
  sidebar container and NOT in `#quarto-margin-sidebar`; (b) standalone doc
  with `toc-location: left`; (c) `toc-location: body`; (d) default/`right`
  unchanged (snapshot-neutral); (e) banner + `toc-location: left` →
  `toc-left` on `#title-block-header`; (f) invalid value diagnostics (if we
  validate).
- **Phase 1 — Option plumbing.** Read `toc-location` from merged metadata
  (`as_plain_text`, per the metadata-as-str lint), normalize, publish for
  template + downstream consumers (e.g.
  `rendered.navigation.toc-location`).
- **Phase 2 — Placement.** Template/transform changes for `left`
  (standalone and website shapes), `body`, keeping `right` as-is;
  body-class / `#quarto-content`-class plumbing.
- **Phase 3 — Banner producer.** `TitleBannerTransform` (or the title
  pipeline) writes `quarto-template-params.banner-header-class = "toc-left"`;
  un-inert the template.rs:337 hook; update the title_banner.rs module docs.
- **Phase 4 — Preview parity.** `TocSlot`/`chromeSlots.tsx` +
  `PreviewDocument.tsx` honor the published location (WASM rebuild chain for
  verification).
- **Phase 5 — Schema + docs + E2E.** Schema entry (enum), docs page in
  `docs/`, end-to-end verification against the Connect-docs repro, snapshot
  review.

## Open design questions for the user

1. **Value scope.** Q1's enum is `body | left | right | left-body |
   right-body`. Proposal: implement `left`, `right`, `body` now; reject or
   warn-and-fallback on `*-body` (follow-up strand for the clone behavior).
   OK?
2. **One mechanism or two?** Q1 has two `left` layouts: standalone
   (`#quarto-sidebar-toc-left` + `.page-columns.toc-left` grid) and website
   (TOC merged into `nav#quarto-sidebar`, `body.floating` grid). Both SCSS
   regimes are already ported. Do we (a) mirror both shapes (standalone docs
   get the toc-left grid, website pages get the sidebar merge), or (b) pick
   one mechanism everywhere (e.g. always the `#quarto-sidebar-toc-left`
   shape, accepting divergence from Q1 in websites — but then nav-sidebar +
   left-TOC coexistence needs its own answer)? Given the driving use case is
   a website (Connect docs), (a) seems safer; the repro's exact shape is the
   website one.
3. **Where does the website merge happen?** The TOC-inside-`#quarto-sidebar`
   case couples two renderers. Options: (a) `SidebarRenderTransform` learns
   about the rendered TOC (ordering: it must then run after `TocRender`, or
   read `navigation.toc` and render on demand); (b) the template grows a
   sidebar-with-toc branch and `sidebar_to_html` exposes a "don't close the
   nav yet" seam; (c) a small `TocLocationTransform` that runs late,
   relocating rendered fragments in metadata. Preference?
4. **`body` fidelity.** In Q1, a body TOC is a *plain* list (no scroll-spy
   classes, no `.toc-active`). q2's `toc_render` bakes `nav-link` +
   `data-scroll-target` into every entry. Match Q1's plainness (second
   render path), or ship the decorated markup in the body too (simpler; the
   classes are inert without the sidebar JS)?
5. **Empty right margin sidebar.** Q1 keeps an empty
   `#quarto-margin-sidebar` (`zindex-bottom`) when the TOC moves left. q2
   currently omits the element when it has nothing to show. Keep q2's
   omission (cleaner; unknown CSS dependencies?) or emit the empty shell for
   parity?
6. **Banner gate.** Copy Q1 exactly (`toc-left` only when the value is
   exactly `left`) or also cover `left-body` if/when that lands (fixing
   Q1's latent inconsistency)? Trivially decidable later if `*-body` is
   deferred per Q1 above.
7. **Preview scope.** Is preview parity (Phase 4) in-scope for this strand,
   or a follow-up strand? The preview's `TocSlot` shape contract means
   `q2 preview` will keep showing the TOC on the right until it's taught
   otherwise.

## Risks / tradeoffs (draft)

- `FULL_HTML_TEMPLATE`'s `#quarto-content` class list is baked in; `left`
  (standalone) needs a `toc-left` class there, which means a new template
  variable and touching the body-class precedence logic
  (`template.rs:697-733`) — the subtle `fullcontent`-vs-empty fallback has
  bitten before (bd-mgoh); tests must pin all four precedence cases.
- The website-without-sidebar case synthesizes a floating sidebar container
  + `body.floating` — interacts with `SidebarRenderTransform`'s skip
  conditions and could surprise the `nav-sidebar` body-class consumers.
- Snapshot churn: any template reshuffle around the margin sidebar risks
  touching many HTML snapshots; keep `right` byte-stable to keep the diff
  reviewable.
- Preview/template shape drift (risk of a `/preview-parity` class of bug) if
  Phase 4 is deferred — flag it explicitly in the strand if so.

## Investigation artifacts

- `claude-notes/plans/toc-location-investigation/repro/` — minimal website
  repro (mirrors the committed external repro).
- `claude-notes/plans/toc-location-investigation/q1-mechanism-notes.md` —
  full Q1 exploration report (file:line references into
  external-sources/quarto-cli).
