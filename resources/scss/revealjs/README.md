# RevealJS SCSS resources

Local SCSS sources for Quarto 2's reveal.js theming, embedded into the
`quarto-sass` crate at compile time (`include_dir!`) and compiled per-deck
through the same layered-SCSS subsystem (`grass` natively, dart-sass on WASM)
that drives `format: html`.

This directory exists because of the **External Sources Policy** (see root
`CLAUDE.md`): nothing compiled or embedded may reference `external-sources/`.
The reveal-template files below are therefore vendored copies.

## Layout

- `reveal-template/` — the reveal.js 6 base theme machinery, vendored from
  reveal.js `css/theme/template/`:
  - `_settings-vars.scss` — the `$kebab: … !default;` variable declarations from
    upstream `settings.scss`. The reveal-native fallbacks (`#bbb`, `uppercase`,
    `Lato`, …).
  - `_expose.scss` — the trailing `:root { --r-*: … }` block from upstream
    `settings.scss`, split out so it can run in the SCSS `rules` layer (after all
    `!default`s collapse) and thus carry Quarto's overridden values.
  - `_theme.scss` — upstream `theme.scss` verbatim (the `var(--r-*)` rule set).
  - `_mixins.scss` — upstream `mixins.scss` verbatim (`light/dark-bg-text-color`).
- `quarto-revealjs.scss` — **Quarto's** reveal layer (the analogue of Quarto 1's
  `quarto.scss`): the `$presentation-*` / `$body-*` user-facing vocabulary, the
  mapping from that vocabulary to reveal-6 kebab variables, and the rule
  overrides that make a deck "feel at home" for Quarto users (left-aligned
  slides, non-uppercase headings, Quarto title-slide layout, …).

## Why the split

reveal.js 6 themes are authored with `@use 'template/settings' with (...)`, which
needs configuration values at the `@use` call site — that fights Quarto's
layered-`!default` merge model (where a higher-priority layer wins by being
emitted first). So instead of `@use`-ing reveal's `settings.scss`, we split its
two responsibilities (declare vars / emit `:root`) across the `defaults` and
`rules` layers and let Quarto's layer override the vars by ordinary `!default`
precedence. See `crates/quarto-sass/src/bundle.rs` (`load_reveal_framework`).

## Provenance / update procedure

`reveal-template/` is copied from **reveal.js 6.0.x** (`css/theme/template/`).
The vendored reveal.js runtime assets (`reset.css`, `reveal.css`, `reveal.js`,
`theme/white.css`) live separately under `resources/revealjs/` and are version-
pinned there; see that directory's README. When bumping reveal.js:

1. Re-copy `settings.scss` → split into `_settings-vars.scss` (declarations) and
   `_expose.scss` (the `:root` block); re-copy `theme.scss` → `_theme.scss` and
   `mixins.scss` → `_mixins.scss`.
2. Diff the variable set; if reveal renamed/added/removed a `--r-*` or a
   `$kebab` variable, update the mapping in `quarto-revealjs.scss` accordingly.
3. Run `cargo nextest run -p quarto-sass` and re-verify a rendered deck.
