---
title: "Brand layers name the theme `brand` (bd-gdrpbf9t, GH #1187)"
date: 2026-11-12
status: done  # Merged in PR #1189 (b3db66ffc), 2026-11-12
description: "brand_to_layers emits a theme-name layer so a Bootswatch theme's theme-override-value entries stop overriding brand colours."
---

Strand: bd-gdrpbf9t · Issue: <https://github.com/quarto-dev/q2/issues/1187>

## Overview

**Current behaviour.** With `theme: [cosmo, brand]` on a dashboard, value
boxes keep cosmo's colours (`.bg-success` is `#3aa716`) instead of the
brand's `success`. The dashboard SCSS picks each value-box colour through
`theme-override-value($theme-name, …)`
(`resources/scss/bootstrap/_bootstrap-functions.scss`), and cosmo has
entries in that table. `brand_to_layers`
(`crates/quarto-sass/src/brand_layer.rs`) never sets `$theme`, so the
name stays `cosmo`.

**New behaviour.** Like Q1's `brandSassLayers`, `brand_to_layers` always
emits a layer whose defaults are `$theme: "brand" !default;`, after the
`defaults.bootstrap` layer and before the color and typography layers.
Defaults merge in reverse, so this layer's `$theme` wins over the
Bootswatch theme's, `brand` has no override-table entry, and the value
boxes fall back to `$success` and the other brand colours.

## Work items

- [x] Failing test: `test_dashboard_value_boxes_use_brand_colors_over_bootswatch`
      in `crates/quarto-sass/src/compile.rs`
- [x] Emit the theme-name layer in `brand_to_layers`
- [x] Update layer-position tests in
      `crates/quarto-sass/tests/integration/brand_layer_test.rs`; add
      `theme_name_layer_follows_bootstrap_defaults`
- [x] Verify (nextest quarto-sass + quarto-core, clippy, Windows cross
      clippy, `cargo xtask lint`, notes render)
