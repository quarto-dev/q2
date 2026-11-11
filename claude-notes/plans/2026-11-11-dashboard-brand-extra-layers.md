---
title: 'Dashboard themes: brand layers a second time after the theme (GH #1132)'
date: 2026-11-11
description: 'Match Quarto 1 so a dashboard theme file listed after the brand entry can use brand palette variables.'
status: in-progress
braid:
  strand: bd-yawzm5ba
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1132>
**Strand:** bd-yawzm5ba

## Overview

**Current behaviour.** With `format: dashboard` and
`theme: [cosmo, brand, custom.scss]`, a `custom.scss` default such as
`$navbar-bg: $brand-navy;` fails with `Undefined variable`. User layers
are merged with their defaults reversed, so the brand layers' defaults
land *after* `custom.scss`\'s.

**Quarto 1.** `format-dashboard.ts` (lines 191–194) pushes the
`_brand.yml` SCSS bundle once more, after the HTML theme bundle. With
defaults reversed, that extra copy's `$brand-*` defaults come first, so
every theme file can read them. Plain `html` has no extra bundle, and
there Quarto 1 and q2 already agree (the same theme fails in both).

**New behaviour.** In `crates/quarto-sass/src/compile.rs`, when
`config.dashboard_layer` is on and the context carries a brand, append
`brand_to_layers(brand)` after the theme layers (before the doc-vars
layer) in every themed assembly path: `assemble_theme_scss` and both the
native and WASM `compile_with_doc_vars`.

The cache key needs no change: whenever a brand resolves, the `brand`
token is in `theme:` (auto-injected if absent), and the token already
hashes the brand.

## Work items

- [x] Failing test: dashboard + `[cosmo, brand, custom.scss]` using `$brand-*`
- [x] Shared helper appending the brand layers for dashboards; call it at the three sites
- [x] Test that plain HTML is unchanged (still fails, as in Quarto 1)
- [x] Verify: nextest, clippy (host + Windows cross), xtask lint, notes render
- [ ] PR
