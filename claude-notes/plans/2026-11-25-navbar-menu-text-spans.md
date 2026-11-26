---
title: 'navigation: wrap navbar link text in menu-text spans'
date: 2026-11-25
description: 'Navbar links and dropdown toggles wrote their text bare, unlike Quarto 1 and the q2 sidebar. Wrap it in a menu-text span, and wrap dropdown entries in a dropdown-text span.'
status: in-progress
---

**Braid:** bd-6vi83y03

**Issue:** <https://github.com/quarto-dev/q2/issues/1372>

## Overview

Quarto 1 writes each navbar link's text (and a dropdown toggle's) inside
`<span class="menu-text">`, after any icon, and each dropdown entry's text
inside `<span class="dropdown-text">`. q2 wrote the text bare unless the
item had responsive text variants, and then used `menu-text` for dropdown
entries too. Site scripts (e.g. Great Docs) find navbar labels through
`.menu-text`.

In `crates/quarto-navigation/src/render_html.rs`, `navbar_text` now takes
the span class and always wraps the text, for scalar text and for a single
merged responsive run. `render_navbar_label` passes the class through:
`menu-text` for links and toggles, `dropdown-text` from
`render_dropdown_item`. Icon-only items still get no span.

## Work items

- [x] `navbar_text` / `render_navbar_label` take the span class
- [x] `render_dropdown_item` uses `dropdown-text`
- [x] Unit tests: scalar link, icon plus text, icon only, toggle and dropdown entry
- [x] Update the responsive dropdown test to `dropdown-text`
- [x] Run quarto-navigation and quarto-core tests
