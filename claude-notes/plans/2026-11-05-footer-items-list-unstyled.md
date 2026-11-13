---
title: 'page-footer: footer item list uses list-unstyled, not nav'
date: 2026-11-05
description: 'Stops page-footer region items from wrapping onto separate lines by emitting the Quarto 1 class list on the footer item list.'
braid:
  strand: bd-kb29hdle
---

**Strand:** bd-kb29hdle · **Issue:** <https://github.com/quarto-dev/q2/issues/1066>

## Overview

Current behaviour: `render_footer_region` in
`crates/quarto-navigation/src/render_html.rs` emits each item region as
`<ul class="nav footer-items">`. Bootstrap's `.nav` sets
`flex-wrap: wrap`, so a region with a logo and a copyright line renders
on two lines.

New behaviour: emit `<ul class="footer-items list-unstyled">`, the class
list in Quarto 1's `nav-footer-section.ejs`. The footer SCSS selects on
`.footer-items` only, so it still matches. `.nav` was added only so that
those selectors would match (see `2026-04-18-navbar-footer-design.md`), and
`.footer-items` alone is enough.

## Work items

- [x] Update the unit test `footer_items_render_as_nav_list` to expect the
      Quarto 1 class list (fails before the fix).
- [x] Change the class string in `render_footer_region`.
- [x] Run nextest, clippy (host and Windows cross) for `quarto-navigation`,
      and nextest for `quarto-core` (pipeline tests read the footer HTML).
