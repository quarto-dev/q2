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

## Findings during implementation

### F1 — Q1's title-block hiding is dead code; "parity" means *don't* hide (2026-08-17)

Decision 6 said "full Q1 parity: hide `header > .quarto-title-block` below `lg`
when the secondary nav is present." Checking Q1's *rendered output* rather than
its source shows Q1 never actually does this.

`website-navigation.ts:497-501` runs
`doc.querySelector("header > .quarto-title-block")` — a `.quarto-title-block`
whose **parent** is a `<header>`. But every Q1 emitter puts that class on the
`<header>` element itself:

- `formats/html/templates/title-block.html:1` — `<header id="title-block-header" class="quarto-title-block default">`
- `formats/html/templates/banner/title-block.html:2` — same shape
- `formats/html/templates/manuscript/title-block.html:1` — same shape

So the selector can only match a `<header>` nested inside another `<header>`,
which the HTML format never produces. Empirically confirmed across the Connect
site: of 350 pages carrying a `.quarto-title-block`, **0** have `d-none` on it,
and the class string is `quarto-title-block default` on 349 of them — including
pages that *do* render the secondary nav with breadcrumbs.

**Consequence:** the first half of decision 6 is a no-op. Q1's real mobile
appearance shows the secondary-nav breadcrumbs *and* the full title block. Since
Carlos asked for parity with Q1, Phase 5 implements nothing for it.

The second half of decision 6 is **live** and still applies: in the
`bread-crumbs: false` branch the collapsed `h1.quarto-secondary-nav-title` is
filled from `h1.title`, and that `h1.title` gains `d-none d-lg-block`
(`website-navigation.ts:483-493`). That code sits inside the
`if (secondaryNavTitleEl)` guard, and the `showBreadCrumbs: false` template
branch does emit the `h1`, so it runs.

### F3 — `role="doc-toc"` on the sidebar silently disabled the drawer (2026-08-17)

**The bug the whole phase-0 test suite could not see, found in a headless
browser on the first try.**

`_bootstrap-rules.scss` hid the TOC on narrow screens with
`@include media-breakpoint-down(md) { nav[role="doc-toc"] { display: none } }`.
q2 puts `role="doc-toc"` on **two** elements: the real TOC (`nav#TOC`) and —
as a divergence from Q1, already tracked in `bd-eczdzfqo` — the navigation
sidebar. So that rule hid the sidebar too.

That overlap was harmless while Decision A hid the sidebar below `lg` anyway.
It became a real bug the moment the sidebar turned into a collapse drawer:
below 768px the toggle latched `.show` on `nav#quarto-sidebar` and the glass
pane dimmed correctly, but the drawer stayed at `display: none` and zero
width. **Nothing appeared.** `.show` only defeats Bootstrap's own
`.collapse:not(.show)` rule; it does not override an unrelated `display: none`
from somewhere else in the cascade.

Why no test caught it: every markup assertion passed (the element, its
classes, the toggle's target — all correct), and the SCSS cliff test asked
about `min-width: 992px`, where the sidebar is fine. The failure lived at
`max-width: 767px`, in a rule that predates this work and mentions neither
`collapse` nor `#quarto-sidebar`.

Fix: scope the rule to `nav#TOC[role="doc-toc"]`, which is what it always
meant. Guarded by
`quarto-sass::compile::tests::test_narrow_viewport_hiding_does_not_catch_the_sidebar`,
which fails on any `max-width` rule that hides by `doc-toc` without naming
`#TOC`. The `#TOC` qualifier stays correct whichever way `bd-eczdzfqo` goes.

### F2 — Q1's duplicate `role` attribute does not survive to output (2026-08-17)

`nav-before-body.ejs:74` and `:79` set `role="navigation"` **and** `role="link"`
on the same `<a>`. Q1's own DOM postprocessor drops the second — the rendered
Connect pages carry only `role="navigation"`. Emitting just `role="navigation"`
is therefore byte-parity with Q1's output, not a deviation from it.

## Work items

Branch: `braid/bd-26bf3j1y-website-mobile-secondary-nav`, off `main` @ `7de02ea2`.

Phase ordering is deliberate: Phase 1 (header wrapper) is the DOM change with the
widest snapshot blast radius, so it goes first and alone. Phases 3+4 must land
**in the same commit** — the sidebar `collapse` class without the
`media-breakpoint-up(lg)` overrides breaks every page at every width (see Risks).

### Phase 0 — Test plan (TDD: failing tests first) — **DONE** (`4b1ee305`)

22 tests added; 15 fail for the right reason, 7 are absence-pins that pass now
and become load-bearing once the markup exists.

- [x] `quarto-navigation` unit tests for `secondary_nav_to_html` (9 in
      `render_html.rs::secondary_nav_tests`): toggle wiring, aria-label
      escaping, mobile-instance classes, collapsed-title branch, markdown
      title, no search button, no headroom hooks, sidebar collapse classes,
      glass pane. 7 fail / 2 absence-pins pass.
- [x] `quarto-core` template tests (5 in `template.rs::tests`): `#quarto-header`
      wraps navbar + secondary nav, static (no `headroom`/`fixed-top`/
      `nav-fixed`), absent when there's nothing to hold, `.quarto-banner` in
      banner mode, and the F1 title-block pin. 2 fail / 3 absence-pins pass.
- [x] `crates/quarto-core/tests/integration/secondary_nav_pipeline.rs` (7 tests,
      registered in `main.rs`), driving the real `ProjectPipeline` on temp-dir
      website fixtures per `CLAUDE.md`'s end-to-end rule. 5 fail / 2 pass.
- [x] The SCSS cliff test —
      `quarto-sass::compile::tests::test_sidebar_stays_visible_at_lg_despite_collapse_class`.
      Compiles the real default CSS, brace-matches every `min-width:992px`
      media block, and asserts some `#quarto-sidebar` rule in one of them sets
      `display` to non-`none`. It also asserts Bootstrap's
      `.collapse:not(.show)` is in the bundle, so the test fails loudly if the
      thing it guards against ever stops being a threat. A markup assertion
      cannot catch this: the element and all its classes are still emitted.
- [x] Verified every new test fails for the right reason (stub
      `secondary_nav_to_html` returns `String::new()` so assertions fail rather
      than the build).
- [x] `cargo clippy --all-targets` clean on the three touched crates.

### Phase 1 — `#quarto-header` wrapper — **DONE**

- [x] Emit `<header id="quarto-header">` in `template.rs` around the navbar and
      the (not-yet-existing) secondary-nav slot. No `headroom`, no `fixed-top`.
- [x] Add `.quarto-banner` in banner mode (`bd-xva3f8uy`, decision 7).
- [x] Port the Q1 SCSS that selects through the wrapper: `#quarto-header > nav`
      padding (`quarto-nav.scss:63-66`) — ported as the paired rule Q1 writes,
      alongside `footer.footer .nav-footer`.
- [x] Update `title_banner.rs`'s module doc, which currently states q2 has no
      `#quarto-header`.
- [x] Re-run snapshots; **documented per `CLAUDE.md`** — see below.

**Snapshot / baseline churn (Phase 1): 0 `.snap` files changed, 1 baseline hash
updated.** The risk section predicted "a snapshot-heavy diff"; it did not
materialize, for a reason worth recording. The header partial is invoked from
inside an `$if$`/`$elseif$` gate at the call site rather than as a bare
`$quarto-header()$` line. A bare call emits the template line's trailing
newline even when the partial expands to nothing, which shifted every rendered
document by one blank line — caught by
`attribution_baseline_snapshot::attribution_off_html_baseline`. Moving the gate
to the call site makes the no-header case emit nothing at all, so non-website
renders are byte-identical.

The one update is `tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`:
`doc_files/styles.css` only, because of the new `#quarto-header > nav` padding
rule. **`doc.html` is unchanged**, which is the useful signal — it confirms the
template restructure is byte-neutral for every render that has no navbar and no
secondary nav.

**Design note — why the gate is split from the markup.** The template language
has no boolean `or`, so "navbar OR secondary-nav" has to be an
`$if$`/`$elseif$` pair somewhere. Putting the pair at the call site (two
identical `$quarto-header()$` lines) keeps the `<header>` markup single-sourced
in the partial; putting it inside the partial would have duplicated the opening
tag and its banner-class conditional instead. It also gives users the Q1-style
`quarto-header.html` override seam for free.

### Phase 2 — Secondary-nav renderer — **DONE** (`cdbf8478`)

- [x] New emitter in `quarto-navigation` beside `navbar.rs`.
- [x] Navigation-phase transform in `quarto-core` writing
      `rendered.navigation.secondary-nav`; skip when already set (sibling
      convention).
- [x] Reuse `breadcrumb_trail` / `breadcrumbs_to_html` with **no extra classes**
      and **no >1-crumb gate** (differs from the title-block instance).
- [x] `bread-crumbs: false` → collapsed `h1.quarto-secondary-nav-title` from the
      page title.
- [x] No search button (decision 4).
- [x] `wasm32` gate (decision 3) **with a comment naming this plan**, so the
      `preview-render-parity` skill's next user finds the rationale.
- [x] Template slot for `rendered.navigation.secondary-nav` inside
      `#quarto-header`.

### Phase 3 + 4 — Sidebar collapse plumbing and SCSS (ONE commit) — **DONE** (`d45253c3`)

- [x] `collapse collapse-horizontal quarto-sidebar-collapse-item overflow-auto`
      on `nav#quarto-sidebar`.
- [x] `#quarto-sidebar-glass` sibling div.
- [x] `media-breakpoint-up(lg)` display overrides — **the cliff guard**;
      Q1 `quarto-nav.scss:640-656`.
- [x] `media-breakpoint-down(lg)` rollup — Q1 `quarto-nav.scss:558-590`.
- [x] **Replaced** (not stacked on) the Decision-A `display: none`; its comment
      now records the supersession and the floating-only rationale.
- [x] Port `.quarto-secondary-nav*` rules — Q1 `quarto-nav.scss:411-450`,
      `470-520`, `592-610`.
- [x] Resolved the open Risks question: **no SCSS gate is needed.** Phase 2's
      `wasm32` gate means the preview emits no secondary nav, and with no bar
      the drawer is simply never opened — the sidebar behaves as it did under
      Decision A without any build-target-conditional CSS. One compiled
      stylesheet serves both, as suspected. Confirmed, not assumed.
- [x] Vestigial Q1 copies triaged. `#quarto-sidebar.collapse` (z-index) is
      **no longer vestigial** — the sidebar now carries `.collapse`, so it does
      its Q1 job; commented as live. `.quarto-sidebar-toggle*` **is still
      unreferenced**: it styles Q1's separate rollup accordion, not this
      drawer. Kept with a note rather than deleted, since a future rollup
      feature would want it verbatim.
- [x] Update `claude-notes/plans/2026-05-01-website-sidebar-breakpoints.md`:
      Decision A superseded; `docked`/`toc-left` deferrals still stand.

**Bug found after this commit, in the browser:** see finding **F3** —
`nav[role="doc-toc"] { display: none }` below `md` was also hiding the
sidebar, so the drawer never opened below 768px. Fixed by scoping to `#TOC`,
with a new regression test.

### Phase 5 — Title-block visibility (decision 6, amended by F1) — **DONE** (`cdbf8478`)

Shipped inside the phase-2 commit: the collapsed title and the `h1.title`
hiding are one behavior, and splitting them would have left a commit where the
bar shows the title and the document shows it again right below.

- [x] ~~`header > .quarto-title-block` gains `d-none d-lg-block`~~ — **dropped.**
      F1 shows Q1's selector never matches, so parity means emitting nothing.
      A regression pin lives in Phase 0 instead.
- [x] `bread-crumbs: false`: `h1.title` gains `d-none d-lg-block`, its content
      feeding the collapsed secondary-nav title. Implemented via a
      `rendered.navigation.secondary-nav-collapsed-title` flag set by the
      transform and consumed by all three `TITLE_BLOCK_PARTIAL` branches
      (Q1's postprocessor targets the first `h1.title` regardless of branch).

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
