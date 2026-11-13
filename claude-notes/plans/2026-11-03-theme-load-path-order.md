---
title: 'quarto-sass: a theme partial wins over a same-named Bootstrap partial'
date: 2026-11-03
description: 'A custom theme that imports its own `_mixins.scss` got Bootstrap''s instead; theme directories now come first and embedded resources no longer answer relative lookups.'
---

**Strand:** bd-r8lodg6o · **Issue:** <https://github.com/quarto-dev/q2/issues/1032>

## Overview

A custom theme doing `@import "mixins"` resolved to Bootstrap's
`_mixins.scss`, not the `_mixins.scss` next to the theme. The import
succeeded, and the render failed later with `Undefined mixin`.

Two things caused it, and both had to change:

1. `crates/quarto-sass/src/compile.rs` built the load paths as
   Bootstrap defaults first, then the theme directories. Dart Sass and
   grass take the first match. A new `theme_load_paths` helper puts the
   theme directories first, then the context's paths, then the defaults.
   This is the Quarto 1 order. All three compile entry points use it.
2. Before it searches the load paths, grass tries the import relative to
   the importing file. For the `stdin` bundle that is a bare `mixins`.
   `RuntimeFs` in `crates/quarto-system-runtime/src/sass_native.rs`
   asked the embedded provider first, and that provider also answers
   bare relative names, so Bootstrap won before any load path was read.
   `RuntimeFs` now consults embedded resources only for rooted paths.
   Every embedded load path is under `/__quarto_resources__`.

## Work items

- [x] Fixture `crates/quarto-sass/test-fixtures/shadowed-partial/` and
      the failing test `test_compile_theme_css_theme_partial_shadows_bootstrap_partial`
- [x] `theme_load_paths` helper in `compile.rs`
- [x] Rooted-path guard in `RuntimeFs` and the unit test
      `embedded_resources_do_not_answer_relative_lookups`
- [x] Nextest and clippy on both crates; the issue's reproducer with `q2 render`
