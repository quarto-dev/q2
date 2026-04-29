# Website sidebar layout: body class + grid placement

**Date:** 2026-04-29
**Beads:** `bd-mgoh` (this task); discovered-from `bd-2jwk` (website examples).
**Parent epic:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Status:** Draft — awaiting user review before implementation.

## Symptom

Rendering `examples/websites/01-minimal` with `q2 render` and serving the
result locally, the sidebar `<nav id="quarto-sidebar">` appears at the
**bottom of the page**, below `<main>` and below the prev/next strip,
instead of in a left column where Q1 puts it.

Verified by side-by-side DOM inspection of:
- Q2 output served on `127.0.0.1:8080`
- Q1 output (same project) served on `127.0.0.1:8081`

## Root cause

Two related issues in `crates/quarto-core/src/template.rs` and the
website rendering pipeline.

### Issue 1: body class is hardcoded `fullcontent`

`template.rs:162`:

```
<body class="fullcontent$if(body-classes)$ $body-classes$$endif$">
```

The `body-classes` template variable is documented but **nothing in
the website pipeline ever sets it**. So every Q2 page ships with body
class `fullcontent` and nothing else.

`resources/scss/bootstrap/_bootstrap-rules.scss` keys the entire grid
layout off body classes:

| Body class | Mixin selected | Sidebar column? |
|---|---|---|
| `floating` | `page-columns-float-wide()` | yes — 150px left column |
| `docked` | `page-columns-docked-wide()` | yes — 250px left column |
| `fullcontent:not(.floating):not(.docked)` | `page-columns-fullcontent-wide()` | **no** |

Q1 (working) body classes for the same project:
`nav-sidebar floating quarto-light` — `floating` triggers the sidebar
column. Q1 builds these classes dynamically in
`external-sources/quarto-cli/src/project/types/website/website-navigation.ts:526–554`:

- adds `nav-sidebar` when a sidebar is present and not hidden
- adds `floating` or `docked` mirroring the sidebar's style
- adds `nav-fixed` when a fixed-top navbar is present
- (theme classes like `quarto-light` come from another path)

### Issue 2: sidebar wrapper grid placement

Q2 template (`template.rs:171–175`) emits:

```html
<div id="quarto-sidebar-container" class="sidebar-column">
  <nav id="quarto-sidebar" class="sidebar sidebar-navigation sidebar-floating">…</nav>
</div>
```

Q1 emits the `<nav id="quarto-sidebar">` **directly** as a grid child
of `#quarto-content` (no wrapper). The SCSS in `resources/scss/`
targets `nav#quarto-sidebar` directly (verified: zero matches for
`quarto-sidebar-container` or `sidebar-column` in the SCSS tree).

Concrete consequence (computed-style readout from the Q2 page):
- `#quarto-sidebar-container` has
  `grid-column: body-content-start / body-content-end` and
  `grid-row: auto`.
- `<main>` has the same column track and `grid-row: content-top /
  content-bottom`.
- With `body.fullcontent`, `grid-template-columns` collapses
  `[page-start page-start-inset]` to a single line — there is no
  left sidebar column at all.
- The sidebar's auto-row resolves to an implicit row **after**
  `content-bottom`, so it lands below the page content. Prev/next
  is rendered inside `<main>` (`template.rs:222–224`), so the
  sidebar ends up below that too — exactly the user's observation.

### Issue 3 (secondary, smaller): missing `quarto-container` class

Q1's `#quarto-content` carries `quarto-container page-columns
page-rows-contents page-layout-article`. Q2 omits `quarto-container`.
Today nothing in `resources/scss/` selects on `quarto-container`, so
this is cosmetic. Worth fixing for parity but not load-bearing for
the sidebar bug.

## Out of scope (for this task)

- **Search bar** — Q1 renders a search input inside the sidebar by
  default; Q2 has no search support yet. We are not adding it here.
  The user explicitly confirmed this on 2026-04-29.
- **Navbar / header** — Q1 puts a `<header id="quarto-header">` above
  `#quarto-content`. Q2's template has navbar injection at the right
  place (`template.rs:163–165`), but the body class `nav-fixed` and
  the precise navbar-padding behavior are out of scope here. We will
  add only the sidebar-driven body classes (`nav-sidebar`,
  `floating`/`docked`); navbar-driven classes can be a follow-up.
- **TOC sidebar (`#quarto-margin-sidebar`)** — already correctly
  placed by the existing template; not changing.

## Tests (TDD: write first, then implement)

All tests live in `crates/quarto-core/`. The website pipeline already
has end-to-end fixtures under
`crates/quarto-core/tests/fixtures/websites/`; we extend that
coverage here.

- [ ] **Unit test (template-context level)**: given a render context
      with a sidebar in `navigation.sidebar`, the rendered HTML body
      tag contains `nav-sidebar` and the sidebar-style class
      (`floating` or `docked` — `floating` is current default for
      `SidebarStyle::Floating`).
- [ ] **Unit test (template-context level)**: given a render context
      with `sidebar: false` (or no `navigation.sidebar`), the body
      tag does NOT contain `nav-sidebar` / `floating` / `docked`.
- [ ] **Unit test (template structure)**: the rendered HTML contains
      `<nav id="quarto-sidebar"` as a direct child of
      `<div id="quarto-content"` (no `quarto-sidebar-container`
      wrapper). Match by string-search on the rendered template
      output.
- [ ] **Existing tests**: update any of the
      `template.rs` tests (`test_full_template_*`,
      `test_full_template_body_classes`) that asserted on the old
      `quarto-sidebar-container` wrapper or hardcoded `fullcontent`.
      Identify and list them in Phase 1; do not change their
      intent, only their expected output.
- [ ] **Integration test (`crates/quarto-core/tests/sidebar_pipeline.rs`)**:
      a website fixture that exercises the full render. Assert that
      the resulting HTML body tag contains `nav-sidebar floating`
      and that `<nav id="quarto-sidebar">` appears as a direct
      child of `#quarto-content` (text search, no DOM parser
      needed).

End-to-end verification (per CLAUDE.md "End-to-end verification
before declaring success"):

- [ ] Re-render `examples/websites/01-minimal` with `cargo run --bin
      q2 -- render` and inspect the produced `index.html` directly.
      Confirm the body tag and the sidebar's grid placement.
- [ ] Reload `127.0.0.1:8080` (the user's running static server) and
      confirm via Chrome DevTools that:
      - body class includes `nav-sidebar floating`
      - `#quarto-sidebar`'s computed `grid-column` resolves to
        `page-start / body-start` (or equivalent left-column
        track), not `body-content-start / body-content-end`
      - the sidebar visually sits to the left of `<main>`, not
        below it
- [ ] Re-check at least one other example (e.g.
      `examples/websites/02-auto-sidebar`) to confirm the fix
      generalizes.

## Implementation phases

### Phase 1: tests first

**Audit findings:**
- Only one existing test asserts on the broken behavior:
  `test_full_template_body_classes` at `template.rs:1581` — asserts
  `<body class="fullcontent my-class another-class">`. It will be
  updated to assert `<body class="my-class another-class">` (no
  hardcoded `fullcontent` prefix when `body-classes` is set).
- No `.snap` files reference `fullcontent`, `quarto-sidebar-container`,
  or `sidebar-column`, so no snapshot churn.

**Design decisions during phase 1:**
- The user's `body-classes` metadata (if set) wins absolutely. No
  appending to `fullcontent`.
- The template's `body-classes` template variable becomes "the full
  class string"; `fullcontent` only fires as a fallback when the
  variable is unset.
- The transform writes to `rendered.navigation.body-classes`. The
  context-build step in `render_with_compiled_template` promotes it
  to the top-level `body-classes` variable iff the user did not set
  `body-classes` directly. (`elseif` in our template engine is
  unreliable per a code comment in `evaluator.rs:568`, so the
  resolution happens in Rust, not in the template.)

**Tests to add/update:**
- [x] `template.rs::test_full_template_body_classes` — updated
      assertion to drop `fullcontent` prefix.
- [x] `template.rs::test_full_template_default_body_class_is_fullcontent`
      (new) — with no `body-classes` set, body class is exactly
      `fullcontent`.
- [x] `template.rs::test_full_template_no_sidebar_wrapper` (new) —
      rendering with `rendered.navigation.sidebar` set produces no
      `<div id="quarto-sidebar-container">` wrapper; the sidebar
      HTML appears between `#quarto-content`'s opening tag and
      `<main>`.
- [x] `template.rs::test_full_template_quarto_container_class`
      (new) — `#quarto-content` carries `quarto-container` for
      parity.
- [x] `sidebar_render.rs::sidebar_render_writes_body_classes_floating`
      (new).
- [x] `sidebar_render.rs::sidebar_render_writes_body_classes_docked`
      (new).
- [x] `sidebar_render.rs::sidebar_render_skips_body_classes_when_disabled`
      (new).
- [x] `sidebar_render.rs::sidebar_render_honors_user_body_classes_override`
      (new).
- [x] `sidebar_pipeline.rs::pipeline_renders_sidebar_for_two_page_website`
      (extended) — body-class and wrapper-drop assertions added.
- [x] All new tests fail in the expected way before implementation.
      Confirmed via `cargo nextest run` 2026-04-29:
      - 3/4 template tests fail (default-body-class passes — body is
        already `fullcontent` today).
      - 2/4 sidebar_render tests fail (skip + user-override pass
        because the absence of `body-classes` writing is itself the
        skip behavior).
      - integration test fails on the body-class assertion.

### Phase 2: derive body classes

**Decision (2026-04-29, user):** compute body classes inside
`SidebarRenderTransform` and write them to the sidebar's rendered
metadata. The template (or a small downstream context-build step)
picks them up. This preserves the `*GenerateTransform` /
`*RenderTransform` split — a user who wants different classes can
write their own filter that manipulates the rendered metadata
between transform and template.

- [x] In `crates/quarto-core/src/transforms/sidebar_render.rs`,
      compute `nav-sidebar {floating|docked}` from `Sidebar.style`
      and write it to `rendered.navigation.body-classes`. Module
      doc-comment updated to describe the new contract and the
      independent skip checks.
- [x] In `render_with_compiled_template`, after metadata→context
      promotion, copy `rendered.navigation.body-classes` to the
      top-level `body-classes` variable iff the user did not set
      `body-classes` directly.
- [x] Drop the hardcoded `fullcontent` prefix from the body-class
      template expression. Now: `<body class="$if(body-classes)$$body-classes$$else$fullcontent$endif$">`.
      `body-classes` is the full class string when set; the
      `fullcontent` literal only fires as fallback when no sidebar
      transform ran (and no user override).
- [x] Single live entry point: `render_with_compiled_template`
      (used by `render_with_resources`, `render_with_format`, and
      the pipeline-driven full-template path). The legacy
      `render_with_template` does not pass through
      `render_with_compiled_template` and therefore does not get
      the body-classes promotion — but it also does not run the
      sidebar transform, so it is unaffected.

### Phase 3: drop the wrapper

- [x] Removed `<div id="quarto-sidebar-container" class="sidebar-column">`
      from `FULL_HTML_TEMPLATE`. The `$rendered.navigation.sidebar$`
      output (`<nav id="quarto-sidebar">…</nav>` from
      `sidebar_to_html`) is now a direct grid child of
      `#quarto-content`. The SCSS rule
      `body.floating .sidebar.sidebar-navigation { grid-column:
      page-start / body-start; ... }` in
      `resources/scss/bootstrap/_bootstrap-rules.scss:289–294`
      now applies as intended.

### Phase 4: minor parity

- [x] Added `quarto-container` to `#quarto-content` class list.
      Touches every full-template render. The byte-identity
      baseline at
      `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`
      was re-captured (single hash change, only delta is the
      `quarto-container` token; styles.css unchanged). The fixture
      file's leading comment was extended to record the bd-mgoh
      re-capture and reason.

### Phase 3: drop the wrapper

- [ ] Remove `<div id="quarto-sidebar-container" class="sidebar-column">`
      and emit the `$rendered.navigation.sidebar$` HTML directly as
      a grid child of `#quarto-content`. Order in the template
      should match Q1: sidebar first, then margin-sidebar, then
      `<main>`.
- [ ] Confirm `crates/quarto-navigation/src/render_html.rs::sidebar_to_html`
      produces a `<nav id="quarto-sidebar">…</nav>` already (it does
      — verified). No changes needed there.

### Phase 4: minor parity

- [ ] Add `quarto-container` to the `#quarto-content` class list
      for parity. Document in the commit message that no SCSS rule
      currently selects on it (it's defensive; keeps us aligned
      with Q1 in case future SCSS imports rely on it).

### Phase 5: verify and snapshot

- [x] `cargo nextest run -p quarto-core` — 1425 passed, 33
      skipped. The single byte-identity baseline test was the only
      regression (intentional: `quarto-container` addition);
      baseline file re-captured.
- [x] `cargo nextest run --workspace` — 8089 tests passed (1
      leaky), 195 skipped.
- [x] `cargo xtask verify --skip-hub-build` — passed (run by user
      in a separate terminal; no warnings or failures reported).
- [x] End-to-end check on `01-minimal`:
      - rendered with `q2 render`, then loaded in Chrome at
        `http://127.0.0.1:8080/`.
      - body class: `nav-sidebar floating` ✓
      - `#quarto-content` class: `quarto-container page-columns
        page-rows-contents page-layout-article` ✓
      - `#quarto-sidebar` is direct child of `#quarto-content`,
        wrapper gone ✓
      - `#quarto-sidebar` computed `grid-column`: `page-start /
        body-start` (left sidebar column from
        `page-columns-float-wide()` mixin) ✓
      - bounding rects: sidebar at `left=310, right=560, top=0`;
        main at `left=585.5, top=17`; sidebar is **left of** main
        and **not below** ✓
- [x] End-to-end check on `02-auto-sidebar`:
      `<body class="nav-sidebar floating">` and direct-child
      sidebar confirmed.
- [x] End-to-end check on `03-nested-sidebar` (uses `style:
      docked`):
      - guide pages: `<body class="nav-sidebar docked">` ✓
      - reference pages: `<body class="nav-sidebar docked">` ✓
      - project root index (no sidebar matches): `<body
        class="fullcontent">` ✓ (correct fallback)
- [ ] Snapshot/baseline churn documented in commit message —
      single hash file changed
      (`tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`),
      no `.snap` files affected.

## Resolved questions (from plan review 2026-04-29)

1. **Where to compute body classes** — inside
   `SidebarRenderTransform`, written to a new field on the rendered
   sidebar metadata (e.g. `rendered.navigation.body_classes`). The
   template-context build then forwards it to the `body-classes`
   template variable. Rationale: keeps the
   `*GenerateTransform` / `*RenderTransform` split honest — a user
   who wants different body classes writes a filter that
   manipulates the rendered metadata between the two transforms,
   matching the existing pattern for the sidebar HTML itself.
2. **Theme classes (`quarto-light` / `quarto-dark`)** — out of
   scope. Separate task when themes are wired.
3. **Test fixture** — prefer extending an existing one in
   `crates/quarto-core/tests/fixtures/websites/` to avoid snapshot
   churn. If no existing fixture is a clean fit, add a small new
   one. Decide during phase 1.

## Files likely to change

- `crates/quarto-core/src/template.rs` — template string and the
  context-building code that feeds it.
- A new or existing pipeline stage (TBD during phase 2) that
  computes the body-class string.
- Snapshot files under `crates/quarto-core/` that capture full
  HTML output — these will update.
- `crates/quarto-core/tests/sidebar_pipeline.rs` — new assertion(s).
