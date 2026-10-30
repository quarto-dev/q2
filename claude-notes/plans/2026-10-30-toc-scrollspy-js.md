---
title: 'TOC scroll tracking: port Q1''s "On this page" JS, with a `toc-component:` React override (bd-toc-scrollspy-js-41e624d8)'
date: 2026-10-30
description: 'Assessment and design for the client-side half of the HTML table of contents: the active-link tracking and toc-expand collapsing that Quarto 1 ships in quarto.js, ported as a predicate-gated JS artifact, plus a way to replace the default with a React component through the bound-components mechanism.'
status: in-progress  # decisions taken 2026-10-30; implementation on `braid/bd-toc-scrollspy-js-41e624d8-toc-scrollspy-js`
braid:
  strand: bd-toc-scrollspy-js-41e624d8
  priority: P2
---

**Strand:** bd-toc-scrollspy-js-41e624d8. **Related:** bd-react-components-a8flgldm (bound components, landed), bd-s73m7at2 (the preview half of components, open), bd-e2kpwy7n (`toc-location`, in progress), bd-3swqhbsa (browser harness for rendered output, open), bd-ersobfbt (the quarto-nav.js port, the closest precedent).

**Branch:** `braid/bd-toc-scrollspy-js-41e624d8-toc-scrollspy-js` (worktree `.worktrees/bd-toc-scrollspy-js-41e624d8-toc-scrollspy-js`, off `main` at `aa58e8e1a`). Investigated in the main checkout at the same commit.

## Triage verdict

**Ready to design, and the default is small.** Everything the native
render needs exists: the TOC markup q2 emits already carries the hooks
Q1's script selects on, the artifact store ships predicate-gated JS
files in one transform (`quarto_nav_js.rs` is a complete template), and
the preview injects the same files from `entry.tsx`. The port of the
default behavior is one JS file of about 120 lines, one transform, and
one new option (`toc-expand`, which q2 does not read today).

The customization half is where the design work is. The bound-components
mechanism (bd-react-components-a8flgldm) gives us the runtime, the import
map, vendored React, `.tsx` transpilation and a props contract, but it
binds only to Divs and Spans *in the AST*. The TOC is template-emitted
chrome, so it needs a third kind of binding ("this element is emitted by
the template, not found in the AST") and a way to hand the component
the structured TOC. Both are small extensions, and the preview side of
a custom TOC component waits on bd-s73m7at2.

## What Quarto 1 does

Read from `external-sources/quarto-cli` on 2026-10-30.

**Render time** (`src/format/html/format-html-bootstrap.ts:398-425`, a DOM
postprocessor): adds `toc-active` to `nav#TOC`; sets
`data-toc-expanded` from `toc-expand` (`true` → `99`, a number → that
number, `false` → `-1`, unset → no attribute); adds `nav-link` to every
TOC link, `active` to the **first** link (so the no-script page shows
one active entry), and copies each `href` into `data-scroll-target`.

**Browser** (`src/resources/formats/html/quarto.js`, one
`DOMContentLoaded` closure of 885 lines, of which the TOC part is):

| lines | behavior | keep? |
|---|---|---|
| 58, 112-118 | select `nav.toc-active[role="doc-toc"]`, collect `a[data-scroll-target]` | yes |
| 121-143 | on link click, reveal the heading's anchor (`reveal-anchorjs-link`) | **no**: q2 has no anchor-sections, nothing to reveal |
| 145-186 | `updateActiveLink`: the last section whose `offsetTop - 200 <= pageYOffset` is active; at the bottom of the page the last entry; dispatches `quarto-sectionChanged` on change | yes |
| 357-520, 580-600, 690-705 | `manageSidebarVisiblity`: when a `column-screen`/`column-page` element scrolls under the TOC, the TOC's children fade out and a fixed `quarto-sidebar-toggle` strip appears | **no**, separate feature (below) |
| 708-770 | reader mode (`window.quartoToggleReader`, localStorage) | **no**: q2 renders no reader toggle |
| 771-822 | `walk`: collapse every nested `ul` unless `depth <= tocOpenDepth`, it holds the active link, or it directly follows the active link; `data-toc-expanded` missing means depth 1 | yes |
| 825-850 | throttled `scroll` (5 ms) and `resize` (10 ms) listeners re-run both | yes |

The TOC overlap rollup (`manageSidebarVisiblity`) is a distinct feature
with its own CSS (`.quarto-sidebar-toggle*`, present but unreferenced in
q2: `resources/scss/bootstrap/_bootstrap-rules.scss:2541-2585`) and its
own interaction with the margin-column layout. It is out of scope here
and worth its own strand if wanted; the port below leaves the hooks it
would need (the same `nav#TOC` element, the same script file).

## What q2 does today

**TOC pipeline** (Navigation phase, `pipeline.rs:2529-2535`):
`TocGenerateTransform` (`transforms/toc_generate.rs`; reads `toc`,
`toc-depth`, `toc-title`) writes the structured `navigation.toc`;
`TocRenderTransform` (`toc_render.rs:276-307`) renders it to the inner
`<ul>` at `rendered.navigation.toc`, each link already
`class="nav-link" data-scroll-target="#id"`; `TocLocationTransform`
publishes the placement flags. The `nav#TOC` wrapper has **three
emitters** that must stay in sync: the doctemplate partial
`TOC_BLOCK_PARTIAL` (`template.rs:650`), its Rust twin `toc_block_html`
(`sidebar_render.rs:222`, website pages with `toc-location: left`), and
the preview's `TocSlot` (`ts-packages/preview-renderer/src/q2-preview/chromeSlots.tsx`).
All three write `<nav id="TOC" role="doc-toc" class="toc-active">`.

What is missing relative to Q1's render step: no `active` on the first
link, no `data-toc-expanded`, and **no code reads `toc-expand`** at all
(`grep -rn toc-expand crates/` is empty). The CSS side is complete:
`.sidebar nav[role="doc-toc"] ul > li > a.active` is styled
(`_bootstrap-rules.scss:2091`), and the `collapse` class the walk adds
is Bootstrap's own `.collapse:not(.show) { display: none }`.

**No script ships for the TOC.** Consequences today: no entry is ever
active, and every nesting level is always expanded. Q1 with no
`toc-expand` shows only the top level plus the active trail, so the
port changes the default look of nested TOCs toward Q1.

**How JS reaches a page.** An artifact with a `js:` key becomes a
`<script src>` in `<head>`, emitted by `ApplyTemplateStage` in
`(link_order, key)` order; `Artifact::link_attribs` can add
`type="module"`. `QuartoNavJsTransform` (`transforms/quarto_nav_js.rs`)
is the precedent for a predicate-gated port of a Q1 script: an
`include_bytes!` of `resources/js/quarto-nav/quarto-nav.js`, a pure
`decide(meta)` function, Project-scoped artifacts under
`site_libs/quarto-nav/`, `cfg(not(wasm32))` because the preview has no
`ApplyTemplateStage`, and unit tests on the decision plus the integration
test `quarto-core/tests/integration/headroom_pipeline.rs` that renders a
site and checks the tag and the file.

**The preview** includes `toc-render` (it is not in
`Q2_PREVIEW_TRANSFORM_EXCLUDED`) and mounts the HTML through `TocSlot`
with `dangerouslySetInnerHTML`, re-mounting the `nav` whenever the TOC
HTML changes (every heading edit). The preview loads chrome JS by
injecting the same vendored files as inline scripts at module top of
`entry.tsx:48-80`; `quarto-nav.js` copes with React mounting the header
late by keying its init off the header *element* through a
`MutationObserver` ("q2 EXTENSION" note in its header). A TOC script
has the same problem, more often.

**Bound components** (bd-react-components-a8flgldm, landed 2026-10-13;
plan `2026-10-13-react-components.md`, docs
`docs/guides/authoring/components.qmd`). `components:` declares
`{name, src, export, bind, content, props}`; `ComponentsResolveTransform`
(Finalization phase, `HtmlRender` profile, `pipeline.rs:2688-2698`)
matches `bind` (`#id` or `.class`, `config.rs:58 Binding`) against
Div/Span attrs, sets `data-quarto-component="<name>"`, ships the module
graph (transpiling `.tsx` through `quarto-transpile`), registers the
vendored React ESM and `quarto-components.js`, and appends an import map
+ JSON manifest + module tag to the `header` include slot
(`components/runtime.rs:head_block`). The browser runtime does
`querySelectorAll('[data-quarto-component]')` over the **whole
document** and mounts `<Component attrs props children node host/>`,
where `children` is the captured no-script DOM. Two consequences for
this plan: a template-emitted element that carries the attribute is
mounted exactly like an AST element, with no runtime change; and the
manifest's per-component `props` is the natural carrier for the
structured TOC.

The preview half of components (bd-s73m7at2) is open: the preview does
not yet dispatch on `data-quarto-component`, so a custom TOC component
would show the default in `q2 preview` until that lands.

## Design

### D1. The default: a direct port, shipped as a gated artifact

- **File** `resources/js/quarto-toc/quarto-toc.js`, hand-written, not
  minified (the quarto-nav precedent), with the Q1 original quoted in
  `claude-notes/plans/toc-scrollspy-investigation/q1-toc-reference.md`
  for diff review. Contents: the five "keep" rows above, verbatim in
  behavior (`sectionMargin = 200`, the bottom-of-page rule, the throttle
  periods, the `walk` rules, the `quarto-sectionChanged` event). Dropped:
  anchor reveal, reader mode, the overlap rollup, each noted in the file
  header the way `quarto-nav.js` lists its deviations.
- **Re-initializable** (the one q2 extension, mirroring `quarto-nav.js`):
  `initForToc(navEl)` binds the listeners to one `nav#TOC` element and
  tears them down when the element identity changes; a
  `MutationObserver` on `document.body` re-checks. Native pages observe
  nothing after the first run; the preview re-binds after every
  `TocSlot` re-mount. `toc-location: body` and the website left regime
  both still produce exactly one `nav.toc-active[role="doc-toc"]`, so
  the selector is unchanged.
- **Transform** `TocJsTransform` (`transforms/toc_js.rs`, Navigation
  phase, registered right after `TocLocationTransform`,
  `cfg(not(wasm32))`): predicate `rendered.navigation.toc` non-empty,
  which is the same signal the three `nav#TOC` emitters gate on. Stores
  `js:quarto-toc` → `quarto-toc/quarto-toc.js`, Project scope. The key
  sorts after `js:quarto-nav:*` and before `js:tabsets`; nothing depends
  on the order since the script binds at `DOMContentLoaded`. When D2's
  `toc-component` is declared the artifact is **not** stored: the
  component owns the behavior, and two scripts toggling `.active` would
  fight.
- **`toc-expand`** (new option, read in `TocRenderTransform`): `true` →
  `99`, integer → itself, `false` → `-1`, as Q1. Emitted as
  `data-toc-expanded` on the **top-level `<ul>`** that
  `rendered.navigation.toc` already holds, not on `nav#TOC`. Reason:
  one producer (`toc_render.rs`) instead of three emitters to keep in
  sync, and the preview gets it for free. The script reads
  `navEl.querySelector('[data-toc-expanded]')`. Nothing targets the
  attribute in CSS, so the Q1 placement is not a compatibility surface.
  A user-supplied `rendered.navigation.toc` (the documented bypass)
  carries no attribute and gets the depth-1 default, as in Q1.
- **First link `active` at render time** (`toc_render.rs`), so the
  no-script page and the first paint match Q1 and the script's first
  `updateActiveLink` is a no-op at the top of the page. This touches the
  `toc-containers` smoke fixtures' expectations only if they assert on
  the link's class list; to be checked in Phase 0.

Not chosen: making the default itself a React component. It would
dogfood D2, but it costs about 200 KB of vendored React on every page
with a TOC for 120 lines of DOM code, and the user's stated goal is a
default that feels directly comparable to Q1. React loads only when an
author asks for it.

Not chosen for now: `IntersectionObserver` instead of `offsetTop`
arithmetic (the Phase 4 plan of 2026-01-28 suggested it). A direct port
first; the observer version can replace the internals later without
changing the contract.

### D2. Customization: `toc-component: <src>` through bound components

```yaml
toc: true
toc-component: my-toc.tsx     # document or _quarto.yml; a path like components[].src
```

desugars, in `TocRenderTransform`, to a `components:` declaration
appended to the document's list:

```yaml
components:
  - name: quarto-toc            # reserved name; a user declaration with it is Q-25-5
    src: my-toc.tsx
    bind: chrome                # new: "the template emits the host, do not search the AST"
    content: dom
    props: { title, entries, expanded, location }   # the structured TOC, see below
```

and publishes `rendered.navigation.toc-component = "quarto-toc"`, which
the three `nav#TOC` emitters turn into
`data-quarto-component="quarto-toc"` on the nav. `ComponentsResolveTransform`
then does everything else unchanged: ships the module graph, registers
React and the runtime, writes the manifest. The runtime finds the nav
like any host and mounts the component with:

- `attrs`: `{id: "TOC", classes: ["toc-active"], attributes: {role: "doc-toc"}}`;
- `props`: `{ title: string, entries: TocEntry[], expanded: number | null, location: "left" | "right" | "body" }` where `TocEntry = { id, title, level, number?, children }` with `title` as plain text (`ConfigValue::to_plain_json` of `navigation.toc`, the S2 encoder; `title_html` is a cheap addition if a component wants the markup);
- `children`: the static `<h2 id="toc-title">`, `<ul>`, and `.toc-actions` as rendered (`content: dom`), so a component that only wants to decorate can render `children` and add behavior around it;
- `host`: the `nav` element, which is what a scroll tracker needs.

Required extensions, all small:

1. `Binding::Chrome` in `components/config.rs`: `matches` is never
   called, Q-25-4 ("no element for a binding") is skipped, and the
   `bind:` parser accepts the literal `chrome` only for internally
   synthesized declarations (authors keep `#id` / `.class`; whether to
   expose `bind: chrome` publicly is an open question below).
2. Props carried as a `ConfigValue` map on the synthesized declaration,
   so `parse_components` and the manifest treat them like any `props:`.
3. The `data-quarto-component` attribute in `TOC_BLOCK_PARTIAL`,
   `toc_block_html` and `TocSlot`, each gated on
   `rendered.navigation.toc-component`.
4. `TocJsTransform` reads the same key and ships nothing when it is set.

Diagnostics come for free from the components machinery: a missing or
out-of-project `src` is Q-25-2, a transpile error is Q-25-9 with its
position, a user `components:` entry named `quarto-toc` is Q-25-5. One
new code is needed: `toc-component` set with `toc: false` or no
headings (no TOC rendered) is a located warning in the `Q-13-*`
navigation family, matching how `toc-location` warns only when a TOC
rendered.

Naming, considered and not chosen:

- `toc: { component: … }`: `toc:` is a boolean in Pandoc and in every
  q2 reader of it (`is_feature_disabled`, `TocGenerateTransform`), and
  the existing option family is flat (`toc-depth`, `toc-title`,
  `toc-location`, `toc-expand`). A flat `toc-component` fits.
- A plain `components:` entry with `bind: "#TOC"`: reads naturally, but
  `#TOC` is not in the AST, so it would need `ComponentsResolveTransform`
  to special-case one id. Keeping the chrome binding internal and the
  sugar explicit is less magic. If more chrome slots become bindable
  (navbar, sidebar, footer: the preview's bd-d8fo direction), the same
  `Binding::Chrome` + `rendered.navigation.<slot>-component` pattern
  repeats, and a general `components[].bind: toc | navbar | …` vocabulary
  can be added then.

### D3. The preview

- **Default**: inject `quarto-toc.js` in `entry.tsx` beside
  `quarto-nav.js` (same `?raw` import, same `data-q2-*` marker), relying
  on D1's re-init. The preview's `TocSlot` wraps the `<ul>` in a `<div>`;
  the walk counts depth on `UL` only, so the wrapper is harmless. The
  preview's iframe scrolls the document itself (the existing TOC test in
  `q2-preview-spa/e2e/chrome.spec.ts:348` reads `#quarto-margin-sidebar nav#TOC`), so `pageYOffset` arithmetic holds.
- **Custom component**: `TocSlot` emits `data-quarto-component` when
  the key is set (extension 3 above), so the day bd-s73m7at2 dispatches
  on the attribute, the TOC mounts the user's component with the same
  props (the structured TOC is already in `meta.navigation.toc` on the
  preview side; `TocSlot` passes it as `props`). Until then the preview
  shows the default markup and, since the default script skips a nav
  that carries the attribute, no scroll tracking; this is stated in the
  docs as the current preview limitation, the same sentence the
  components guide already uses.

### D4. Testing

- **Unit** (`transforms/toc_js.rs`): `decide(meta)` with and without a
  rendered TOC, with `toc-component` set; artifact key, path, scope.
  (`toc_render.rs`): `toc-expand` → attribute for `true`, `false`, `2`,
  unset; first link active; the synthesized declaration and
  `rendered.navigation.toc-component`.
- **Integration** (`quarto-core/tests/integration/toc_js_pipeline.rs`,
  modeled on `headroom_pipeline.rs`): a site with headings gets the
  `<script src="site_libs/quarto-toc/quarto-toc.js">` tag and the file,
  byte-identical to the resource; a page with `toc: false` gets
  neither; with `toc-component` the tag is absent and the manifest
  names `quarto-toc` with the entries in `props`.
- **Smoke-all** fixtures under `crates/quarto/tests/smoke-all/toc-containers/`
  or a new `toc/`: `ensureFileRegexMatches` for the script tag and
  `data-toc-expanded="-1"`, `ensureHtmlElements` for
  `nav#TOC a.nav-link.active`.
- **Browser, preview**: a Playwright test in
  `q2-preview-spa/e2e/chrome.spec.ts` that scrolls the iframe to the
  second section and expects its link to be `.active` and a nested
  `ul.collapse` to lose the class. This is the one automated browser
  check for the scroll behavior.
- **Browser, rendered output**: no harness exists (bd-3swqhbsa). Verify
  by hand over `q2 preview --static` through the DevTools MCP, as the
  components strand did, and record the checks in the plan. If
  bd-3swqhbsa is picked up first, the TOC fixture is its third spec.

## Work items

### Phase 0: tests first

- [ ] Unit tests for `decide` and `store_artifacts` in a new `toc_js.rs` (fail: module missing).
- [ ] `toc_render.rs` tests for `toc-expand` and first-link `active` (fail).
- [ ] Integration test `toc_js_pipeline.rs` (fail).
- [ ] Smoke fixture with the regex assertions (fail). Check the existing `toc-containers` fixtures for assertions the `active` class would change.

### Phase 1: native default

- [ ] `claude-notes/plans/toc-scrollspy-investigation/q1-toc-reference.md`: the Q1 code quoted with line numbers.
- [ ] `resources/js/quarto-toc/quarto-toc.js` + README entry in `resources/js/README.md`.
- [ ] `transforms/toc_js.rs`, registered after `toc-location`; pipeline ordering test like `test_quarto_nav_js_registered_after_nav_renders`.
- [ ] `toc-expand` in `TocRenderTransform`; `data-toc-expanded` on the top `<ul>`; first link `active`.
- [ ] Hand check in Chromium over `q2 preview --static`: active link follows scroll, bottom-of-page rule, nested levels collapse and expand, `toc-expand: true` keeps everything open.

### Phase 2: preview default

- [ ] `entry.tsx` injection; confirm re-init after a heading edit re-mounts `TocSlot`.
- [ ] Playwright scroll test in `q2-preview-spa/e2e/chrome.spec.ts`.

### Phase 3: `toc-component`

- [ ] `Binding::Chrome`, props on synthesized declarations, Q-25-4 skip.
- [ ] Desugaring in `TocRenderTransform`; `rendered.navigation.toc-component`; the new Q-13 warning when no TOC renders.
- [ ] Attribute in the three emitters; `TocJsTransform` opt-out.
- [ ] Fixture `crates/quarto/tests/integration/fixtures/components-toc/`: a website with a `.tsx` TOC that renders `props.entries` as its own list and tracks the active section with a tiny `useEffect` scroll listener; the e2e pin in `components_render.rs` style.
- [ ] `@quarto/components`: a `QuartoTocProps` type for the props shape.
- [ ] Hand check in Chromium.

### Phase 4: docs and bookkeeping

- [ ] `docs/guides/authoring/navigation.qmd`: `toc-expand`, `toc-component` (with the preview caveat), the default collapse behavior.
- [ ] `docs/guides/authoring/components.qmd`: a short "chrome components" paragraph pointing at `toc-component`.
- [ ] Error page for the new Q-13 code; `error_catalog.json`.
- [ ] Strands to file: TOC overlap rollup (`manageSidebarVisiblity` + the unreferenced `.quarto-sidebar-toggle` CSS); preview mounting of `toc-component` as a note on bd-s73m7at2; bd-3swqhbsa gets the TOC fixture listed.
- [ ] `cargo xtask verify` (the WASM leg sees `toc_render.rs`; the hub-client leg sees `entry.tsx` and `TocSlot`); Windows cross-check for `quarto-core`.

## Decisions (Carlos, 2026-10-30)

The five open questions below, answered the same day. The design text
above already matches; this is the record.

1. **`toc-component`** is the public spelling (the plan as a whole was
   approved with it).
2. **`bind: chrome` stays internal.** An author's `bind: chrome` is
   Q-25-6; the expectation is that other chrome slots will grow their
   own components later, and the public vocabulary is designed then.
3. **`data-toc-expanded` goes on the top-level `<ul>`**, one producer.
4. **No helper exports for component authors yet.** Designing an
   `@quarto/toc` module before a real custom TOC exists would be
   over-design; follow-up once one shows what it wants.
5. **`quarto-sectionChanged` is kept.**

Two further choices made while implementing:

- `toc-component` without a rendered TOC is **inert, with no
  diagnostic**, exactly as `toc-location` is; the new `Q-13` warning the
  first draft proposed was dropped because Q1 has no such option and the
  inert rule already exists for the family.
- **One fix over Q1, found in the browser check.** Above the first
  section (the title block, when the first heading sits more than 200 px
  down the page) Q1's `updateActiveLink` finds no qualifying section and
  leaves the previous link active, so scrolling back to the top of a long
  page kept the last entry highlighted. The port makes the first entry
  active in that case, which is also what the render-time `active`
  shows. Documented in the script header.
- **The scroll container is resolved, not assumed** (found by the
  preview Playwright test). The hub-client preview scrolls `#root`
  inside a viewport-sized iframe, so Q1's `innerHeight + pageYOffset >=
  body.offsetHeight` is always true there and the last entry was always
  active. The port finds the nearest scrollable ancestor of the first
  section (the window on every native page) and listens for `scroll` on
  the document in the capture phase, which sees both. The D3 claim that
  "the preview's iframe scrolls the document itself" was wrong.
- The leaf entries of `navigation.toc` omit `children` when empty
  (`TocEntry::to_config_value`); the props builder fills the key in so
  `QuartoTocEntry.children` is always an array.

## Open questions

1. **`toc-component` as the public spelling?** Alternatives were a
   nested `toc:` map (conflicts with the boolean) and a `components:`
   entry bound to `#TOC` (needs a special case). My recommendation is
   the flat key.
2. **Expose `bind: chrome` to authors now?** Internal-only in this
   strand is my recommendation; a public vocabulary of bindable chrome
   slots is a design of its own once a second slot wants it.
3. **`data-toc-expanded` on the `<ul>` (one producer) or on `nav#TOC`
   (Q1 placement, three emitters)?** Recommendation: the `<ul>`.
4. **Should the default script also export helpers for component
   authors** (a `trackActiveSection(links, host)` ESM entry a custom
   component can import through the import map as `@quarto/toc`)? It
   would let a component restyle without re-implementing scrollspy, at
   the cost of shipping the file as a module script with a self-init
   guard. Not needed for Phase 3's fixture; could be a follow-up once
   the first real custom TOC shows what it wants.
5. **Keep `quarto-sectionChanged`?** Nothing in q2 consumes it. The
   quarto-nav port dropped `quarto-hrChanged` on that ground; keeping
   this one is cheap and is a reasonable public hook for page scripts.
   Recommendation: keep.

## Risks

- **Default look changes.** Nested TOC levels collapse by default once
  the script ships, as in Q1. Sites that relied on the always-expanded
  q2 rendering need `toc-expand: true`. Worth a changelog line.
- **Three `nav#TOC` emitters.** Every attribute added to the nav must
  land in the partial, the Rust twin, and `TocSlot`; the existing
  `toc_block_html_twin_*` tests are the guard, and D1 keeps the new
  `data-toc-expanded` off the nav for that reason.
- **Preview re-init.** The observer pattern is proven by
  `quarto-nav.js`, but the TOC re-mounts far more often (every heading
  edit). Listener teardown must be exact or the preview leaks a scroll
  handler per edit; the Playwright test should edit twice and assert a
  single active link.
- **No rendered-output browser harness.** The scroll behavior of
  `q2 render` output is hand-verified until bd-3swqhbsa; the integration
  test pins the bytes and the tag, which is what caught nothing in the
  components work (the import-map bug was browser-only).

## CI follow-up (2026-10-30)

The hub-client smoke-all runner (both browsers) failed the two fixtures
that asserted `a.nav-link.active[data-scroll-target="#outer"]` through
`ensureHtmlElements`. That runner reads the live preview DOM after the
script has run, and the script applies Q1's rules: the entry whose
section is within `SECTION_MARGIN` of the top is current, and a page too
short to scroll makes the last entry current. Confirmed in Chromium on
the rendered fixture: `#inner` at a 300 px viewport, `#second` with no
scrollbar. The Rust and WASM runners parse the static HTML, where the
first link is active by construction.

Fix: the live-DOM assertions now ask only that some entry is active;
the first-link-active check moved to `ensureFileRegexMatches`, which
every runner evaluates on the rendered HTML. While there, the sandboxed
preview entry (`hub-client/quarto-hub-sandboxed-preview/src/entry.tsx`,
an adapted copy of the q2-preview entry) gained the same
`quarto-toc.js` injection, and the docs mention the short-page rule.
