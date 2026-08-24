# Website header: headroom.js scroll-away + fixed-top parity (bd-ersobfbt)

**Date:** 2026-08-21
**Braid:** bd-ersobfbt
**Branch:** `main` @ `587721bb` (investigated in the main checkout; no worktree created)
**Status:** Implementation in progress on topic branch `braid/bd-ersobfbt-headroom-fixed-top` (off `main` @ `587721bb`; investigation commits included). Started 2026-08-24.

Reference material collected during the investigation:
`claude-notes/plans/headroom-fixed-top-investigation/q1-headroom-reference.md`
(the complete Q1 implementation, file:line, quoted).

## Triage verdict

**Ready to design.** The prerequisite (bd-26bf3j1y, the `#quarto-header`
wrapper) is closed, every piece of infrastructure the native `q2 render`
side needs already exists, and the preview side is in better shape than the
strand description assumes — the hub-client iframe is *persistent* and
already ships Bootstrap JS. The open questions are about *which design* to
port (Q1's JS-driven body-padding machinery verbatim, or a `position:
sticky` re-expression), how much of the preview to cover, and whether
`pinned:` ships in the same change.

## Issue context

Filed 2026-08-17 by Carlos as the Q1-parity follow-up deferred from
bd-26bf3j1y (decision 2: the header wrapper ships *static* — no `headroom`,
no `fixed-top`, no `body.nav-fixed`). Priority 3, type feature. The
description's core constraint is still exactly right and is the organising
principle of this plan:

> `fixed-top`, `body.nav-fixed`, and the padding rule go together. Adding
> `fixed-top` without the compensation makes the header overlap content.
> Ship them in one change or not at all.

Also in scope per the description: `window.quartoToggleHeadroom` and the
guarded `onclick` hooks on the secondary-nav toggle / title link (and, in Q1,
the navbar hamburger too — `navtoggle.ejs:3`).

## Dependency graph

- **blocks / discovered-from → bd-26bf3j1y** (closed 2026-08-18): introduced
  `QUARTO_HEADER_PARTIAL` in `crates/quarto-core/src/template.rs:647` and the
  `SecondaryNavRenderTransform`. Its plan
  (`2026-08-17-website-secondary-nav-mobile.md`, resolved decisions 2 and 3)
  is the origin of this strand and of the "preview is skipped" stance —
  which, as §"Preview" below shows, rests on a stale premise.
- **Related (not linked): bd-e7b7** "Q2 website JS library loading (native +
  hub-client incremental-stable)" — open. Its description ("Currently no
  `<script>` tags emitted for site libs", "hub-client iframe must not
  re-execute modules per render") predates `BootstrapJsStage`
  (`js:*` artifacts) and Phase F.1's persistent-iframe Bootstrap injection.
  This strand will be the *third* consumer of that infrastructure
  (after bootstrap/clipboard/tabsets), and should link to bd-e7b7 as
  `related` rather than wait on it (edge added during this investigation).
- No incoming `blocks` edges: nothing is waiting on this. Urgency is purely
  "dogfooding parity" (Posit Connect docs).

## What the code looks like today

### Native `q2 render` — everything needed is in place

| Need | Exists? | Where |
|---|---|---|
| `<header id="quarto-header">` wrapper | yes | `template.rs:647` `QUARTO_HEADER_PARTIAL`; emitted when navbar *or* secondary nav exists (`template.rs:274-277`) |
| Ship a static JS file conditionally | yes | `stage/stages/bootstrap_js.rs` is the documented prototype ("predicate → Project-scoped `js:*` artifact"); `clipboard_js.rs`, `tabsets_js.rs` are siblings. `ApplyTemplateStage` emits `<script src>` in sorted-key order (`apply_template.rs:167`) |
| Vendored JS home | yes | `resources/js/{bootstrap,clipboard,tabsets}/` (+ README) |
| Body classes from Rust | yes, **but replace-not-append** | `rendered.navigation.body-classes` written by `SidebarRenderTransform` (`transforms/sidebar_render.rs:132-136`), consumed by `template.rs:903-923` **and mirrored by the preview** (`PreviewDocument.tsx:101-104`). The merge at `template.rs:911-915` is a `match` that *replaces*; the only producer runs only when a sidebar exists. Q1 sets `nav-fixed` on navbar-only pages too (`old-docs/_site/*.html` → `<body class="nav-fixed quarto-light">`), so `nav-fixed` needs its own producer/compose step — see question 7 |
| `navbar: pinned:` / `sidebar: pinned:` parsed | yes, **unused** | `quarto-navigation/src/navbar.rs:162,236` and `sidebar.rs:404,446` — the fields exist and default to `false`; nothing reads them |
| Headroom / nav-fixed SCSS | **no** | `grep -rn 'headroom\|nav-fixed\|navbar-default-offset' resources/scss` → nothing. `_bootstrap-rules.scss:457` explicitly notes Q1's print rule `.fixed-top { position: relative }` was *not* ported "because Q2 emits no `.fixed-top` navbar element" |
| `quarto-nav.js` equivalent (offset machinery) | **no** | q2 has no site-navigation JS at all. Q1's headroom init lives in `quarto-nav.js:202-238`, *not* `quarto.js`, and it is inseparable from the `updateDocumentOffset` machinery at `quarto-nav.js:105-200` (see below). Q1's compiled copies are **already in-repo** at `old-docs/_site/site_libs/quarto-nav/{headroom.min.js,quarto-nav.js}` (also `examples/websites/06-site-metadata/q1-site/site_libs/quarto-nav/`) — vendoring need not touch `external-sources/` |
| `onclick` hooks | no, deliberately | `render_html.rs:294` docs the absence; `render_html.rs:2961-2969` has an **absence-pin test** (`!html.contains("quartoToggleHeadroom")`) that must be flipped. Two more pins: `quarto-sass/src/compile.rs:1002-1007` documents the print `.fixed-top{position:relative}` rule as deliberately *not* ported, and `template.rs:2610` asserts the **exact string** `<header id="quarto-header" class="quarto-banner">`, which breaks the moment `headroom fixed-top` joins the class list |
| Sidebar / TOC sticky offset | hardcoded `top: 0px` | `_bootstrap-rules.scss:1770` `.sidebar.margin-sidebar { top: 0px }`; `:1763` `.sidebar { position: sticky; will-change: top; transition: top 200ms }` (the transition is already there, waiting for a JS `top` writer that does not exist) |

### The part the strand description under-weights: Q1 headroom ≠ headroom.js alone

Q1's behaviour is two cooperating layers (full quotes in the reference doc):

1. **`headroom.min.js` v0.12.0** (4.5 KB, MIT) — toggles
   `headroom--pinned` / `headroom--unpinned` on `#quarto-header` by scroll
   direction, `tolerance: 5`. Pure class toggling; the visual is
   `quarto-nav.scss:100-114` (`transform: translateY(-100%)` with a 200 ms
   transition).
2. **`quarto-nav.js`'s `updateDocumentOffset`** — because the header is
   `position: fixed`, *something* has to push the page down by the header's
   measured height. Q1 does this in JS, on load and on every pin/unpin and
   on a `ResizeObserver` of the header: sets `body.style.paddingTop`,
   every `.sidebar` / `.headroom-target`'s `style.top` + `maxHeight`
   (0 / 100vh when unpinned, headerHeight / `calc(100vh - h)` when pinned),
   `.quarto-container` `minHeight`, and a `section:target::before` spacer
   in a dynamic `<style id="quarto-target-style">` for anchor-jump
   compensation, then dispatches `quarto-hrChanged`.
   `body.nav-fixed { padding-top: navbar-default-offset($theme-name) }` —
   a 13-theme lookup table with a 64 px fallback — exists **only** to
   pre-guess that padding so the page doesn't jump before the JS runs.

Consequence: `pinned: true` in Q1 omits *only* layer 1
(`website-navigation.ts:1500-1541`); layer 2 still runs, because the header
is still `fixed-top`. So the minimum viable port of "fixed-top parity" is
layer 2, and headroom is the add-on. The strand title has it backwards in
emphasis — the scroll-away is the easy 4.5 KB; the offset bookkeeping is the
design.

### Preview (`q2 preview` / `format: q2-preview`) — the premise has moved

bd-26bf3j1y's decision 3 and the comment at
`crates/quarto-core/src/pipeline.rs:1375-1383` both say the hub-client
preview "reinitializes its iframe on every render tick", so Bootstrap JS
(and by extension any chrome JS) is gated `cfg(not(wasm32))`. **That is no
longer how the preview works:**

- `ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx:214-217`:
  "the q2-preview app re-renders its body via React on each `UPDATE_AST`
  but never reloads the iframe, so the contentWindow/document — and these
  listeners — persist across edits."
- `ts-packages/preview-renderer/src/q2-preview/entry.tsx:35-55` (Phase F.1,
  bd-kw93.14): Bootstrap 5's bundle is imported `?raw` from
  `resources/js/bootstrap/` and injected once at module top. The design
  note there is the right frame for headroom too: *"chrome JS is
  iframe-template responsibility, not document-render responsibility"* —
  i.e. do **not** try to route a `js:headroom` artifact through the WASM
  pipeline; `ApplyTemplateStage` is excluded from q2-preview anyway.
- `q2-preview/chromeSlots.tsx`: navbar/sidebar/footer HTML strings come from
  `meta.rendered.navigation.*` and are injected via memoised
  `dangerouslySetInnerHTML` slots wrapped in `display: contents` hosts.
- `utils/codeCopy.ts` is the established pattern for "native ships a JS
  library, preview re-implements the behaviour as one delegated listener on
  a stable React root".

Two preview facts that bear directly on this strand:

1. **The preview does not emit `<header id="quarto-header">` at all.**
   `PreviewDocument.tsx:280` renders `<NavbarSlot>` bare before
   `#quarto-content`; there is no header wrapper and (by the wasm32 `cfg`)
   no secondary nav. That is a *pre-existing* render/preview parity gap from
   bd-26bf3j1y — the `#quarto-header > nav { padding: 0 1em }` rule ported
   in `_bootstrap-rules.scss:3068` cannot match in preview today. This
   strand has to add the wrapper on the React side before it can make it
   `headroom fixed-top`. (Filed as incidental work — see §Strands.)
2. **`body.nav-fixed` needs no TS logic** if Rust writes it into
   `rendered.navigation.body-classes`: `PreviewDocument.tsx:101-119` already
   mirrors that key onto `document.body.className`.

Scroll-sync interplay to keep in mind: `scrollSyncDom.ts:96-98`
`isElementVisible` treats `rect.top >= 0` as visible, so an element sitting
under a fixed header would be judged visible and not scrolled to;
`scrollIntoView({block:'center'})` itself is unaffected. Editing overlays
(`editChromeGeometry.ts`, `caretGeometry.ts`) work in document coordinates
and should not care about a fixed header, but must be checked end-to-end.

## Assessment

Carlos's framing is right on both counts: the render side is a clean,
pattern-following addition (one stage + one vendored file + one JS init
file + SCSS + a template class + a body class + flipping three absence
pins), and the preview side is tractable *because* the iframe is persistent
— a `useEffect` in the chrome slot that constructs `Headroom` on a
React-owned `<header>` survives edits, and the `?raw` injection pattern from
F.1 is already there for the library. The thing to decide is not *whether*
but *which layout mechanism*, because that determines how much of
`quarto-nav.js` we port.

### Option A — port Q1 verbatim (fixed header + JS offset machinery)

Vendor `headroom.min.js`; write `resources/js/quarto-nav/quarto-nav.js`
as a port of Q1's `quarto-nav.js:105-269` (offset + headroom init +
hashchange + ResizeObserver); SCSS ports `navbar-default-offset`,
`body.nav-fixed`, headroom transforms, `.notransition`, print
`.fixed-top{position:relative}`. Preview: same `quarto-nav.js` injected
`?raw` at module top *or* a React re-expression of the same bookkeeping.

- \+ byte-level CSS parity: user SCSS that targets `body.nav-fixed`,
  `header.headroom--unpinned`, `[data-bs-offset]` keeps working; the
  `preview-render-parity` skill has a fixed target.
- \+ smallest design surface — the behaviour is already specified.
- − inherits Q1's known wart: the theme offset table is a guess, so custom
  themes / tall logos / banners get a load-time content jump; Q1 papers over
  it with `.notransition`.
- − ~170 lines of imperative DOM JS that writes inline `style.top` on every
  sidebar — a second writer for properties the SCSS also sets, and a
  `quarto-hrChanged` event with no q2 consumer yet.

### Option B — `position: sticky` header + CSS custom property

Keep `#quarto-header` in normal flow but `position: sticky; top: 0` (no
body padding, no flash, no per-theme table), still `class="headroom"` so
headroom.js's `translateY(-100%)` works identically. A ~30-line script sets
`--quarto-header-height` on `:root` from a `ResizeObserver` and the SCSS
consumes it: `.sidebar { top: var(--quarto-header-height, 0) }`,
`section:target { scroll-margin-top: var(--quarto-header-height) }`
(replaces the dynamic `<style>` spacer), and `header.headroom--unpinned ~ *
.sidebar { top: 0 }` for the unpinned case.

- \+ no layout JS on the critical path; no theme lookup table; no
  `.notransition` dance; anchor compensation becomes declarative.
- \+ fits the project's stated preference for re-expressing Q1 DOM
  mutation as structure the writer/CSS get right the first time.
- − diverges from Q1's DOM contract: no `body.nav-fixed`, no `fixed-top`,
  no `body[data-bs-offset]`. Anyone porting Q1 SCSS keyed on those finds a
  gap; the strand's own title promises "fixed-top parity".
- − sticky has edge cases Q1's fixed doesn't: ancestor `overflow` clipping
  (the preview's `display: contents` host is fine; hub-client's outer
  layout needs checking), and the announcement bar / draft alert that Q1
  injects *into* the header still work but the sizing is via the var.

**My lean:** Option A for the *DOM/CSS contract* (emit `headroom fixed-top`,
`body.nav-fixed`, the Q1 SCSS) but implement the offset bookkeeping once,
natively and in preview, as a **shared `quarto-nav.js` ported from Q1** —
i.e. not re-expressing in React. Rationale: the strand is explicitly a
parity strand, the `preview-render-parity` skill needs one target, and
the F.1 precedent already injects vendored JS unchanged into the preview
iframe. Option B is the better design in a vacuum; it is a different strand
("replace Q1's fixed-header layout") and should be argued on its own.
This is question 1 below.

## Work items

### Phase 0 — Tests first (TDD; each written, run, and confirmed failing before its implementation lands)

- [x] N1 `template.rs` partial tests: header emits `class="headroom fixed-top"`;
      banner mode composes `class="headroom fixed-top quarto-banner"` (flips the
      exact-string pin at `template.rs:2610`).
- [x] N2 `template.rs` body-class tests (accumulating list, decision 7b):
      rendered navbar present → `nav-fixed` appended after the structural
      class; navbar+sidebar → `nav-sidebar floating nav-fixed quarto-light`;
      no navbar → unchanged; user-supplied `body-classes` still wins whole.
- [x] N3 `render_html.rs`: navbar toggler + secondary-nav toggle + secondary-nav
      title link carry `onclick="if (window.quartoToggleHeadroom) { window.quartoToggleHeadroom(); }"`
      (flips the absence pin at `render_html.rs:2961-2969`).
- [x] N4 `transforms/quarto_nav_js.rs` unit tests: website+navbar → both
      artifacts (`js:quarto-nav:headroom`, `js:quarto-nav:nav`); `pinned: true`
      navbar → headroom omitted, nav kept; any pinned sidebar → same; no
      navbar/sidebar → nothing; non-website → nothing.
- [x] N5 `quarto-sass` compile tests: `body.nav-fixed` gets non-zero
      `padding-top`; `header.headroom--unpinned` has `transform:translateY(-100%)`;
      print block gains `.fixed-top{position:relative}` (updates the deliberate-
      absence comment at `compile.rs:1002-1007`).
- [x] N6 `quarto-core` integration test (website fixture, real ProjectPipeline):
      header classes present, `body.nav-fixed` present, both `<script>` tags
      under `site_libs/`, headroom script absent under `pinned: true`.
- [ ] P1 preview vitest: `PreviewDocument` wraps navbar (+ secondary nav) in a
      React-owned `<header id="quarto-header">` matching `template.rs`
      conditions; `document.body` gains `nav-fixed` when navbar HTML present.
- [ ] P2 preview vitest: header element identity is stable across an
      `UPDATE_AST` re-post (Headroom's binding survives chrome churn).

### Phase 1 — Native render

- [x] Vendor `resources/js/headroom/headroom.min.js` (v0.12.0, from
      `old-docs/_site/site_libs/quarto-nav/`; version-contract note in
      `resources/js/README.md`).
- [x] Write `resources/js/quarto-nav/quarto-nav.js` — ~60-line port of Q1's
      header machinery (headerOffset, updateDocumentOffset, Headroom init +
      `quartoToggleHeadroom`, hashchange compensation, ResizeObserver, 250ms
      initial measure). Documented deviations: no `.headroom-target`
      (no q2 producer), no footer/dashboard offsets, no `quarto-hrChanged`
      (no consumer). DOMContentLoaded-guarded so script order is irrelevant.
- [x] `QuartoNavJsTransform` (`transforms/quarto_nav_js.rs`,
      `cfg(not(wasm32))`, Navigation phase, `ProjectKind::Website` gate,
      reads `navigation.navbar`/`navigation.sidebar` + `pinned`); register in
      `pipeline.rs`; registration test.
- [x] Template: add `headroom fixed-top` to `QUARTO_HEADER_PARTIAL`; refactor
      the body-class merge (`template.rs:903-924`) into an accumulating list
      with `nav-fixed` pushed when `rendered.navigation.navbar` is non-empty.
- [x] `render_html.rs`: the three guarded `onclick` hooks.
- [x] SCSS: one fenced block "fixed-top header (bd-ersobfbt; replaced by
      bd-pt1wxeq2)" — headroom transition/transform rules, `body.nav-fixed`
      padding, `.notransition`, print `.fixed-top{position:relative}`;
      `navbar-default-offset()` map in `_bootstrap-functions.scss`.
- [x] Re-run snapshots/goldens; **measure and report churn to Carlos**
      (count + summary + surprises), incl.
      `phase5-single-doc-baseline/expected_hashes.txt`.
- [x] End-to-end verification: `cargo run --bin q2 -- render` a website
      fixture; inspect output HTML (record invocation + snippet in plan).

**Phase 0+1 completion notes (2026-08-24):**

- TDD confirmed for every leg: N1/N2 (4 red), N3 (2 red), N4 (7 red on
  stubs), N5 (red verified by stashing the SCSS), N6 (3 red on wrong
  path assumption, then corrected to `site_libs/`). All green after
  implementation; full workspace run: **13003 passed / 0 failed**.
- **Snapshot/baseline churn (report for Carlos): 0 `.snap` files
  changed; 1 baseline hash updated** —
  `phase5-single-doc-baseline/expected_hashes.txt` `doc_files/styles.css`
  (the new SCSS block; note appended in the file). `doc.html` hash
  UNCHANGED — the strand is byte-neutral for single-doc renders (no
  navbar → no header, no `nav-fixed`, no scripts). No other snapshot
  or golden moved, including the body-class golden tests, which pass
  unmodified: the accumulating-list refactor is behavior-identical
  when no navbar is rendered.
- End-to-end (per CLAUDE.md): `cargo run --bin q2 -- render <scratch>/hr-site`
  (2-page navbar website). Inspected `_site/index.html`:
  `<header id="quarto-header" class="headroom fixed-top">`,
  `<body class="fullcontent nav-fixed quarto-light">`, script tags in
  order `site_libs/quarto/bootstrap.bundle.min.js` →
  `site_libs/quarto-nav/headroom.min.js` →
  `site_libs/quarto-nav/quarto-nav.js`, toggler carries the guarded
  onclick, and compiled CSS contains `body.nav-fixed{padding-top:64px}`.
  Files verified on disk under `_site/site_libs/quarto-nav/`.
  Browser-level scroll behavior is verified in Phase 2's browser pass.

### Phase 2 — Preview

- [ ] React-owned `<header id="quarto-header">` wrapper in
      `PreviewDocument.tsx` (conditions mirror `template.rs:274-278`;
      `headroom fixed-top` + banner class; closes bd-2yd37vuk).
- [ ] Lift `cfg(not(wasm32))` on `SecondaryNavRenderTransform`
      (`pipeline.rs:1373-1379`); add `SecondaryNavSlot` to chromeSlots +
      PreviewDocument; update the transform's module docs.
- [ ] `nav-fixed` in `PreviewDocument.tsx` body-class computation (mirror of
      the Rust accumulating list).
- [ ] Inject `headroom.min.js` + `quarto-nav.js` `?raw` at `entry.tsx` module
      top (idempotent, `data-q2-*` markers, F.1 pattern).
- [ ] `npm run build:all` green; `npm run test:ci` green.
- [ ] End-to-end browser verification (`q2 preview` after full WASM rebuild
      chain, per CLAUDE.md §Verifying Rust changes in q2 preview): scroll
      pin/unpin works, no overlap, scroll-sync + edit overlays sane. Record
      in plan.

### Phase 3 — `pinned:` docs + schema

- [ ] `docs/` page(s) for `website: navbar: pinned` / `sidebar: pinned`;
      confirm YAML schema coverage; render docs/ with q2.

### Phase 4 — Cleanup + bookkeeping

- [ ] Rewrite stale iframe-reinit rationale: `pipeline.rs:1372-1383`,
      `bootstrap_js.rs:47-56`, `clipboard_js.rs:41-51`,
      `secondary_nav_render.rs:35-49`.
- [ ] Update header-is-static docs: `template.rs:616-666` partial doc,
      `title_banner.rs` module doc if stale.
- [ ] Stale template.rs line refs in `PreviewDocument.tsx` comments.
- [ ] Close bd-2yd37vuk (with the Phase 2 commit); braid comments on
      bd-ersobfbt at each phase boundary.
- [ ] Full `cargo xtask verify` (WASM leg affected) before hand-off.

## Resolved decisions (Carlos, 2026-08-24)

1. **Layout mechanism: Option A** (Q1-verbatim fixed header + JS offset
   machinery) *for this strand*, to decompose the work cleanly — **but the
   Option B redesign (bd-pt1wxeq2, filed) will likely start right away,
   possibly before this strand's PR merges.** Structure everything for the
   swap (see §Structuring for the Option B swap).
2. **One `quarto-nav.js` for both surfaces.** Inject the same vendored
   `quarto-nav.js` + `headroom.min.js` `?raw` at `entry.tsx` module top
   (F.1 precedent). No React re-implementation.
3. **No edit-mode special-casing.** Preview behaves identically to render;
   experience actual problems before guessing at a freeze policy.
4. **Lift the `cfg(not(wasm32))`** on `SecondaryNavRenderTransform` in this
   strand; add the preview slot for it. The `onclick` hooks then have a
   target in preview too.
5. **`pinned:` ships here** (stage predicate + docs).
6. **Port the 13-theme `navbar-default-offset` map verbatim** into
   `_bootstrap-functions.scss`, keyed on the existing `$theme-name`.
   Avoids load-jump surprises for migrating users on vendored Bootswatch
   themes; may not survive Option B, and that's fine.
7. **`nav-fixed` via option (b): refactor the body-class merge into an
   accumulating class list** (`template.rs:911-915` and the
   `PreviewDocument.tsx:101-119` mirror). Touches the golden body-class
   tests. **Commitment: measure and report to Carlos the snapshot/baseline
   churn this causes** (count + summary + surprising diffs, per the
   CLAUDE.md snapshot policy).
8. **React-owned `<header>` wrapper in preview is acceptable** — the
   element Headroom binds to persists across chrome HTML re-injection;
   only its children churn. No re-init machinery.

## Structuring for the Option B swap (bd-pt1wxeq2)

Decision 1's context: B starts soon after (maybe before) this merges, so A
must be cheap to unwind. Concretely:

- **One JS file.** The entire offset machinery lives in the single vendored
  `resources/js/quarto-nav/quarto-nav.js` (~60-line port, not the 325-line
  Q1 file). B deletes/replaces one file; `headroom.min.js` survives B
  unchanged (the scroll-away classes work on sticky too).
- **One compose point for body classes.** The accumulating class list
  (decision 7) is the only place `nav-fixed` is added — native and preview
  read the same `rendered.navigation.*` keys. B removes one `push`.
- **One delimited SCSS block.** All ported rules (headroom transforms,
  `body.nav-fixed`, `navbar-default-offset`, `.notransition`, print
  `.fixed-top`) go in a single clearly-fenced "fixed-top header (bd-ersobfbt,
  replaced by bd-pt1wxeq2)" section of `_bootstrap-rules.scss` /
  `_bootstrap-functions.scss`.
- **Centralized parity pins.** The tests that pin `fixed-top` /
  `nav-fixed` / body padding live together (one module per surface) and are
  named so B's author can find and flip them wholesale, the way this strand
  flips bd-26bf3j1y's absence pins.
- **The stage/predicate survives B.** `HeadroomJsStage`'s predicate
  (website + navbar/sidebar, `pinned:` opt-out) is mechanism-independent;
  B only changes *which* files it ships.

## Risks / tradeoffs (draft)

- **The cliff the strand warns about is real and testable.** `fixed-top`
  without `nav-fixed` padding overlaps content. Phase 0's compile test
  must exist before the template change lands (same shape as the
  `test_sidebar_stays_visible_at_lg_despite_collapse_class` guard).
- **Script key ordering.** `ApplyTemplateStage` sorts keys; `js:quarto-nav:*`
  sorts after `js:code-copy-init` and `js:highlight`, before `js:tabsets`.
  Headroom needs nothing but the DOM; quarto-nav.js needs nothing from
  Bootstrap. Safe today; document the choice in the stage as
  `bootstrap_js.rs` does.
- **Preview header wrapper is a parity fix in disguise.** Adding
  `#quarto-header` in preview changes the preview DOM for every website
  page regardless of headroom (the `> nav` padding rule starts matching).
  Snapshot/e2e churn in `hub-client/e2e` is plausible; budget for it.
- **Hub-client outer layout vs `position: fixed` inside the iframe.** The
  iframe is `height: 100%` of the preview pane, so `fixed` is relative to
  the iframe viewport — correct. But the Q2 sandboxed preview
  (`Q2SandboxedPreviewIframe.tsx`) is a different, cross-origin surface;
  out of scope unless it renders website chrome (it does not today).
- **Stage predicate.** `BootstrapJsStage`/`ClipboardJsStage` gate on
  `!is_minimal_html` — every HTML doc, including standalone documents with
  no navbar. Nav JS must gate on "website with navbar or sidebar"
  (`WebsiteBootstrapIconsTransform` in `transforms/website_bootstrap_icons.rs:72-96`
  is the `ProjectKind::Website`-gated precedent that stores artifacts the
  same way).
- **Verbatim `quarto-nav.js` imports dead Q1 code.** Of its 325 lines only
  ~130 are header/offset related; the rest (sidebar rollup, announcement
  bar, `quarto-hrChanged` consumers) target selectors q2 never emits. A
  ~60-line port (`headerOffset` / `updateDocumentOffset` / Headroom init /
  `ResizeObserver` / `hashchange`) plus vendored `headroom.min.js` is the
  honest minimum; `resources/js/README.md` wants a version-contract note
  per file either way.
- **Golden hashes.** `tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt:296-303`
  moved for the wrapper; a class change plus new `<script>` tags move it
  again. `secondary_nav_pipeline.rs:161` prefix-matches and survives.
- **`quarto-hrChanged` / `headroom-target`.** Q1's sidebar-rollup toggle
  (`quarto.js:352-360`, reader mode) is the only `.headroom-target`
  producer and q2 has not ported it. The port can drop that selector or
  keep it inert; either is fine, but note it so the next porter knows.
- **TOC active-item tracking** in Q1 uses a flat 200 px margin, not header
  height; q2's TOC highlighting (if/when JS-driven) should not copy that.
- **Stale rationale in three places** (`pipeline.rs`, `bootstrap_js.rs`,
  `2026-08-17` plan decision 3) will mislead the next reader if Phase 4 is
  skipped.

## Strands filed during investigation

- **bd-2yd37vuk** — preview lacks the `#quarto-header` wrapper that
  bd-26bf3j1y added to the native template — `discovered-from` this strand.
- **bd-pt1wxeq2** — Option B (sticky + `--quarto-header-height`) redesign,
  blocked by this strand. Filed 2026-08-24 when decision 1 resolved.
