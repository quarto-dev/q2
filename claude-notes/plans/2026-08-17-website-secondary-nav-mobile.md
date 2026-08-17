# Website mobile secondary-nav bar (bd-26bf3j1y)

**Date:** 2026-08-17
**Braid:** bd-26bf3j1y
**Checkout:** `/Users/cscheid/rooms/room-3/q2`, branch `main` @ `cf3318c6` (no worktree created — the skill works in the checkout it was invoked in)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

None of that blocks design; all of it changes what the phases are. Three
questions below need the user's call before phases can be pinned.

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

## Proposed phases (draft)

Skeleton only — contents depend on the answers below, especially Q1 (merge with
`bd-yxlh`) and Q3 (preview behavior).

- **Phase 0 — Test plan.** Failing tests first: `sidebar_to_html` class
  assertions; `secondary_nav_to_html` unit tests (toggle wiring, both
  breadcrumb/no-breadcrumb branches, search-button gating); a
  `secondary_nav_pipeline.rs` integration test driving `render_document_to_file`
  and asserting on real output; a template test for the title-block
  `d-none d-lg-block` gating.
- **Phase 1 — `#quarto-header` wrapper.** Introduce it in `template.rs` around
  navbar + secondary nav; decide `headroom fixed-top` (Q1) vs. static. Absorbs
  `bd-xva3f8uy` (`.quarto-banner`) if we take it.
- **Phase 2 — Secondary-nav renderer.** New emitter in `quarto-navigation`
  (`secondary_nav.rs` or beside `navbar.rs`), plus a Navigation-phase transform
  writing `rendered.navigation.secondary-nav`. Consumes the existing
  `breadcrumb_trail` / `breadcrumbs_to_html` with no extra classes and no >1
  gate; `bread-crumbs: false` fallback emits the collapsed title.
- **Phase 3 — Sidebar collapse plumbing.** `collapse collapse-horizontal
  quarto-sidebar-collapse-item overflow-auto` on `nav#quarto-sidebar`,
  `#quarto-sidebar-glass` sibling. **Overlaps `bd-yxlh`.**
- **Phase 4 — SCSS.** The `media-breakpoint-down(lg)` rollup *and* the
  `media-breakpoint-up(lg)` display overrides; replace (not stack on) the
  Decision-A `display: none`; port `.quarto-secondary-nav*` rules. Decide the
  `docked` / `toc-left` blast radius. **Overlaps `bd-yxlh`.**
- **Phase 5 — Title-block visibility.** The two Q1 postprocessor behaviors as
  template conditionals.
- **Phase 6 — Preview story.** Per Q3: gate, ship-inert, or a preview-specific
  affordance.
- **Phase 7 — E2E + docs.** `q2 render` on a fixture at narrow width with the
  output inspected (per `CLAUDE.md`); dogfood on `docs/`; user-facing docs only
  if behavior changes for authors.

## Open design questions for the user

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

- **Two strands, one SCSS rule.** The Decision-A `display: none` at
  `_bootstrap-rules.scss:181-183` is named as "the thing to replace" by
  `bd-yxlh`. Whoever touches it must close the other strand or the next session
  will re-litigate. This is the single most likely source of drift.
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
- **Preview/render divergence either way.** Whichever way Q3 goes, `q2 preview`
  and `q2 render` will differ at narrow widths — in DOM (option b) or in
  behavior (option a). Worth writing down wherever the `preview-render-parity`
  skill will find it.
- **No DOM postprocessor.** Two Q1 behaviors here are postprocessor mutations;
  re-expressing them declaratively is the kind of thing that invites a "just add
  a small postprocess step" shortcut. `CLAUDE.md` forbids it; if the declarative
  route turns out to be genuinely blocked, that is a stop-and-ask, not a
  workaround.
