---
title: "Navbar right-side dropdowns: dropdown-menu-end"
date: 2026-11-05
description: "Add the Bootstrap end-alignment class to dropdown menus in the right navbar section."
---

**Strand:** bd-jdgfgwdu · **Issue:** https://github.com/quarto-dev/q2/issues/1059

## Overview

Currently `render_navbar_item` in `crates/quarto-navigation/src/render_html.rs`
always emits `<ul class="dropdown-menu">`. Quarto 1 emits
`dropdown-menu dropdown-menu-end` for menus in the right section, so the
menu aligns to its toggle's right edge instead of overflowing the page.

New behaviour: items rendered from `navbar.right` get `dropdown-menu-end`;
left items are unchanged.

## Work items

- [x] Failing unit test for right vs. left menus
- [x] Thread an `end` flag through `render_navbar_item`
- [x] Verify (nextest, clippy, Windows clippy, strict note render)
