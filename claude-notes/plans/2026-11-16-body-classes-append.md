---
title: "Page body-classes add to the computed body classes"
date: 2026-11-16
description: "GH #1218: a page's body-classes joins the computed layout classes on the body element, as in Quarto 1, instead of replacing them."
---

Strand: bd-79oucia9. Issue: <https://github.com/quarto-dev/q2/issues/1218>.

## Overview

**Before.** A top-level `body-classes` (from metadata or a filter) was
written on `<body>` wholesale, and the compose in
`render_with_compiled_template` (`crates/quarto-core/src/template.rs`)
was skipped. The page lost `fullcontent`, `nav-fixed` and
`quarto-light`, which the theme's CSS and JavaScript select on. The
preview mirrors did the same, except that they still appended
`quarto-light`.

**After.** The page's classes join the computed list, matching Quarto 1
(whose template writes `body-classes` and whose postprocessors
`classList.add` the layout classes on top). Order: the sidebar's
structural base (`rendered.navigation.body-classes`), then the page's
`body-classes`, then `fullcontent`, `nav-fixed`, and the color-mode
class. A class that appears twice is emitted once. Quarto 1 has no way
to replace the computed classes, so we don't add an opt-in for that.

Class order differs from Quarto 1 (`nav-fixed my-page fullcontent
quarto-light` there), but order has no effect on CSS selectors.

Preview mirrors: `PreviewDocument.tsx` (new `composeBodyClasses`
helper) and `custom/dashboard/DashboardDocument.tsx`.

## Work items

- [x] Rust compose: append user `body-classes`, dedupe; update doc comments
- [x] Rust tests: replace the "wins wholesale" test; add sidebar + dedupe test
- [x] Preview mirrors and their vitest expectations
- [x] Smoke fixture `q2-preview/body-classes-override.qmd` asserted the old replace behaviour; updated
- [x] Follow-up filed: bd-mlfmu9pk (`docs/` landing page workaround for the old behaviour)
- [x] Verify (nextest, clippy, Windows cross clippy, vitest, tsc, eslint, xtask lint, notes render)
