---
title: "Sidebar: rotate the chevron of expanded sections"
---

Strand: bd-rl7r38pw.

## Overview

Q1's website theme rotates the disclosure chevron of an expanded sidebar
section, so readers can tell open sections from closed ones:

```css
.sidebar-item .sidebar-item-toggle[aria-expanded=true] .bi-chevron-right::before {
  transform: rotate(90deg)
}
```

q2 emits the same markup (`crates/quarto-navigation/src/render_html.rs`,
the `a.sidebar-item-toggle[aria-expanded]` containing
`i.bi.bi-chevron-right`). Bootstrap's collapse plugin keeps
`aria-expanded` on the trigger in sync when a section is toggled. But the
sidebar block of `resources/scss/bootstrap/_bootstrap-rules.scss`, a port
of Q1's `projects/website/navigation/quarto-nav.scss`, skipped the
chevron rules, so every section shows a right-pointing chevron.

Missing from the port (Q1 `quarto-nav.scss`):

- 412-416: `.sidebar-item .sidebar-item-toggle .bi { font-size: 0.7rem; text-align: center }`
- 418-420: `transition: transform 200ms ease` on the chevron `::before`
- 422-432: `aria-expanded="false"` gives `transform: none`, `"true"` gives `rotate(90deg)`
- 756-762: `.sidebar-section .bi-chevron-right` sizing (`vertical-align: middle`, `::before` at `0.9em`)

Out of scope:

- Q1's `<button>` reset rules (383-410). Q1 1.11 changed the toggle to a
  `<button>`, but q2 still emits `<a>`, so those rules have nothing to
  style.
- Q1's `.quarto-secondary-nav … .bi-chevron-right` rotation (515-528).
  q2's secondary-nav toggle uses `bi-layout-text-sidebar-reverse`, not a
  chevron.

## Checklist

- [x] Test first: a compiled-CSS test in `crates/quarto-sass/src/compile.rs`
  (next to `test_sidebar_stays_visible_at_lg_despite_collapse_class`) that
  asserts the default bundle has a rule whose selector targets
  `.sidebar-item-toggle[aria-expanded="true"] .bi-chevron-right::before`
  with `transform: rotate(90deg)`. Confirm that it fails.
- [x] Port the rules into the sidebar block of `_bootstrap-rules.scss`,
  with a provenance comment.
- [x] Test passes; `cargo clippy -p quarto-sass --all-targets -- -D warnings`.
- [x] End-to-end: render a website with nested sidebar sections; check the
  compiled CSS and confirm in a headless browser that the expanded
  section's chevron `::before` computes to a rotated transform and the
  collapsed one does not.
- [x] `cargo xtask verify --skip-hub-build` (local failures are environment-only: pandoc 3.13 vs pinned 3.12.1, local R/cairo; the one real failure, the `phase5-single-doc-baseline` styles.css hash, was re-captured)
- [x] PR #1119; CI green (15/15).

## End-to-end verification (2026-11-07)

Setup: a three-page website with `sidebar.collapse-level: 1` and two
sections, rendered with `cargo run --bin q2 -- render <site>`. Inspected
the output:

- `a.html`: the toggle for the section holding the current page has
  `aria-expanded="true"`; the other section has
  `aria-expanded="false"` plus `collapsed`.
- The compiled `site_libs/quarto/quarto-theme-*.css` contains
  `.sidebar-item .sidebar-item-toggle[aria-expanded=true] .bi-chevron-right::before{transform:rotate(90deg)}`
  and the `=false` / `transform:none` counterpart.
- In headless Chrome, served over HTTP: the open section's chevron
  `::before` computes to `matrix(0, 1, -1, 0, 0, 0)` (90 degrees) and
  the closed one to `none`. Clicking the closed toggle makes Bootstrap
  flip `aria-expanded` to `true`, and that chevron then rotates too. A
  screenshot showed both chevrons pointing down after the click.
