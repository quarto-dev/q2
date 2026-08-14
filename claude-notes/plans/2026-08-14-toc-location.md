# toc-location option (left/right/body); unlocks banner toc-left class (bd-e2kpwy7n)

**Date:** 2026-08-14
**Braid:** bd-e2kpwy7n
**Branch:** investigated on `main` (worktree `.worktrees/bd-nn2fou8h-execute-visibility`, reused after its strand merged)
**Status:** Design aligned with user 2026-08-14 (decisions recorded below). Ready to implement.

User-stated scope note: `external-sources/quarto-cli` is context, not a
contract — what matters is a mechanism that renders TOCs in alternative
locations (`left` first among them), not byte-for-byte Q1 parity.

## Design decisions (user-aligned, 2026-08-14)

1. **Value scope:** implement `left`, `right`, `body` now. `left-body` /
   `right-body` warn-and-fall-back (`left-body` → `left`, `right-body` →
   `right`) with a diagnostic; the clone behavior is a follow-up strand.
2. **Two mechanisms, Q1 parity:** port both `left` layouts — standalone
   (`#quarto-sidebar-toc-left` + `.page-columns.toc-left` grid) and website
   (TOC merged into `nav#quarto-sidebar`, `body.floating` grid). Possible
   unification is a future cleanup, not this strand.
3. **Website merge lives in `SidebarRenderTransform`** (option (a)): the
   rendered sidebar fragment contains the TOC, so custom templates that
   emit `$rendered.navigation.sidebar$` keep working with no new
   variables. Cost: `TocRenderTransform` must run before
   `SidebarRenderTransform` (both Navigation-phase — reorder is legal),
   and SidebarRender grows a synthesize-floating-sidebar branch for
   website + left + no configured sidebar.
4. **`body` ships the decorated markup** (`nav-link`,
   `data-scroll-target`, `.toc-active`) — deviation from Q1's plain list,
   deliberate: scroll-spying is likely coming and the classes are inert
   without the sidebar JS.
5. **No empty right-margin shell:** when the TOC moves left, q2 keeps
   omitting `#quarto-margin-sidebar` entirely (deviation from Q1's empty
   `zindex-bottom` shell — Q1's text-centric infra makes elision hard; we
   can do better).
6. **Banner gate:** `banner-header-class: toc-left` when the (normalized)
   location is exactly `left`. The `left-body` question is recorded on the
   `*-body` follow-up strand (when it lands, the gate should cover
   `left-body` too, fixing Q1's latent inconsistency).
7. **Preview parity is a follow-up strand** — `q2 preview`'s `TocSlot`
   keeps showing the TOC on the right until that strand lands; flagged
   there explicitly.

Follow-up strands filed from this design: see "Follow-up strands" below.

## Triage verdict

**Ready to design.** (Superseded: design questions answered above; now
ready to implement.) The gap is confirmed at HEAD, the Q1 mechanism is
fully mapped, the q2 insertion points are identified, and the SCSS +
template hooks are already ported and inert.

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

See `claude-notes/plans/toc-location-investigation/q1-mechanism-notes.md`
for the full file:line map.

- **Option**: `toc-location` enum `["body", "left", "right", "left-body",
  "right-body"]`, default `right`, HTML-doc formats only
  (`src/resources/schema/document-toc.yml:36-52`).
- **Mechanism**: templates emit a `div#quarto-toc-target` placeholder; a DOM
  postprocessor (`format-html-bootstrap.ts:342-412`) moves `nav[role=doc-toc]`
  into it (adding `.toc-active`, `nav-link`, `data-scroll-target`,
  collapse classes). q2 re-expresses this as template/transform logic
  (no-DOM-postprocessor rule).
- **Two layout regimes for `left`:**
  - *Standalone/article path* (`before-body-article.ejs`): `#quarto-content`
    gets class `toc-left`; a `div#quarto-sidebar-toc-left.sidebar.toc-left`
    holds the TOC; grid comes from `.page-columns.toc-left` mixins gated on
    `body:not(.floating):not(.docked)`.
  - *Website path* (`nav-before-body.ejs` + `sidebar.ejs`): the TOC target
    goes **inside `nav#quarto-sidebar`** — merged after nav items when a
    sidebar exists, or as the sole content of a synthesized
    `nav#quarto-sidebar … floating` when no sidebar is configured. Body gets
    `floating`/`docked`, and the `body.floating` grid (NOT `toc-left`)
    provides the left column. This is the repro's shape.
- **`body`**: no target emitted → the TOC stays where Pandoc put it in
  `main` (Q1 leaves it undecorated; q2 will decorate per decision 4).
- **`left-body` / `right-body`**: sidebar TOC plus a plain clone
  (`id="TOC-body"`, `.toc-actions` stripped) left in the body. Deferred.
- **Banner**: `format-html-title.ts:169-175` sets
  `banner-header-class: toc-left` when `toc-location === "left"`.
- **About pages** force `right`; **manuscripts** default to `left` (both
  out of scope here; noted for the project-types work).

## Phases

### Phase 0 — Test plan (TDD: failing tests first)

End-to-end style per repo policy (route through `render_document_to_file`
/ project render, not `HtmlRenderConfig::default()` shortcuts):

- [ ] **Website + `left`, no configured sidebar** (repro shape): output has
      `nav#quarto-sidebar … floating` containing `nav#TOC`; body classes
      include `floating`; NO `#quarto-margin-sidebar` (decision 5); no TOC
      in the margin region.
- [ ] **Website + `left`, with configured sidebar**: single
      `nav#quarto-sidebar`; nav items first, TOC appended after; no second
      container; margin sidebar omitted.
- [ ] **Standalone doc + `left`**: `#quarto-content` class list gains
      `toc-left`; `div#quarto-sidebar-toc-left.sidebar.toc-left` contains
      `nav#TOC`; body classes do NOT include `floating`/`docked`; margin
      sidebar omitted.
- [ ] **`body`**: `nav#TOC` renders inside `main#quarto-document-content`
      (after the title block, before content), decorated markup; no margin
      sidebar, no left containers.
- [ ] **`right` / unset**: current output byte-stable (snapshot-neutral —
      guard the no-churn goal).
- [ ] **Banner + `left`**: `#title-block-header` class list contains
      `toc-left` (template.rs:337 hook fires).
- [ ] **`left-body` / `right-body`**: warning diagnostic emitted; placement
      falls back to `left` / `right` respectively.
- [ ] **Margin-categories interaction**: categories currently ride the
      margin sidebar (`rendered.navigation.margin_categories`); pin the
      expected behavior when the TOC moves left (categories keep the margin
      sidebar shell — the `$else$` branch at template.rs:229-234 already
      handles TOC-less margin categories).

### Phase 1 — Option plumbing

- [ ] Read `toc-location` from merged metadata with `as_plain_text` (the
      `metadata-as-str` lint exists for exactly this key shape).
- [ ] Normalize: `left` | `right` | `body`; `*-body` → warn + fallback;
      unknown value → diagnostic + default `right`.
- [ ] Publish `rendered.navigation.toc-location` (string) early in the
      Navigation phase so TocRender, SidebarRender, TitleBanner, and the
      template all read one source of truth. (Smallest home: a tiny
      transform or a helper called from `toc_generate`/`toc_render` —
      decide at implementation; must be readable before SidebarRender.)

### Phase 2 — Placement

- [ ] Reorder pipeline: `TocRenderTransform` before
      `SidebarRenderTransform` (both `TransformPhase::Navigation`; verify
      no other ordering dependency between them — sidebar/breadcrumb
      comment at pipeline.rs:1383 mentions ordering vs TocRender, re-check
      why).
- [ ] `SidebarRenderTransform`: when location is `left` and a website
      sidebar exists → append the rendered TOC (plus `h2#toc-title`) inside
      the `nav#quarto-sidebar` fragment (seam in
      `quarto_navigation::sidebar_to_html` or post-append in the
      transform). When location is `left`, website project, no configured
      sidebar → synthesize the Q1 wrapper
      (`nav#quarto-sidebar.sidebar.collapse.collapse-horizontal.quarto-sidebar-collapse-item.sidebar-navigation.floating.overflow-auto`)
      holding only the TOC, and write body-classes `floating` (decide
      whether `nav-sidebar` belongs in that class list — it's q2's own
      addition; check its SCSS consumers).
- [ ] `FULL_HTML_TEMPLATE`: gate the right-margin TOC region on location
      `right`; add the standalone-left `#quarto-sidebar-toc-left` block +
      `toc-left` class on `#quarto-content` (new template variable); add
      the `body` emission point in `main`. Standalone-left must not claim
      the website wrapper (no `floating`).
- [ ] Body-class precedence (`render_with_compiled_template`): confirm the
      four existing cases still hold; standalone-left rides the existing
      "TOC present → empty class" case (grid comes from `#quarto-content`'s
      `toc-left`, not body).
- [ ] Update the stale comments: template.rs:296-298 ("which Q2 doesn't
      support yet") and title_banner.rs:41-43.

### Phase 3 — Banner producer

- [ ] `TitleBannerTransform` (or the title pipeline stage that owns
      `quarto-template-params`) writes
      `quarto-template-params.banner-header-class = "toc-left"` when banner
      mode is active and normalized location is `left`.
- [ ] Module docs updated (title_banner.rs deviation note becomes a
      pointer to this plan).

### Phase 4 — Schema, docs, E2E

- [ ] Schema entry for `toc-location` (enum with all five Q1 values so
      `*-body` validates, even though placement falls back).
- [ ] Docs page in `docs/` (user-facing usage, not internals; rendered
      with `cargo run --bin q2 -- render docs/`).
- [ ] End-to-end verification: render the local repro AND the Connect-docs
      `api/index.html` page; inspect output (record invocation + snippet
      here per the end-to-end policy).
- [ ] Snapshot review: report counts + summary per the snapshot policy;
      `right`-path snapshots expected unchanged.

## Follow-up strands

- **`*-body` clone behavior** (`left-body`/`right-body`): body clone
  `id="TOC-body"` with actions stripped, plus lifting the warn-fallback.
  Must record the banner-gate constraint: when `left-body` lands, the
  banner `toc-left` class should cover it too (fixing Q1's latent
  `=== "left"` inconsistency). Filed as bd-jclcm0in.
- **Preview parity**: teach `TocSlot`/`chromeSlots.tsx` +
  `PreviewDocument.tsx` the published `rendered.navigation.toc-location`;
  until then `q2 preview` shows the TOC on the right for left/body docs.
  Filed as bd-tqijrhsu.

## Risks / tradeoffs

- `FULL_HTML_TEMPLATE`'s `#quarto-content` class list is baked in; the new
  `toc-left` template variable touches the body-class precedence logic
  (`template.rs:697-733`) — the subtle `fullcontent`-vs-empty fallback has
  bitten before (bd-mgoh); Phase 0 pins all precedence cases.
- The synthesized floating sidebar interacts with
  `SidebarRenderTransform`'s skip conditions and the `nav-sidebar`
  body-class consumers — audit before choosing the class list.
- Snapshot churn: keep `right` byte-stable so the diff stays reviewable.
- Preview/template shape drift until the preview follow-up lands — the
  strand records this explicitly.

## Investigation artifacts

- `claude-notes/plans/toc-location-investigation/repro/` — minimal website
  repro (mirrors the committed external repro).
- `claude-notes/plans/toc-location-investigation/q1-mechanism-notes.md` —
  full Q1 exploration report (file:line references into
  external-sources/quarto-cli).
