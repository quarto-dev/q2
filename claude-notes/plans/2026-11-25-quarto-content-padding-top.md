---
title: 'scss: port the quarto-content top padding from Quarto 1'
date: 2026-11-25
description: 'Website pages started right under the navbar because the Q1 rule that pads every child of quarto-content by 14px was never ported. Port it, with its narrow-screen overrides.'
status: in-progress
---

**Braid:** bd-twokvvk4 · **Issue:** <https://github.com/quarto-dev/q2/issues/1376>

## Overview

Quarto 1's `quarto-nav.scss` pads every grid child of `#quarto-content`
(`padding-top: $content-padding-top`, 14px), exempts the sidebar glass, and
below the `lg` breakpoint moves the gap to the subtitle and the first
section `h2`. q2's `resources/scss/bootstrap/_bootstrap-rules.scss` only
had the glass exemption, so page title, sidebar and TOC sat flush under the
navbar.

New behaviour: the block is ported next to the glass exemption, with
`$content-padding-top: 14px !default` in `_bootstrap-variables.scss`. The
docs landing page's hero rule (`docs/theme/partials/_home.scss`) keeps its
own padding, because the custom-layout rule zeroes `main.content`'s padding
with higher specificity, so the padding is applied once; only its comment
changes.

## Work items

- [x] Test in `crates/quarto-sass/src/compile.rs`
  (`test_compile_default_css_includes_content_padding_top`), failing first
- [x] Port the rules and the variable
- [x] Update the hero comment in `_home.scss`
- [x] Verify (quarto-sass tests, clippy, xtask verify, lint, notes render)
