# Website sidebar responsive breakpoints

## Status

Investigation + design space. **No implementation in this plan.**
Aimed at producing a shared mental model so we can pick a fix
approach with eyes open. Defer implementation to the user's
go-ahead.

## Why

Discovered while investigating bd-f5yi. Once the
`nav[role="doc-toc"]` injection bug is fixed and the website
sidebar appears in the hub-client preview, the *visual* state at
common viewport widths is broken: at viewports between 768 and
991 px, the sidebar collapses to an ~26 px-wide vertical bar with
a scrollbar — present in the DOM, technically interactive, but
unusable. The user verified the same defect on the **native
renderer** (`localhost:8000/_site` of
`examples/websites/08-hub-preview/`), so this is no longer a
hub-client iframe bug — it's a deficiency in the SCSS we emit.

## Empirical breakpoint map

Native render of `08-hub-preview/index.qmd`, body class
`nav-sidebar floating`, sidebar class
`sidebar sidebar-navigation sidebar-floating`. Captured via
Chrome DevTools viewport emulation; screenshots saved in
`claude-notes/2026-05-01-sidebar-breakpoints/native-<width>.png`.

| Viewport (px) | Sidebar rect | Sidebar `display` | Body main width | Verdict |
|---------------|--------------|-------------------|-----------------|---------|
| 1600 | 250 × 768 (x=150) | block | 749 | ✅ usable |
| 1400 | 250 × 768 (x=50) | block | 749 | ✅ usable |
| 1200 | 249 × 768 (x=26) | block | 599 | ✅ usable |
| 1100 | 216 × 768 | block | 566 | ✅ usable |
| 1000 | 183 × 768 | block | 533 | ✅ usable (border) |
|  992 | 180 × 768 | block | 530 | ✅ last good width |
|  991 |  26 × 768 | block | 664 | ❌ **broken** (cliff at `lg`) |
|  900 |  26 × 768 | block | 574 | ❌ broken |
|  800 |  26 × 768 | block | 525 | ❌ broken |
|  768 |  26 × 768 | block | 508 | ❌ broken |
|  767 |   0 ×   0 | **none** | full | ✅ acceptable (sidebar collapsed) |
|  600 |   0 ×   0 | none | full | ✅ acceptable |

The cliff is exactly at the Bootstrap `lg` breakpoint (992 px).
Below `lg` and above `md` (768 ≤ vp < 992) the layout is broken;
below `md` the sidebar disappears outright.

## Why the mid range collapses

In `crates`-adjacent `resources/scss/bootstrap/_bootstrap-rules.scss`
the relevant rule chain is:

```scss
// resources/scss/bootstrap/_bootstrap-rules.scss:80
body.floating {
  .page-columns { @include page-columns-float-wide(); }   // wide ≥ lg
}

// resources/scss/bootstrap/_bootstrap-rules.scss:163
@include media-breakpoint-down(lg) {
  body.floating {
    .page-columns { @include page-columns-float-mid(); }   // mid <  lg
  }
}

// resources/scss/bootstrap/_bootstrap-rules.scss:212-242
@include media-breakpoint-down(md) {
  body, body.floating, ... {
    .page-columns {
      @include page-columns();
      @include grid-template-columns-narrow();             // narrow < md
    }
  }
  nav[role="doc-toc"] { display: none; }                   // hides TOC + sidebar
}
```

The mid mixin at `_bootstrap-mixins.scss:986-1000`:

```scss
@mixin page-columns-float-mid {
  @include page-columns();
  grid-template-columns:
    [screen-start]                   $grid-float-mid-page-gutter-start
    [screen-start-inset]              $grid-float-mid-sidebar-gutter
    [page-start page-start-inset
     body-start-outset body-start]    $grid-float-mid-body-gutter-start
    [body-content-start]              $grid-float-mid-body
    [body-content-end]                $grid-float-mid-body-gutter-end
    [body-end]                        $grid-float-mid-margin-seg3
    [body-end-outset]                 $grid-float-mid-margin-seg2
    [page-end-inset]                  $grid-float-mid-margin-seg1
    [page-end]                        $grid-float-mid-margin-gutter
    [screen-end-inset]                $grid-float-mid-page-gutter-end
    [screen-end];
}
```

The four named lines `[page-start page-start-inset
body-start-outset body-start]` are **collapsed into a single grid
line**. The sidebar in `_bootstrap-rules.scss:298-303` is placed at
`grid-column: page-start / body-start`, which now spans **zero
tracks**. The 26 px we see is the sidebar's intrinsic minimum
width (vertical scrollbar + paddings), squeezed into a zero-width
column.

The author's intent is recorded inline at
`_bootstrap-mixins.scss:225`:

```scss
// No sidebar, only margins
$grid-float-mid-margin-width: $grid-float-margin-width !default;
```

So the mid grid was designed assuming **the floating sidebar is
hidden in this range**. But nothing in `_bootstrap-rules.scss`
actually hides it. (The `nav[role="doc-toc"] { display: none; }`
at line 239 lives under `media-breakpoint-down(md)` — too narrow.)
That's the gap.

## How Q1 handled this

`external-sources/quarto-cli/.../navigation/quarto-nav.scss:558-590`
has the missing piece:

```scss
@include media-breakpoint-down(lg) {
  .sidebar-navigation .sidebar-item a,
  .nav-page .nav-page-text,
  .sidebar-navigation { font-size: $sidebar-font-size-collapse; }

  .sidebar-logo { display: none; }

  .sidebar.sidebar-navigation {
    position: static;
    border-bottom: 1px solid $table-border-color;
  }
  .sidebar.sidebar-navigation.collapsing { position: fixed; z-index: 1000; }
  .sidebar.sidebar-navigation.show       { position: fixed; z-index: 1000; }
  .sidebar.sidebar-navigation { min-height: 100%; }
  ...
}
```

Q1's mid-range behavior:
1. Take the sidebar **out of the grid** (`position: static`).
2. Stack it horizontally below the navbar with a `border-bottom`.
3. Bootstrap-collapse it by default — visible only when a
   hamburger toggle adds the `.show` class.
4. A `nav.quarto-secondary-nav` strip provides the hamburger
   itself.

We don't yet emit the hamburger nor the `quarto-secondary-nav`
container in Q2. That's the missing chunk needed for full Q1
parity at the mid range.

## Configuration matrix

To plan a fix that doesn't regress other layouts, here are the
configurations the SCSS distinguishes today.

### Sidebar style (`body` class)

| Class   | Frontmatter | Sidebar position | Mid-range behavior today |
|---------|-------------|------------------|--------------------------|
| `floating` | default for `style: floating` | sidebar in `[page-start, body-start]` | broken (this bug) |
| `docked` | `style: docked` | sidebar in `[screen-start, body-start]` | likely also broken (untested in repro) |
| (neither — `nav-sidebar` only) | rare | n/a | n/a |

### TOC style (`page-columns.toc-left`)

| Class | Frontmatter | TOC position | Mid behavior |
|-------|-------------|--------------|--------------|
| (default) `.toc-right` | default | margin column | n/a — TOC handled via margin segments |
| `.toc-left` | `toc-location: left` | left of body | uses `page-columns-tocleft-mid` (separate codepath) |

### Content modes

`fullcontent`, `slimcontent`, `listing` — each has its own mid
mixin under `_bootstrap-mixins.scss`. They share the same defect
(zero-width sidebar tracks at mid). The fix has to apply across
all three plus the default.

### Grid variable defaults

From `_bootstrap-variables.scss:225-232`:

```
$grid-sidebar-width:        250px
$grid-body-width:           800px
$grid-margin-width:         250px
$grid-column-gutter-width:  1.5em (~24px)
```

Sum: ~1300 px is the "comfortable" full layout.
At 992 px, sidebar (250) + body (500 min) + margin (~50–250) +
gutters already overflow what's available — explaining why the
cliff happens at `lg`.

## Design space for a fix

Three reasonable directions; not mutually exclusive but pick one
to start.

### A. Hide the floating sidebar below `lg` (smallest fix)

Add to `_bootstrap-rules.scss`:

```scss
@include media-breakpoint-down(lg) {
  body.floating .sidebar.sidebar-navigation,
  body.docked   .sidebar.sidebar-navigation {
    display: none;
  }
}
```

Aligns the runtime behavior with the existing
`page-columns-float-mid`'s author-stated intent ("No sidebar,
only margins"). Restores the 768–991 range to "no sidebar at
all", matching the <768 behavior. Cost: navigation is unreachable
at those widths until the user resizes.

Pros: minimal surface, no new HTML, no JS.
Cons: poor UX when the user is at 900 px and wants to navigate
to a sibling page. Would also feel inconsistent with the
hub-client preview, which most often opens at a half-width pane
(~850 px) — exactly the broken band.

### B. Port Q1's `position: static` rollup pattern (full parity)

Port the rules from `quarto-nav.scss:558-590`. Requires:

1. SCSS rules under `media-breakpoint-down(lg)` that take the
   sidebar out of the grid and stack it.
2. Markup change: emit a `nav.quarto-secondary-nav` strip
   containing the hamburger toggle, plus `data-bs-toggle` /
   `data-bs-target` wiring on the toggle. Need to verify what the
   sidebar-render transform currently produces and add the toggle
   element if absent.
3. Bootstrap's collapse JS is already loaded; no extra JS code.

Pros: full Q1 parity, navigation reachable at all widths, clean
mobile/tablet UX.
Cons: multi-file change spanning SCSS + sidebar render markup.
Probably needs a beads ticket of its own.

### C. Lower the breakpoint where the *wide* layout takes over

Don't switch to mid mode until vp < 800 (or 768). The wide grid
mixin scales smoothly — at 992 the sidebar is already only 180 px
wide, and the body still has ~530 px. We could keep the wide
mixin active down to ~800.

Implementation: a custom Bootstrap breakpoint or shifting the
`media-breakpoint-down(lg)` boundary just for the website
sidebar. Might be expressible as `@media (max-width: 799px)` on
the mid-mode wrapper.

Pros: smallest visual change at the most common widths
(half-pane previews, tablets).
Cons: at 800–900 the body content is already pinched; gutters
get awkward. We'd be papering over the underlying capacity issue
(too many columns for the available width).

### Recommendation

I'd start with **A** as a one-day fix to stop the bleeding (the
26 px ghost is actively confusing), then plan **B** as the
proper fix in a separate ticket once we decide whether the
hub-client preview should ship the hamburger toggle. **C** is a
band-aid I'd avoid unless A + B turn out to be too invasive.

The hub-client perspective specifically: most editing sessions
are at half-width (around 850 px) with the editor on the left.
**A** at least makes that view honest — sidebar disappears, body
has full width — instead of broken. Once **B** lands, the
sidebar reappears as a collapsible stripe even in that view.

## SUPERSEDED 2026-08-17 — Decision B shipped (bd-26bf3j1y)

**Decision A is gone.** `body.floating .sidebar.sidebar-navigation
{ display: none }` has been *replaced* (not stacked on) by the full
Decision-B rollup, in `bd-26bf3j1y`. Below `lg` the floating sidebar
now leaves the grid (`position: static`) and becomes a Bootstrap
collapse drawer opened by `nav.quarto-secondary-nav`'s toggle. See
`claude-notes/plans/2026-08-17-website-secondary-nav-mobile.md`.

What changed since Resolved Decision 2 said "B is not feasible yet":
the JS-loading prerequisite is met on the native path — `BootstrapJsStage`
ships `site_libs/quarto/bootstrap.bundle.min.js` with every
Bootstrap-themed render. The hub-client half of `bd-e7b7` is still
open, so the secondary nav is **not** rendered under WASM; the preview
keeps the sidebar-less narrow view this plan described. That divergence
is deliberate.

`bd-yxlh`, which tracked Decision B, is closed as superseded — the
markup and the SCSS could not be tested apart.

**Still standing from this plan:** the `docked` and `toc-left` mid-range
defects in "Future deferred work" below. The rollup is floating-only,
which a body-class census of the Posit Connect Q1 site confirmed is
enough for the porting target (342 `floating`, 0 `docked`, 0 `toc-left`).
Decision 4 (print media) also still stands.

## Resolved decisions (2026-05-01)

1. **Ship A now.** Floating-only. B is deferred behind a JS-
   loading prerequisite (see #2 below). C is rejected (band-aid).
2. **B is not feasible yet.** B's hamburger-toggle UX likely
   requires Bootstrap's JS bundle to be loaded. Q2 doesn't yet
   have a story for shipping JS modules with website renders —
   and the hub-client preview adds an additional constraint:
   modules must not be re-parsed/re-executed on every
   incremental HTML re-render. So the dependency chain for B is:
   - Fix A (this plan).
   - Design how Q2 websites load JS libraries, both natively
     and in the hub-client live-preview path (separate plan).
   - Implement Bootstrap JS bundling on top of that.
   - Then design + implement B.
   File a beads ticket for "JS library loading for Q2 websites"
   when this work is unblocked; reference it from the followup
   to bd-f5yi.
3. **Floating-only for now.** Don't touch `docked` or `toc-left`
   in this change — only `body.floating .sidebar.sidebar-navigation`.
   The `docked` and `toc-left` mid defects are real but not
   exercised by any current example fixture. Recorded here so
   future-us can find them quickly:
   - `_bootstrap-mixins.scss:1058+` (`page-columns-docked-mid` and
     friends) collapse the same way.
   - `_bootstrap-mixins.scss` (`page-columns-tocleft-mid`)
     equally collapses the toc-left column.
   - When an example exercises one of these, port the same
     `display: none` gate (or the eventual Q1-rollup parity
     work) to the relevant body-class branch.
4. **Print media**: skip for now. Most print-media CSS is
   already broken; we'll address it as a separate cross-cutting
   pass.

## Work items

- [ ] Implement A: hide `body.floating .sidebar.sidebar-navigation`
      under `media-breakpoint-down(lg)` in
      `resources/scss/bootstrap/_bootstrap-rules.scss`.
- [ ] Native render test: `examples/websites/08-hub-preview/`
      at viewport 800 px renders without the 26px-ghost
      sidebar artifact.
- [ ] Re-capture screenshots at 800 / 900 / 992 / 1200 px under
      `claude-notes/2026-05-01-sidebar-breakpoints/` (post-fix
      filenames) for the empirical map.
- [ ] File a follow-up beads ticket: "Q2 website JS library
      loading (native + hub-client incremental-stable)" — the
      prerequisite for B. Reference bd-f5yi.
- [ ] After (3): file a beads ticket for B (port Q1's
      `position: static` rollup + hamburger markup), blocked on
      the JS-loading work above.

### Future deferred work (recorded so we can find it)

- **Docked sidebar mid defect**: same zero-width collapse in
  `page-columns-docked-mid` (and slimcontent / fullcontent /
  listing variants). Apply the same gate when an example
  exercises a docked sidebar.
- **`toc-left` mid defect**: same zero-width collapse in
  `page-columns-tocleft-mid`. Apply the same gate when an
  example exercises toc-left.
- **B (full Q1 rollup parity)**: see Resolved Decision 2 above.
- **Print media**: see Resolved Decision 4 above.

## References

- Empirical screenshots:
  `claude-notes/2026-05-01-sidebar-breakpoints/native-{767,768,800,900,1000,1100,1200,1400,1600}.png`
- Current SCSS:
  - `resources/scss/bootstrap/_bootstrap-rules.scss:46-242, 298-323`
  - `resources/scss/bootstrap/_bootstrap-mixins.scss:912-1031, 985-1000, 216-242`
  - `resources/scss/bootstrap/_bootstrap-variables.scss:224-232`
- Q1 reference behavior:
  `external-sources/quarto-cli/src/resources/projects/website/navigation/quarto-nav.scss:558-590`
- Triggering bug discovery: bd-f5yi (sidebar missing in
  hub-client preview); see
  `claude-notes/plans/2026-05-01-hub-client-website-render-ux.md`.
