---
title: "Sidebar section toggles as keyboard-operable buttons"
---

Strand: bd-3bg8h1rx (related to the websites epic, bd-0tr6).

## Overview

A user reported that q2's sidebar section toggles differ from Quarto 1:

```html
<!-- Quarto 1 -->
<button aria-controls="quarto-sidebar-section-3" aria-expanded="true"
        aria-label="Toggle section: Groups" class="sidebar-item-toggle text-start"
        data-bs-target="#quarto-sidebar-section-3" data-bs-toggle="collapse" type="button">
<!-- q2 -->
<a aria-expanded="true" aria-label="Toggle section" class="sidebar-item-toggle text-start"
   data-bs-target="#quarto-sidebar-section-0-2" data-bs-toggle="collapse" role="navigation">
```

An `<a>` without `href` is not focusable, so keyboard users cannot
expand or collapse sections. `role="navigation"` is a landmark role, so
screen readers announce every toggle as a navigation region. The label
does not name the section and is not localized. `aria-controls` is
missing. The theme's button-reset rules have nothing to match.

q2 ported its sidebar markup before Quarto 1 fixed this in quarto-cli
`a8b5b2ba6`, "Make website sidebar section toggles keyboard-operable
buttons" (#14826, released in 1.11, fixes quarto-cli#12116 and #4935).

### Quarto 1 markup after the fix (`sidebaritem.ejs`)

- **Section with an `href`**: the header stays an `<a>` link, and its
  label becomes `<span class="menu-text" id="{sectionId}-label">`. A
  separate `<button type="button" class="sidebar-item-toggle
  text-start[ collapsed]">` sits beside it. The button carries
  `data-bs-toggle`, `data-bs-target`, `aria-controls`,
  `aria-expanded`, and `aria-label="{toggle-section}: {plainText}"`.
  Its `<i class="bi bi-chevron-right ms-2">` has `aria-hidden="true"`.
- **Section without an `href`**: one `<button type="button"
  class="sidebar-item-text sidebar-item-toggle sidebar-link
  text-start[ collapsed]">` holds both the label span (with the
  `-label` id) and the chevron (`aria-hidden`). That gives the section
  a single tab stop. This button has no `aria-label`, because the
  visible text names it.
- `<ul id="{sectionId}" aria-labelledby="{sectionId}-label">`.
- In `quarto-nav.scss`, the toggle buttons lose their native button
  styling, `button.sidebar-item-text` lays out as a flex row with
  baseline alignment, and the chevron is pushed to the right edge.

The same commit removed `role="navigation"` (and the duplicate
`role="link"`, `aria-controls`, `aria-expanded`, and `aria-label`) from
the mobile secondary nav's "click anywhere" `<a class="flex-grow-1">`.
The real `<button class="quarto-btn-toggle">` beside it keeps those
attributes.

### q2 code

- `crates/quarto-navigation/src/render_html.rs`:
  - `render_sidebar_section` emits the two `<a role="navigation">`
    elements, with the label hard-coded to `"Toggle section"`.
  - `secondary_nav_to_html` shares one set of `collapse_attrs`
    (including `aria-*`) between the button and the flex-grow link,
    and adds `role="navigation"` to the link.
- `resources/scss/bootstrap/_bootstrap-rules.scss` (~576): the
  chevron-rotation rules are present (bd-rl7r38pw), but the
  button-reset and layout rules are not.
- `crates/quarto-core/src/transforms/sidebar_render.rs` builds
  `SidebarRenderOptions`. The localized `toggle-section` term is already
  in the language table (`LanguageTerms`); this change threads it
  through, the same way `search-label` is threaded.

q2's section ids are path-based (`quarto-sidebar-section-0-2`) rather
than counter-based. That difference is intentional and is kept.

## Checklist

### Phase 1: tests (must fail first)

- [x] `render_html.rs` unit tests:
  - [x] A section with an `href` renders the header `<a>` with
    `<span class="menu-text" id="{id}-label">`. It also renders a
    `<button type="button" class="sidebar-item-toggle text-start">` with
    `aria-controls`, `aria-expanded`, and
    `aria-label="Toggle section: {text}"`, and its chevron has
    `aria-hidden`.
  - [x] A section without an `href` renders exactly one `<button
    class="sidebar-item-text sidebar-item-toggle sidebar-link
    text-start">` containing the label span and the chevron, and no
    separate chevron control.
  - [x] The `<ul>` has `aria-labelledby="{id}-label"`.
  - [x] No sidebar output contains `role="navigation"`, and no `<a>`
    carries `data-bs-toggle`.
  - [x] Formatted section text (inlines, for example emphasis) gives a
    plain-text `aria-label` with no markup.
  - [x] A localized `toggle_section_label` passed in through the
    options is used.
  - [x] `secondary_nav_to_html`: the flex-grow link has no `role`,
    `aria-controls`, `aria-expanded`, or `aria-label`, and the button
    keeps all of them.
- [x] quarto-sass: a compiled-CSS test that the button reset and the
  `button.sidebar-item-text` flex layout are present.
- [x] quarto-core integration (`sidebar_pipeline.rs`): a website render
  produces button toggles with the localized label, through the real
  `LanguageTerms` path.
- [x] A smoke-all fixture `crates/quarto/tests/smoke-all/navigation/sidebar-disclosure/`,
  ported from Quarto 1's `tests/docs/smoke-all/website/sidebar-disclosure`
  with ids adjusted to q2's path-based scheme.
- [x] Confirm the new tests fail on the unfixed code.

### Phase 2: fix

- [x] Add `toggle_section_label: &'a str` to `SidebarRenderOptions`,
  with a public English default constant. Thread it through
  `render_sidebar_entry` and `render_sidebar_section`.
- [x] Rewrite `render_sidebar_section` to match Quarto 1's markup (see
  above).
- [x] Remove `role="navigation"` and the duplicated `aria-*` from the
  secondary-nav flex-grow link, and update the doc comment that
  explains the old choice.
- [x] Port the SCSS block from `quarto-nav.scss` into
  `_bootstrap-rules.scss`.
- [x] Pass the localized `toggle-section` from `sidebar_render.rs`
  (both call sites).
- [x] Fix any existing assertions that pinned the old markup.

### Phase 3: verification

- [x] `cargo nextest run -p quarto-navigation -p quarto-sass -p quarto-core`,
  plus clippy with `-D warnings`.
- [x] End-to-end: render the smoke fixture with `cargo run --bin q2`
  and inspect the HTML. Check keyboard focus in a browser if one is
  available.
- [x] `cargo xtask verify --skip-hub-build`. Rust: 19,180 of 19,182
  tests pass with `--no-fail-fast`. The two failures are
  `capture_classifies_pieces` (local R install) and
  `quarto-preview` `head_peek_timeout_closes`, a timing test that
  passes on rerun. The remaining steps pass under
  `--skip-rust-tests`. Hub-client `test:wasm` first failed on
  unrelated fixtures because the WASM bundle was stale; after
  `npm run build:wasm`, all hub-client suites pass.
- [x] Open the PR and watch CI until it is green (PR #1121). Every check passed on the first run, including Clippy (Windows).

## Details

### Verification record

- The new unit, sass, pipeline, and smoke tests failed on the unfixed
  code (the unit tests did not compile, because the
  `toggle_section_label` option did not exist yet) and pass with the
  fix. `cargo nextest run -p quarto-navigation -p quarto-sass
  -p quarto-core` is green apart from
  `knitr::capture_tests::capture_classifies_pieces`. That test fails
  locally because of the local R install (`rlang_error` vs
  `simpleError`) and is unrelated.
- The Phase 5 byte-identity baseline (`phase5-single-doc-baseline`)
  changed for `styles.css` only. Rendering with the new SCSS hunk
  removed reproduces the old hash, so the hunk is the only delta. The
  baseline was re-captured with a provenance note.
- Browser check (chrome-devtools) on the rendered smoke fixture:
  - The toggles have no native button styling, and the chevrons sit at
    the right edge, as before.
  - Tab from "Index" lands on the "Group" button, and Enter expands it
    (`aria-expanded="true"`, `ul.show`).
  - The accessibility tree lists `button "Group" expandable` and
    `button "Toggle section: Linked" expandable expanded`, with no
    per-toggle navigation landmarks.
- The smoke fixture needs `_quarto.render-project: true`. A per-file
  smoke render has no project index, so no page is marked active and
  hrefs stay unresolved.
- The Windows clippy cross-check cannot run from macOS (`aws-lc-sys`
  compiles C for the target). The change adds no platform-specific
  code.

## Follow-ups (filed, out of scope here)

Other Quarto 1.11 accessibility changes that q2 lacks:

- bd-erecx4j8: distinct, localizable `aria-label` on every `nav`
  landmark (quarto-cli #14813).
- bd-kwgtllvn: the navbar dark-mode toggle becomes a `<button
  role="switch">` (#14739).
- bd-59go2q2g: navbar search after the nav links, so tab order matches
  visual order (#14754).
- bd-4iuoquu8: one consistent focus ring (#15163).

Already at parity: the announcement dismiss `<button>` (#15253) and no
`role="menu"` on the navbar toggler (#14805).
