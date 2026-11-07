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

- [ ] Test first: a compiled-CSS test in `crates/quarto-sass/src/compile.rs`
  (next to `test_sidebar_stays_visible_at_lg_despite_collapse_class`) that
  asserts the default bundle has a rule whose selector targets
  `.sidebar-item-toggle[aria-expanded="true"] .bi-chevron-right::before`
  with `transform: rotate(90deg)`. Confirm that it fails.
- [ ] Port the rules into the sidebar block of `_bootstrap-rules.scss`,
  with a provenance comment.
- [ ] Test passes; `cargo clippy -p quarto-sass --all-targets -- -D warnings`.
- [ ] End-to-end: render a website with nested sidebar sections; check the
  compiled CSS and confirm in a headless browser that the expanded
  section's chevron `::before` computes to a rotated transform and the
  collapsed one does not.
- [ ] `cargo xtask verify --skip-hub-build`; PR; monitor CI.
