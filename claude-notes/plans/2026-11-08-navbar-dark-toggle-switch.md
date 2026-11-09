---
title: 'Color-scheme toggle as an accessible switch (bd-kwgtllvn)'
date: 2026-11-08
description: 'Port quarto-cli #14739: the navbar and floating dark-mode toggles become a localized `<button role="switch">` whose `aria-checked` tracks the mode.'
status: active
braid:
  strand: bd-kwgtllvn
---

**Checkout:** main checkout, branch
`braid/bd-kwgtllvn-color-scheme-toggle-accessible` (made with
`switch-task` off `main` @ `8f17e950d`). The design was aligned with the
user on 2026-11-09; see Decisions.

## Triage verdict

**Ready to design.** The defect reproduces at HEAD, the Quarto 1 fix
(quarto-cli `fa48b15ea`) is small and well understood, and q2 already
has the dashboard half of it. One bookkeeping decision comes first: an
older strand, bd-ix7psky0, covers the same Quarto 1 commit (question 1).

## Issue context

bd-kwgtllvn (P2 bug, `a11y`) was filed 2026-11-07 during bd-3bg8h1rx,
as one of four follow-ups for Quarto 1.11 accessibility changes that q2
lacks. It asks to make the navbar dark-mode toggle a `<button
role="switch">`, localize its label, and keep `aria-checked` in sync.

Quarto 1's change (#14739, which fixes quarto-cli#13463) does the
following:

- The toggle becomes `<button type="button" role="switch"
  aria-checked>` with a localized `aria-label`. This applies to all
  three places a toggle is created: the website navbar and sidebar
  (`navdarktoggle.ejs`), plain documents with a light/dark theme pair
  (the floating fallback in `quarto-html-after-body.ejs`), and
  multi-page dashboards.
- `setColorSchemeToggle()` also sets `aria-checked`, on load and on
  every toggle.
- The floating fallback is placed in a landmark of its own: a fresh
  `<header>` when the page has no banner, otherwise a `<div
  role="region" aria-label=…>`. Banner detection handles headers nested
  inside a non-sectioning wrapper and headers inside sectioning content.
- SCSS: a `button.quarto-color-scheme-toggle` reset.

## Dependency graph

- **discovered-from** bd-3bg8h1rx (closed, PR #1121): the sidebar
  section toggle fix. A survey of the Quarto 1 accessibility commits
  near quarto-cli #14826 surfaced this one.
- **related** bd-0tr6: the websites epic, an umbrella only.
- **Not linked, but overlapping:**
  - **bd-ix7psky0** ("HTML: make the dark/light mode toggle an
    accessible switch (Quarto 1 #14739 parity)") has been P3 and
    `in_progress` since 2026-10-21. It was filed by the vendored resync
    (bd-jsfg1494). No branch, plan, or commits reference it, so it was
    filed and parked, and the dashboard port then did part of the work
    independently. Same Quarto 1 commit, so this is a duplicate.
  - bd-ld-toggle-into-tools-hpae7m9r and bd-fod3: these fold the
    hard-coded navbar toggle into general `tools:` support. This change
    must keep the `quarto-color-scheme-toggle` markup contract they
    rely on.
  - bd-0pic6 (light/dark epic): owns the runtime
    (`quarto-color-mode.js`, from bd-ld-a4).

## What the code looks like today

The full repro and notes are in
`navbar-dark-toggle-switch-investigation/findings.md`, with fixtures in
`repro-site/` and `repro-plain/`.

| Creation path | q2 location | State |
|---|---|---|
| Website navbar | `crates/quarto-navigation/src/render_html.rs` (`navbar.dark_mode_toggle`) | `<a href="">`, hard-coded English `title`, no `aria-checked`. **Needs the fix.** |
| Floating fallback (no navbar) | `crates/quarto-core/resources/js/quarto-color-mode.js`, `DOMContentLoaded` | Creates an `<a>`, hard-coded English title, appended to `<body>` with no landmark. **Needs the fix.** |
| Dashboard navbar | `crates/quarto-core/src/template.rs` (~507) | Already a localized `<button role="switch">`, but `aria-checked` is stuck at `"false"`. |
| Sidebar `tools:` | Not rendered (bd-fod3) | Not applicable. |
| Runtime state sync | `setColorSchemeToggle()` in `quarto-color-mode.js` | Toggles `.alternate` only. **Needs the fix**, for all three paths. |
| SCSS reset | `_bootstrap-rules.scss` (`button.quarto-color-scheme-toggle`) | Already ported. Its comment is stale. |

The runtime is configured through `data-*` attributes on its inline
`<script id="quarto-color-mode">` (`template.rs`, `color-mode-script`),
so a localized fallback label can travel the same way. The navbar
already threads a localized `search_label` from `navbar_generate.rs`
into the renderer, which is a ready pattern for the toggle label.

No JS behavior tests exist for `quarto-color-mode.js`. Its integration
tests (`quarto-core/tests/integration/theme_light_dark.rs`) assert
markup only. `ts-packages/preview-renderer` runs real resource scripts
in JSDOM (for example `quarto-nav-sidebar-scroll.integration.test.ts`),
which is a workable harness.

## Decisions (2026-11-09)

1. Keep **bd-kwgtllvn**, retitled "Color-scheme toggle as an
   accessible switch". bd-ix7psky0 is closed as its duplicate.
2. Port Quarto 1's **full** landmark logic for the floating fallback: a
   `<header>`, or a named `region` when the page already has a banner,
   using Quarto 1's banner detection.
3. Test the runtime with **JSDOM** in `ts-packages/preview-renderer`,
   which reads `crates/quarto-core/resources/js/quarto-color-mode.js`.
4. The navbar button keeps **both** `title` and `aria-label`.
5. No dashboard-specific test. The shared runtime test of
   `aria-checked` is enough.
6. Work on a topic branch made with `switch-task`.

## Checklist

### Phase 0: tests first (must fail on HEAD)

- [x] `quarto-navigation` unit tests: the navbar toggle is `<button
  type="button" role="switch" aria-checked="false">` with `title` and
  `aria-label` from a threaded, escaped label, and is not an `<a>`.
- [x] `quarto-core` pipeline test: a website with a light/dark pair
  and `lang:` set renders the localized label on the navbar toggle.
- [x] `quarto-core`: the `color-mode-script` tag carries the localized
  `data-toggle-label`.
- [x] JSDOM tests (`preview-renderer`) running the real runtime:
  - [x] `aria-checked` matches the mode on load (light default, dark
    default, stored preference) and flips on
    `quartoToggleColorScheme()`. Covers every toggle, the dashboard
    one included.
  - [x] Fallback: a `<button type="button" role="switch">` with the
    label from `data-toggle-label` (falling back to English), and no
    fallback when a toggle already exists.
  - [x] Landmark: a `header.quarto-color-scheme-toggle-container` when
    the page has no banner; a `div[role=region][aria-label]` when there
    is a body-level header, a header nested in a plain wrapper, or an
    explicit `role="banner"`; a `<header>` when the only header sits
    inside sectioning content (`main`, `article`, …).
- [x] A smoke-all fixture for the static navbar markup.

### Phase 1: navbar

- [x] Add a `dark_toggle_label` to `Navbar`, round-tripped as the
  internal `dark-toggle-label` key and filled in `navbar_generate.rs`
  from `crate::language::toggle_dark_mode_label`. That helper is shared
  with `template.rs`.
- [x] Emit the button markup in `navbar_to_html`.

### Phase 2: runtime

- [x] Make `setColorSchemeToggle()` set `aria-checked`.
- [x] Rewrite the fallback: button, localized label, landmark
  container.
- [x] `template.rs`: add `data-toggle-label` to the script tag.

### Phase 3: cleanup

- [x] Update the stale SCSS comment. Grep for
  `a.quarto-color-scheme-toggle` selectors (none found).
- [x] **Added in scope:** port Quarto 1's `.top-right` utility
  (`_quarto-rules.scss:3-7`). The browser check showed the fallback
  rendering at the bottom-left of the page. Catalog strand bd-q36vnfdp
  had held the utility back because nothing emitted it, but the
  runtime has emitted it since bd-ld-a4, so every plain dark-themed
  page has misplaced the toggle since then. Covered by a compiled-CSS
  test, with the Phase 5 `styles.css` baseline re-captured.

### Phase 4: verification

- [x] Run nextest and clippy on the touched crates, plus the
  `preview-renderer` integration tests.
- [x] Browser check: keyboard activation and the accessibility tree
  (a switch with its state; the fallback inside a landmark).
- [x] `cargo xtask verify --skip-hub-build`.

### Verification record

- Rust, with `--no-fail-fast`:
  - The only code-independent failure is
    `knitr::capture_classifies_pieces` (local R install).
  - `/usr/local/bin/pandoc` is 3.13, outside the oracle range
    (3.6–3.12), and about 30 pandoc-oracle tests fail under it. With
    Homebrew pandoc 3.12.1 first on `PATH` they pass.
  - Homebrew pandoc has no HTTP support, so three remote-image
    smoke-all fixtures (`url-image-mediabag`, `online-logo`,
    `crossref-grand-finale`) fail under it. `url-image-mediabag` passes
    with 3.13. All other smoke-all fixtures pass, including
    `navigation/dark-toggle-switch`.
- `cargo xtask verify --skip-hub-build --skip-rust-tests` passes. That
  covers the hub-client WASM suite on a rebuilt bundle and the
  `preview-renderer` typecheck. The 13 new JSDOM tests pass.
- Browser check (chrome-devtools) on the repro fixtures:
  - Navbar: Space on the focused toggle switches to dark mode, and the
    accessibility tree shows `switch "Toggle dark mode" checked`.
  - Plain document: the fallback sits top-right (17px from the top and
    right edges, after the `.top-right` port) in the page's only
    `banner`. Here the title-block header is inside `main`, so it is
    not a banner. Enter toggles it, and it reports `checked`.

## Risks / tradeoffs (draft)

- **User CSS** that targets `a.quarto-color-scheme-toggle` stops
  matching. Quarto 1 accepted this and noted it in its changelog.
  q2 has no changelog convention for parity breaks; this could go in
  the PR description.
- **Banner detection** depends on the final DOM. The q2 templates (the
  title-block header, custom layouts) should be checked against Quarto
  1's selector logic, so that we never emit a second banner.
- **Hub preview** does not run this runtime, so nothing changes there.
  Preview theming stays with bd-0pic6.
