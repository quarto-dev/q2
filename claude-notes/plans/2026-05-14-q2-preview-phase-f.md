# q2 preview — Phase F plan

**Epic:** bd-kw93 (q2 preview)
**Predecessor:** Phases A, B, C, D all merged on `feature/q2-preview-command`.
**Date:** 2026-05-14
**Status:** Sub-tasks filed bd-kw93.14 (F.1) and bd-kw93.15 (F.2); both ready to pick up. Plan signed off 2026-05-14.

## Progress

- [ ] **F.1** (bd-kw93.14) — Cross-page navigation + link-rewriting + Bootstrap JS.
- [ ] **F.2** (bd-kw93.15) — All chrome injection + favicon + docs closeout. Blocked by F.1.

> **Note on phase naming.** Phase E (epic plan §"Phase E — Stretch") is the
> post-MVP stretch bucket: `.tsx` hot-reload, multi-window verification,
> `q2 preview --share <url>`. None of those are blockers for building the
> Q2 docs site, and none are filed yet. Phase F jumps ahead to ship the
> website-chrome work the docs site actually needs; Phase E stays in the
> epic plan as an explicit "later" marker.

## Goal

Phase F closes the chrome-rendering gap between `q2 preview` and
`q2 render --format html`. After Phase F, running `q2 preview` against a
website project shows the navbar, sidebar, prev/next page-nav, table of
contents, page footer, and favicon — the same chrome a browser sees
when serving the `q2 render` output. Cross-page click navigation works
in-place (no SPA reload), with browser back/forward and anchor links
honoured.

This work is what makes `q2 preview` viable as the authoring loop for
the Q2 docs site.

## Settled decisions

These are the answers from the 2026-05-14 design conversation; folded
into the sub-task plans below.

- **F-D1 (Chrome render strategy):** **HTML injection.** Include the
  `*-render` transforms in the q2-preview pipeline so
  `meta.rendered.navigation.{navbar,sidebar,page-nav,toc,footer}` is
  populated with Bootstrap HTML. `PreviewDocument.tsx` injects each
  via `dangerouslySetInnerHTML` keyed on the string content so React
  re-renders chrome only when the server-emitted HTML changes.
  Pragmatic first cut, shippable in days, byte-identical to
  `q2 render`. The DOM-stability compromise (open Bootstrap dropdowns,
  sidebar scroll position) is bounded: chrome only re-renders on
  `_quarto.yml` edits, page switches, or sidebar reorderings — all
  uncommon during normal authoring. **bd-d8fo** is filed as the
  longer-term follow-up to replace HTML injection with proper React
  components driven by the structured `meta.navigation.*` data.

- **F-D2 (Cross-page navigation UX):** **Full SPA-style routing.**
  Click `.html` link → swap `activeFile` in-place. `history.pushState`
  so the browser back/forward buttons walk the in-SPA navigation.
  Anchor links (`foo.qmd#section`, after rewrite `foo.html#section`)
  scroll to the section after the new page renders. Missing-page link
  surfaces through the existing `PreviewErrorOverlay` from Phase D.4
  rather than blanking the iframe.

- **F-D3 (Link rewriting strategy):** **All hrefs become `.html`;
  SPA intercepts `.html` clicks.** Include `link-rewrite` in the
  q2-preview pipeline (it's currently in
  `Q2_PREVIEW_TRANSFORM_EXCLUDED`). The iframe's existing
  click-handler infrastructure gets extended to recognize `.html`
  paths and map them back to the corresponding `.qmd` via the file
  index. User context (2026-05-14): a separate service-worker PR is
  imminent and will provide much stronger proxy-like infra for this
  pattern, so leaning into `.html`-everywhere now sets us up for the
  service-worker world cleanly.

- **F-D4 (Chrome scope):** **All chrome that exists in `q2 render`
  today.** That's navbar, sidebar, page-nav, TOC, footer, favicon.
  Q2 doesn't yet have breadcrumbs, light/dark toggle, or locale
  picker server-side, so they're not in F either.

- **F-D5 (Test rigor):** **Structural assertions.** Playwright
  asserts that chrome DOM elements exist and contain expected text
  (e.g. `getByRole('navigation')` matches; clicking navbar links
  changes the active page; sidebar highlights the current page).
  Existing Rust unit tests on each `*-render` transform stay as
  they are. No byte-for-byte snapshot parity with `q2 render` — that
  level of rigor moves to bd-d8fo's acceptance, where it pays off
  (the React-components phase has real drift risk because two
  separate code paths emit HTML for the same data).

## Open questions

None blocking; deferred to sub-task plans:

- **F-Q1:** When `_quarto.yml` changes, does the chrome re-render
  picture up the new structure cleanly? The `dangerouslySetInnerHTML`
  approach should handle it because the string changes, but verify
  with a Playwright spec.

- **F-Q2:** Bootstrap JS (collapse toggles, dropdown menus) — does
  the embedded Bootstrap bundle in the preview SPA already include
  the JS the chrome HTML needs, or do we need to inject it? The
  `BootstrapJsStage` is `#[cfg(not(target_arch = "wasm32"))]` so
  it's *not* part of the WASM pipeline; this needs investigation
  during F.1.

- **F-Q3:** When `link-rewrite` is included in q2-preview, internal
  body links also become `.html` URLs. The iframe interceptor needs
  to handle them, but it also can't intercept *external* `.html`
  links (links to other sites that happen to end in `.html`). The
  check should be: target path is in the project index. Verify.

## Dependency order

Phase F is **two sub-tasks** by design (carlos@ 2026-05-14): one for
the cross-page navigation + Bootstrap-JS infrastructure, one
bundling all five chrome injections plus the favicon and the docs
update. Fewer sub-tasks → fewer hand-offs → less mid-flight
question-answering. The chrome injections all share the same
pattern (remove from `Q2_PREVIEW_TRANSFORM_EXCLUDED` + add a
`dangerouslySetInnerHTML` slot keyed on a meta path), so bundling
them is cheap.

```
F.1 (cross-page navigation + Bootstrap JS infra)
   │  Includes link-rewrite in the q2-preview pipeline; wires
   │  onNavigateToDocument in PreviewApp.tsx; history.pushState;
   │  popstate listener; anchor scrolling after render;
   │  missing-page error overlay. Verifies Bootstrap JS is loaded
   │  in the iframe so the dropdown / collapse JS in the chrome
   │  HTML works (model on MathJsStage's WASM-pipeline inclusion
   │  pattern). Must land first — F.2 emits .html hrefs that need
   │  somewhere to go, and emits Bootstrap-flavored chrome HTML
   │  that needs the JS to be interactive.
   ↓
F.2 (all chrome injection + favicon + docs)
   │  Removes from Q2_PREVIEW_TRANSFORM_EXCLUDED:
   │  navbar-render, sidebar-render, page-nav-render,
   │  toc-render, footer-render, website-favicon. Adds five
   │  dangerouslySetInnerHTML slots in PreviewDocument.tsx
   │  consuming meta.rendered.navigation.{navbar,sidebar,
   │  page-nav,toc,footer}. Truths up docs/q2-preview.qmd.
   │  Final Phase F closeout smoke.
```

## Work breakdown

### F.1 — Cross-page navigation + link-rewriting + Bootstrap JS

Foundational. Without this, the chrome renderers in F.2 emit
`.html` hrefs that go nowhere and Bootstrap-driven dropdowns /
collapses don't work.

**Affects:**
- `crates/quarto-core/src/pipeline.rs` — remove `"link-rewrite"` from
  `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
- `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts` —
  extend the click-handler path to recognize `.html` URLs that map
  back to project `.qmd` files. Check against the project file index
  (NOT just the `.html` suffix) so external `https://example.com/foo.html`
  links don't get hijacked. The existing `onNavigateToDocument`
  callback signature is reusable.
- `q2-preview-spa/src/PreviewApp.tsx` — wire
  `onNavigateToDocument` so a callback invocation calls
  `setState({ ..., activeFile })`. Also `history.pushState`. Also
  `popstate` listener so browser back/forward walks the in-SPA
  history.
- `q2-preview-spa/src/PreviewApp.tsx` — after `activeFile` change
  triggers a render, scroll to the anchor from `window.location.hash`
  if present. Coordinate with the iframe's post-render hook, not the
  state change, to avoid the race (Risk 3 below).
- **Bootstrap JS** — model on `crates/quarto-core/src/stage/stages/math_js.rs`:
  MathJS ships in the WASM pipeline because the engine "typesets
  once on `DOMContentLoaded`" and holds no cross-reinit state.
  Bootstrap 5 auto-initializes via `data-bs-*` attributes, so the
  same "load once at iframe boot, let it auto-attach to any chrome
  DOM that lands later" pattern works. Two options to evaluate
  during implementation:
  1. Include `BootstrapJsStage` in the WASM pipeline (currently
     `#[cfg(not(target_arch = "wasm32"))]`). Easy if it works.
  2. Statically inject `bootstrap.bundle.min.js` into the iframe
     template alongside the existing Bootstrap CSS. Cleaner for
     preview because it bypasses any state-preservation concerns
     the original gate was guarding against.
  Pick whichever's cleaner; document the choice in the commit
  message.

**Acceptance:**
- Playwright spec navigates a multi-page fixture by clicking
  body links + uses `window.history.back()` to verify back-button
  works. Anchor links (`foo.qmd#section`, after rewrite
  `foo.html#section`) scroll to the right section after navigation.
  Missing-page link surfaces the error overlay from D.4.
- Playwright spec verifies a Bootstrap-driven element in the body
  (e.g. a `details` element or a manually-authored `data-bs-toggle`
  div) actually toggles when clicked. (We'll lean on this same
  Bootstrap JS in F.2 for the chrome.)
- Existing tests don't regress.

---

### F.2 — All chrome injection + favicon + docs closeout

Bundles the five chrome injections + favicon polish + docs
truthing-up. Each chrome item follows the same pattern, so writing
them as one sub-task is cheaper than five hand-offs.

**Affects:**
- `crates/quarto-core/src/pipeline.rs` — remove from
  `Q2_PREVIEW_TRANSFORM_EXCLUDED`:
    - `"navbar-render"`
    - `"sidebar-render"`
    - `"page-nav-render"`
    - `"toc-render"`
    - `"footer-render"`
    - `"website-favicon"`
- `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx`
  — add five `dangerouslySetInnerHTML` slots, each reading the
  corresponding `meta.rendered.navigation.<key>` string and
  rendering a `<div>` keyed on that string so React only re-renders
  the slot when its content actually changes:
    - Navbar at top of `<div id="quarto-content">`.
    - Sidebar in the page's sidebar slot (per Bootstrap markup
      conventions — read what `q2 render` emits for the same
      fixture to copy the wrapper structure).
    - Page-nav below `<main class="content">`.
    - TOC in the page's TOC slot.
    - Footer at bottom of `<div id="quarto-content">`.
- `docs/q2-preview.qmd` — remove the "limitations" bullet about
  missing chrome; update the "what 'live' means" section to
  mention chrome re-render semantics.
- `claude-notes/plans/2026-05-11-q2-preview-epic.md` — mark Phase F
  done, with a sentence in the epic-plan status line.

**Implementation note (DOM re-render avoidance):**

The `dangerouslySetInnerHTML` approach must be careful to not
re-render chrome on every page edit. The simplest pattern:

```tsx
const navbarHtml = extractMetaString(meta.rendered?.navigation?.navbar);
return <NavbarSlot html={navbarHtml ?? ''} />;

// in NavbarSlot.tsx:
const NavbarSlot = memo(({ html }: { html: string }) => (
  <div dangerouslySetInnerHTML={{ __html: html }} />
));
```

`React.memo` + prop-equality on the string means the slot's
inner DOM is preserved when chrome HTML hasn't changed. The
chrome only re-renders when `_quarto.yml` changes (rare) or the
active page changes (expected; the sidebar's highlighted item
changes, the page-nav's prev/next change, the TOC changes).

**Acceptance:**
- Playwright specs against `examples/websites/04-navbar-footer/`,
  `examples/websites/02-auto-sidebar/`, `examples/websites/03-nested-sidebar/`:
    - Navbar items render and are clickable; clicking switches the
      active page (proves F.1 ↔ F.2 integration).
    - Sidebar renders; active page is highlighted; clicking other
      pages switches and re-highlights.
    - Page-nav (prev/next) renders on pages that have sidebar
      ordering.
    - TOC renders on pages with sections; clicking entries scrolls
      to the section.
    - Footer renders on projects that configure `page-footer:`.
    - Bootstrap-driven elements inside the chrome (navbar dropdown
      menus, sidebar collapse toggles) actually open/close on
      click.
- Manual smoke against the q2 docs site (the user's in-flight
  authoring target): `q2 preview docs/` shows the full chrome,
  cross-page navigation works.
- `docs/q2-preview.qmd` renders cleanly and reflects the new
  reality.
- Workspace nextest 8952/8952 still green; Playwright N+5/N+5 green.

## Out of scope for Phase F

- **React components for chrome** (Strategy B). Tracked as
  **bd-d8fo** — pure refactor; ships HTML injection now, swaps to
  React when chrome state-preservation becomes a real complaint.
- **Light/dark mode toggle.** Not yet a Q2 feature server-side.
- **Locale picker.** Not yet a Q2 feature server-side.
- **Breadcrumbs.** Not yet a Q2 feature server-side.
- **Title-block parity.** The current React `PreviewTitleBlock`
  + minimal-mode synthesis handles the common cases. carlos@
  (2026-05-14) noted `q2 render` itself doesn't render title
  blocks perfectly; if a specific painful gap shows up while
  building the docs site, file as a follow-up rather than
  including in Phase F.
- **Search box** (in-navbar `search:` config). Whether the
  navbar-render transform carries it through to the HTML it emits
  is an unknown — if it does, F.2 picks it up automatically
  (since we inject the navbar HTML wholesale). If it doesn't, a
  follow-up.
- **Refactor to non-Bootstrap CSS framework.** This applies to
  `q2 render` too; same scope question for both. Out of scope
  for Phase F.

## Risks

1. **Bootstrap JS interaction.** Dropdown menus, collapse toggles,
   and the search box (if present) all rely on Bootstrap's bundled
   JS. The WASM preview pipeline omits `BootstrapJsStage`. If the
   embedded SPA bundle doesn't already include the JS, chrome
   markup will render but interactive elements won't work. Mitigate
   in F.1 with explicit verification.

2. **External `.html` link false positives.** `<a href="...">` links
   to external sites that happen to end in `.html` (e.g.
   `https://example.org/index.html`) must NOT be intercepted by the
   click handler. The check needs to be: target path is in the
   project file index, not just "ends in .html."

3. **Anchor-scroll race.** After `activeFile` changes, the render
   `useEffect` fires asynchronously and the new content lands in the
   iframe in a future tick. The scroll-to-anchor logic has to wait
   for the iframe's post-render hook to fire, not the activeFile
   state change. Tests need to cover both same-page anchor
   (`#section` only) and cross-page anchor (`other.qmd#section`).

4. **`dangerouslySetInnerHTML` security.** All HTML emitted by the
   `*-render` transforms is generated from project metadata the user
   wrote — same trust boundary as the rest of the rendered page. Not
   a new attack surface, but worth flagging that the chrome bypasses
   React's normal escaping.

5. **DOM stability cost.** Bootstrap dropdowns/collapses inside the
   chrome don't survive chrome re-renders (because
   `dangerouslySetInnerHTML` replaces the inner DOM). This is the
   known trade-off; bd-d8fo addresses it when it becomes painful.

## Sub-task issues (to file after sign-off)

Two bd-issues per the consolidated dependency order:

| Sub-task | Title | Blocked by |
|----------|-------|-------------|
| F.1 | Cross-page navigation + link-rewriting + Bootstrap JS | — |
| F.2 | All chrome injection + favicon + docs closeout | F.1 |

After filing, also add a discovered-from edge on **bd-d8fo** from
F.2 so the React-components follow-up stays linked back to the
HTML-injection work it eventually replaces.

## Pre-flight investigation receipts (2026-05-14)

Recorded so future sessions don't re-derive these:

- **q2-preview pipeline stage exclusions** at
  `crates/quarto-core/src/pipeline.rs::Q2_PREVIEW_STAGE_EXCLUDED`
  (line ~1000): `code-highlight`, `math-js`, `render-html-body`,
  `apply-template`. All four are HTML-string emission stages.
- **q2-preview transform exclusions** at
  `Q2_PREVIEW_TRANSFORM_EXCLUDED` (line 1057): includes the five
  chrome `*-render` transforms (navbar, sidebar, page-nav, footer,
  toc), `link-rewrite`, `website-favicon`, and three custom-node
  preservers (callout-resolve, crossref-render, title-block).
- **Each `*-render` transform emits HTML strings** into
  `meta.rendered.navigation.<key>` via
  `quarto_navigation::render_html::*` helpers (see
  `transforms/navbar_render.rs:119`,
  `transforms/sidebar_render.rs`, etc.).
- **React-side document wrapper:**
  `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx`.
  No consumers of `meta.rendered.navigation.*` today.
- **`onNavigateToDocument` callback** is plumbed through
  `iframePostProcessor.ts` and `PreviewDocument.tsx` but **not
  wired in `q2-preview-spa/src/PreviewApp.tsx`** — F.1 fixes this.
- **Empirical chrome confirmation:** `cargo run --bin q2 -- render
  examples/websites/04-navbar-footer/index.qmd` produces a navbar
  and footer (`grep` for navbar/page-footer/quarto-secondary-nav
  classes shows 6 matches; preview produces zero).
