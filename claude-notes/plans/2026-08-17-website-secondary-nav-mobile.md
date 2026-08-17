# Website mobile secondary-nav bar (bd-26bf3j1y)

**Date:** 2026-08-17
**Braid:** bd-26bf3j1y
**Checkout:** `/Users/cscheid/rooms/room-3/q2`, branch
`braid/bd-26bf3j1y-website-mobile-secondary-nav` off `main` @ `7de02ea2`
**Status:** In progress. Design settled 2026-08-17 (all seven questions answered
— see **Resolved decisions**); implementation started with Carlos's go-ahead the
same day. Work items below are the live tracker.

## Triage verdict

**Ready to design, but the scope in the strand is wrong in one important way and
overlaps an existing strand in another.** The strand describes this as "the
secondary-nav container plus mobile breadcrumbs plus search button." The
investigation says it is really *the mobile navigation subsystem* — the container
is only the visible tip; the load-bearing parts are (a) the sidebar-side collapse
plumbing that **bd-yxlh already owns**, (b) a `#quarto-header` wrapper q2 does not
have, (c) a set of title-block visibility rules Q1 applies in a DOM postprocessor
that q2 must emit declaratively, and (d) a real gap: **the hub-client preview
ships no Bootstrap JS**, so a toggle rendered there is inert by construction —
the exact "toggle wired to nothing" objection that deferred this work in the
first place, now relocated from render to preview.

None of that blocked design; all of it changed what the phases are. The six
questions were answered on 2026-08-17 and are recorded below.

## Issue context

`bd-26bf3j1y` — feature, p2, label `websites`, filed 2026-08-14 by Carlos, still
`open` and untouched since. Filed *at close* of `bd-breadcrumbs-missing-1vpuqh34`
as the deliberate carve-out from that strand: breadcrumbs shipped their
title-block instance (`quarto-title-breadcrumbs d-none d-lg-block`, commit
`66bd2284`), and the secondary-nav container was left out because "a toggle wired
to markup that doesn't exist would be broken UI."

Strand's stated scope:
- container markup (toggle button, flex-grow link, search button, aria wiring),
- `.quarto-sidebar-collapse-item` on the sidebar + Bootstrap collapse plumbing,
- the mobile breadcrumb instance (no extra classes; rendered even for length-1
  trails; `bread-crumbs: false` fallback to a collapsed `h1.quarto-secondary-nav-title`),
- SCSS: Q1 `quarto-nav.scss` ~411–450 and ~470–520.

## Dependency graph

`braid dep tree` shows a single node — the only recorded edge is one outgoing
`discovered-from`. The *real* graph is wider and is not encoded; that is itself a
finding.

- **discovered-from → `bd-breadcrumbs-missing-1vpuqh34`** (closed 2026-08-14).
  Gives the reference markup, the Q1 trail rules, and the explicit scope
  rationale. Its close reason names this strand. Highest-value single piece of
  context, exactly as the skill predicts.

Edges that exist in substance but not in the skein:

- **`bd-yxlh` — "Full Q1 sidebar rollup parity (Decision B from bd-f5yi
  breakpoint plan)"** (open, p3, child of epic `bd-0tr6`). This is *the same
  work seen from the sidebar side*: its own description lists "markup emission:
  `nav.quarto-secondary-nav` strip with the hamburger toggle" as requirement 2,
  and the `media-breakpoint-down(lg)` rollup SCSS as requirement 3. It also
  records the drift hazard: when it lands, the Decision-A `display: none` rule
  must be *replaced*, not stacked. Two strands cannot both own that rule.
- **`bd-e7b7` — "Q2 website JS library loading"** (open, p2). `bd-yxlh` is
  formally `blocks`-ed on it. Partly overtaken by events: native renders now
  ship `site_libs/quarto/bootstrap.bundle.min.js` via `BootstrapJsStage` with an
  integration test. The hub-client half is *not* done and is deliberately so
  (see below).
- **`bd-xva3f8uy` — "quarto-banner class on quarto-header when secondary-nav
  lands"** (open, p4). Explicitly parked *waiting on this strand*: Q1's
  `.quarto-banner nav.quarto-secondary-nav` rule is the only consumer of
  `#quarto-header.quarto-banner`, and q2 has neither. Should become a `blocks`
  edge or be folded in.
- **`bd-6cme` — "[websites] Sidebar search integration"** (open, p2). q2 has no
  search: `navbar_to_html` emits a bare `<div class="quarto-search"></div>`
  placeholder and there is no `window.quartoOpenSearch`. The strand's third
  bullet (search button) is a shell until this lands.
- **`bd-49ar` — "[websites] Sidebar collapse/expand JS"** (open, p2) — sibling
  concern for in-sidebar section collapse, same Bootstrap-JS dependency.

## What the code looks like today

Spot-checked at `cf3318c6`. Everything the strand describes is still accurate;
the surrounding facts it does not mention are the interesting ones.

**Present and reusable:**
- `breadcrumb_trail` + `Crumb` in `crates/quarto-navigation/src/sidebar.rs:761+`;
  `breadcrumbs_to_html` in `render_html.rs`, already class-parameterized "for the
  future mobile instance."
- `BreadcrumbsRenderTransform` (`crates/quarto-core/src/transforms/breadcrumbs_render.rs`),
  Navigation phase, resolving crumb hrefs through `resolve_href_for_html`. Its
  module doc names this strand as the owner of the mobile instance.
- `TITLE_BLOCK_PARTIAL` (`crates/quarto-core/src/template.rs:398`) already has
  `$if(rendered.navigation.breadcrumbs)$` slots in the banner and default
  branches.
- Bootstrap JS **on native renders**: `crates/quarto-core/src/stage/stages/bootstrap_js.rs`
  registers `js:bootstrap`; `artifact_flush.rs:273` writes
  `quarto/bootstrap.bundle.min.js`; `tests/integration/bootstrap_js_pipeline.rs`
  pins one `<script>` per page.

**Absent — this is the actual work:**
- No `#quarto-header` wrapper. `template.rs:274` emits `$rendered.navigation.navbar$`
  bare; `title_banner.rs:39` documents the absence. Q1 nests navbar + secondary
  nav inside `<header id="quarto-header" class="headroom fixed-top">`, and the
  Q1 SCSS this strand wants to port (`#quarto-header > nav` padding, the
  `.quarto-banner nav.quarto-secondary-nav` background) selects through it.
- `sidebar_to_html` (`render_html.rs:293`) emits
  `<nav id="quarto-sidebar" class="sidebar sidebar-navigation {style}" role="doc-toc">`
  — no `collapse collapse-horizontal quarto-sidebar-collapse-item overflow-auto`,
  no `#quarto-sidebar-glass` sibling.
- q2 ships **Decision A** instead of the rollup: `_bootstrap-rules.scss:181-183`
  hides `body.floating .sidebar.sidebar-navigation` outright below `lg`, with a
  comment pointing at `claude-notes/plans/2026-05-01-website-sidebar-breakpoints.md`.
  `docked` and `toc-left` were deliberately left untouched there.
- Vestigial Q1 copies already in tree: `_bootstrap-rules.scss:635-642`
  (`#quarto-sidebar.collapse` z-index) and `2129+` (`.quarto-sidebar-toggle*`)
  select classes q2 never emits.
- No headroom.js, no `window.quartoToggleHeadroom`, no `window.quartoOpenSearch`.
  Q1's inline handlers are guarded (`if (window.quartoToggleHeadroom)`), so
  omitting headroom is safe; `quartoOpenSearch` is called unguarded.

**Three things the strand description does not capture:**

1. **Adding `collapse` to the sidebar hides it on desktop unless the lg+
   overrides come too.** Bootstrap's `.collapse:not(.show) { display: none }` is
   beaten in Q1 only by `quarto-nav.scss:640-656` —
   `@include media-breakpoint-up(lg) { #quarto-sidebar { display: flex; … }
   .sidebar.sidebar-navigation { display: block; position: sticky; } }` (id
   specificity, plus source order). Port those in the same commit or every
   website page loses its sidebar at every width.

2. **Q1 does two title-block mutations in its DOM postprocessor that q2 must
   emit declaratively.** `website-navigation.ts:468-502`: whenever the secondary
   nav exists, `header > .quarto-title-block` gains `d-none d-lg-block`; and in
   the `bread-crumbs: false` branch the empty `h1.quarto-secondary-nav-title` is
   filled from `h1.title`, which itself gains `d-none d-lg-block`. Per
   `CLAUDE.md` ("No DOM postprocessor"), both become template/transform work —
   and the first is a visible change to *every* sidebar-bearing page at narrow
   widths, not just an addition.

3. **The preview has no Bootstrap JS, by design.** `bootstrap_js.rs` is gated
   `#[cfg(not(target_arch = "wasm32"))]` with the reasoning spelled out: the
   hub-client reinitializes its iframe every render tick, so Bootstrap component
   state would be blown away. So in `q2 preview` the toggle would render and do
   nothing — and the preview pane is *most often* at half-width (~850 px), i.e.
   precisely the band this feature targets (`2026-05-01-website-sidebar-breakpoints.md`,
   "hub-client perspective"). Native `q2 render` output is fine.

No repro fixture was captured: this is missing-feature work, not a bug, and the
"symptom" is simply the absence of the markup, which the greps above establish.

## Resolved decisions (2026-08-17)

All six investigation questions were answered by Carlos. The questions as posed
are kept verbatim further down as the record of what was actually decided
against.

1. **Merge with `bd-yxlh` — option (a).** `bd-yxlh` is closed as superseded;
   this strand owns the whole mobile-navigation subsystem, sidebar rollup SCSS
   included. Rationale accepted: the two halves cannot be tested independently.
   Carlos's added context: `bd-26bf3j1y` was filed by an agent auditing Posit
   Connect's docs for missing features, so its scope was framed from the
   symptom side; the subsystem view is the right one.

2. **`#quarto-header` — option (b), static wrapper.** Emit
   `<header id="quarto-header">` with **no** `headroom` and **no** `fixed-top`.
   Headroom (scroll-away header) becomes its own follow-up strand.
   *Consequence not raised at question time:* Q1 also sets `body.nav-fixed`
   whenever `#quarto-header.fixed-top nav.navbar` exists, and its sole consumer
   is `quarto-nav.scss:822` — `body.nav-fixed { padding-top:
   navbar-default-offset($theme-name) }`, pure compensation for the fixed
   header. With a static header q2 needs neither the class nor the padding, so
   omitting both is self-consistent. The headroom follow-up must add
   `fixed-top`, `nav-fixed`, and that padding rule *together*, or the header
   will overlap content.

3. **Preview — skip entirely.** Suppress the secondary nav under
   `target_arch = "wasm32"`, matching `bootstrap_js.rs`'s existing gate, and
   leave Decision A's `display: none` in force there. Carlos's rationale: the
   near-term goal is dogfooding q2 on Posit Connect's docs, and a period without
   mobile nav in preview is acceptable given q2's speed advantage over Q1. No
   new strand — `bd-e7b7` already owns the hub-client JS story.

4. **Search button — omit.** Not emitted until `bd-6cme` lands. Q1's
   `onclick="window.quartoOpenSearch()"` is unguarded and q2 defines no such
   function, so emitting it would ship a button that throws.

5. **Floating-only — confirmed by inspection.** Census of every rendered page in
   `~/repos/github/cscheid/q2-connect-docs/docs-quarto-1/_site` (451 HTML files,
   350 carrying body classes): `floating` × 342, `nav-sidebar` × 341,
   **`docked` × 0**, **`toc-left` × 0**. Neither `_quarto.yml` nor any page's
   front matter in `docs-quarto-1` or `docs-quarto-2` sets `style:` or
   `toc-location:` at all — both sites take the default. Floating-only is
   therefore sufficient for the dogfooding target; `docked` and `toc-left` stay
   recorded deferrals in
   `claude-notes/plans/2026-05-01-website-sidebar-breakpoints.md`.

6. **Title-block hiding — full Q1 parity.** Hide `header > .quarto-title-block`
   below `lg` when the secondary nav is present, and fill the collapsed
   `h1.quarto-secondary-nav-title` from `h1.title` (adding `d-none d-lg-block`
   to that `h1`) in the `bread-crumbs: false` branch. Both emitted
   declaratively — no DOM postprocessor.

**7. `bd-xva3f8uy` — folded in** (Carlos, 2026-08-17). Adding `.quarto-banner` to
`#quarto-header` in banner mode is a single CSS class in the exact template
region Phase 1 introduces, and its only styling consumer is the
`.quarto-banner nav.quarto-secondary-nav` rule Phase 4 ports. It lands in Phase 1
and `bd-xva3f8uy` closes with this work.

## Work items

Branch: `braid/bd-26bf3j1y-website-mobile-secondary-nav`, off `main` @ `7de02ea2`.

Phase ordering is deliberate: Phase 1 (header wrapper) is the DOM change with the
widest snapshot blast radius, so it goes first and alone. Phases 3+4 must land
**in the same commit** — the sidebar `collapse` class without the
`media-breakpoint-up(lg)` overrides breaks every page at every width (see Risks).

### Phase 0 — Test plan (TDD: failing tests first)

- [ ] `quarto-navigation` unit tests for `secondary_nav_to_html`: toggle button
      wiring (`data-bs-toggle="collapse"`, `data-bs-target=".quarto-sidebar-collapse-item"`,
      `aria-controls="quarto-sidebar"`, `aria-expanded="false"`), the
      breadcrumb branch, the `bread-crumbs: false` collapsed-title branch, and
      the absence of any search button.
- [ ] `quarto-navigation` unit tests for `sidebar_to_html`: the four added
      classes and the `#quarto-sidebar-glass` sibling.
- [ ] `quarto-core` template tests: `#quarto-header` wraps navbar + secondary
      nav; `.quarto-banner` present in banner mode and absent otherwise; no
      `headroom` / `fixed-top` / `body.nav-fixed`.
- [ ] `quarto-core` template tests for decision 6: title block carries
      `d-none d-lg-block` when the secondary nav is present and does not when it
      is absent; `h1.title` likewise in the `bread-crumbs: false` branch.
- [ ] New `crates/quarto-core/tests/integration/secondary_nav_pipeline.rs`
      (registered in `main.rs`, alphabetized) driving `render_document_to_file`
      on a real fixture — per `CLAUDE.md`, not `render_qmd_to_html` with
      defaults.
- [ ] A test that pins the `media-breakpoint-up(lg)` sidebar-display overrides.
      **Design this one carefully** — a class-presence assertion cannot catch
      the cliff described in Risks. Candidate: assert on the *compiled* CSS from
      the SCSS pipeline, not on the markup.
- [ ] Verify every new test fails for the right reason before implementing.

### Phase 1 — `#quarto-header` wrapper (widest blast radius; commit alone)

- [ ] Emit `<header id="quarto-header">` in `template.rs` around the navbar and
      the (not-yet-existing) secondary-nav slot. No `headroom`, no `fixed-top`.
- [ ] Add `.quarto-banner` in banner mode (`bd-xva3f8uy`, decision 7).
- [ ] Port the Q1 SCSS that selects through the wrapper: `#quarto-header > nav`
      padding (`quarto-nav.scss:63-66`).
- [ ] Update `title_banner.rs`'s module doc, which currently states q2 has no
      `#quarto-header`.
- [ ] Re-run snapshots; **document counts and diffs per `CLAUDE.md`**.

### Phase 2 — Secondary-nav renderer

- [ ] New emitter in `quarto-navigation` beside `navbar.rs`.
- [ ] Navigation-phase transform in `quarto-core` writing
      `rendered.navigation.secondary-nav`; skip when already set (sibling
      convention).
- [ ] Reuse `breadcrumb_trail` / `breadcrumbs_to_html` with **no extra classes**
      and **no >1-crumb gate** (differs from the title-block instance).
- [ ] `bread-crumbs: false` → collapsed `h1.quarto-secondary-nav-title` from the
      page title.
- [ ] No search button (decision 4).
- [ ] `wasm32` gate (decision 3) **with a comment naming this plan**, so the
      `preview-render-parity` skill's next user finds the rationale.
- [ ] Template slot for `rendered.navigation.secondary-nav` inside
      `#quarto-header`.

### Phase 3 + 4 — Sidebar collapse plumbing and SCSS (ONE commit)

- [ ] `collapse collapse-horizontal quarto-sidebar-collapse-item overflow-auto`
      on `nav#quarto-sidebar`.
- [ ] `#quarto-sidebar-glass` sibling div.
- [ ] `media-breakpoint-up(lg)` display overrides — **the cliff guard**;
      Q1 `quarto-nav.scss:640-656`.
- [ ] `media-breakpoint-down(lg)` rollup — Q1 `quarto-nav.scss:558-590`.
- [ ] **Replace** (not stack on) the Decision-A `display: none` at
      `_bootstrap-rules.scss:181-183`; update its comment, which points at the
      2026-05-01 plan.
- [ ] Port `.quarto-secondary-nav*` rules — Q1 `quarto-nav.scss:411-450`,
      `470-520`, `592-610`.
- [ ] Resolve the open Risks question: does the WASM path share this compiled
      stylesheet? If Phase 2's markup gate suffices, no SCSS gate is needed —
      **confirm, don't assume**.
- [ ] Retire or repurpose the vestigial Q1 copies at
      `_bootstrap-rules.scss:635-642` and `2129+`.
- [ ] Update `claude-notes/plans/2026-05-01-website-sidebar-breakpoints.md`:
      Decision A superseded; `docked`/`toc-left` deferrals still stand.

### Phase 5 — Title-block visibility (decision 6)

- [ ] `header > .quarto-title-block` gains `d-none d-lg-block` when the
      secondary nav is present — all three `TITLE_BLOCK_PARTIAL` branches
      considered (`none` has no title block; check banner).
- [ ] `bread-crumbs: false`: `h1.title` gains `d-none d-lg-block`, its content
      feeding the collapsed secondary-nav title.

### Phase 6 — E2E verification + docs

- [ ] `cargo run --bin q2 -- render <fixture>` with the output **inspected** and
      the invocation + snippet recorded here (per `CLAUDE.md`).
- [ ] Browser check at narrow width: toggle actually opens the sidebar; sidebar
      still present at lg+ (the cliff).
- [ ] Dogfood on `docs/` (`cargo run --bin q2 -- render docs/`).
- [ ] Cross-check against `docs-quarto-1/_site` for Connect parity.
- [ ] `cargo xtask verify` (full, not `--skip-hub-build` — `quarto-core` changes).
- [ ] User-facing docs only if author-visible behavior changes.
- [ ] Close `bd-xva3f8uy` (folded in); confirm `bd-ersobfbt` (headroom) still
      reads correctly against what shipped.

## Design questions as posed (all answered above)

1. **`bd-yxlh` overlap — merge, or split along a seam?** `bd-yxlh` (p3) already
   owns Phases 3–4 verbatim, including the instruction to *replace* the
   Decision-A `display: none`. Options: (a) close `bd-yxlh` as superseded and let
   this strand own the whole subsystem; (b) keep `bd-yxlh` for sidebar+SCSS,
   make this one `blocks`-ed on it, and implement `bd-yxlh` first; (c) treat
   `bd-yxlh` as the epic and this as its child. My recommendation is **(a)** —
   the two halves cannot be tested independently (a toggle without the rollup
   SCSS does nothing visible; the rollup without a toggle is unreachable), and
   splitting guarantees one of them ships broken. But it is your call, and (b)
   is defensible if you want the SCSS reviewed on its own.

2. **`#quarto-header`: full Q1 shape or minimal?** Q1's is
   `<header id="quarto-header" class="headroom fixed-top">` and depends on
   headroom.js for the show-on-scroll-up behavior. We can (a) emit the wrapper
   with `headroom fixed-top` and port headroom.js too, (b) emit the wrapper
   without `headroom`/`fixed-top` (static header — simpler, no new JS, but
   diverges from Q1's scroll behavior and from Q1's CSS assumptions about a
   fixed header), or (c) emit no wrapper and re-target the SCSS selectors at q2's
   flatter DOM. (c) keeps `bd-xva3f8uy` blocked forever. I lean **(b)** —
   structural parity now, headroom as its own strand — but this is the question
   with the most downstream CSS consequence.

3. **What should the preview do?** The toggle is inert under WASM
   (`bootstrap_js.rs` is `cfg(not(wasm32))`, for stated reasons). Options:
   (a) emit the secondary nav in preview anyway and accept a dead button;
   (b) suppress it under WASM and keep Decision A's `display: none` there, so
   preview keeps today's honest-but-sidebar-less narrow view; (c) treat this as
   the forcing function for the hub-client half of `bd-e7b7` and ship Bootstrap
   JS to the preview. (c) is a much bigger job and re-opens the iframe-state
   question. I lean **(b)** for this strand and a follow-up strand for (c) —
   but (a) is arguable if you'd rather have DOM parity between render and
   preview than working buttons (cf. the `preview-render-parity` skill's
   premise, which cuts the other way).

4. **Search button: emit or omit?** q2 has no search (`bd-6cme` open;
   `navbar_to_html` emits a placeholder div; no `window.quartoOpenSearch`).
   Q1's markup calls it unguarded in `onclick`. Emit it now for DOM parity and
   accept a console error on click, emit it disabled/hidden, or omit until
   `bd-6cme` lands? I lean **omit**, matching the reasoning that deferred this
   whole strand from the breadcrumbs one.

5. **Sidebar-style blast radius.** Decision A was floating-only; `docked` and
   `toc-left` were left with the same mid-range defect, unexercised by fixtures.
   Does the rollup cover all three now, or stay floating-only and leave the
   other two as recorded deferrals?

6. **Title-block hiding — parity or restraint?** Q1 hides the whole title block
   below `lg` when a secondary nav exists (the collapsed breadcrumb replaces it).
   That is a visible regression-shaped change for every existing q2 site at
   narrow widths. Full Q1 parity, or emit the secondary nav *without* hiding the
   title block and accept the duplication?

## Risks / tradeoffs (draft)

- ~~**Two strands, one SCSS rule.**~~ Resolved by decision 1: `bd-yxlh` closed as
  superseded, this strand owns `_bootstrap-rules.scss:181-183`.
- **Decision A is still live under WASM.** Decision 3 keeps `display: none` in
  force for the preview while Phase 4 replaces it for native renders. That is
  one SCSS rule that must now be conditional on the build target, which SCSS
  cannot express on its own — the gate has to live wherever the stylesheet is
  assembled. Worth resolving early in Phase 4; if it turns out the two paths
  share one compiled stylesheet, decision 3 may need revisiting (a
  preview-only suppression would then have to be markup-side, i.e. simply not
  emitting the secondary nav and leaving the sidebar hidden as today — which is
  what Phase 2's `wasm32` gate already does, so the SCSS may not need a gate at
  all. Confirm rather than assume.)
- **Sidebar `collapse` is a cliff, not a slope.** Adding the class without the
  `media-breakpoint-up(lg)` overrides removes the sidebar from *every* website
  page at *every* width. It will be obvious in a browser and invisible to any
  test that greps for `id="quarto-sidebar"` — the markup is still there. Worth an
  explicit test asserting on computed visibility or on the presence of the
  override rules, not just the class.
- **`#quarto-header` is load-bearing for other people's work.** `bd-xva3f8uy`
  and the Q1 SCSS both select through it; introducing it changes the DOM every
  website page emits, so navbar/banner snapshots will churn. Expect a
  snapshot-heavy diff and document the counts per `CLAUDE.md`.
- **Preview/render divergence is now deliberate.** Decision 3 makes `q2 preview`
  and `q2 render` differ in DOM at narrow widths, on purpose and indefinitely.
  The `preview-render-parity` skill exists to treat exactly that as a bug, so
  the divergence must be recorded where that skill's next user will find it —
  a comment at the `wasm32` gate naming this plan, at minimum.
- **No DOM postprocessor.** Two Q1 behaviors here are postprocessor mutations;
  re-expressing them declaratively is the kind of thing that invites a "just add
  a small postprocess step" shortcut. `CLAUDE.md` forbids it; if the declarative
  route turns out to be genuinely blocked, that is a stop-and-ask, not a
  workaround.
