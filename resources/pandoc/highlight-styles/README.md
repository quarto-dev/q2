# Pandoc/skylighting highlight styles (`.theme` files)

Quarto 1's built-in syntax-highlight palette catalog, vendored from
`external-sources/quarto-cli/src/resources/pandoc/highlight-styles/`
(quarto-cli `2e6695811`, 2026-07-15, v1.10.15-2). Per the repo's
external-sources policy, compiled code must never read from
`external-sources/`; these local copies are what
`crates/quarto-sass` embeds (see `HIGHLIGHT_STYLES_RESOURCES` in
`crates/quarto-sass/src/resources.rs`).

Each file is KDE-syntax-highlighting theme JSON as consumed by
pandoc/skylighting:

- `text-styles`: ~30 Pandoc token names (`Keyword`, `String`,
  `Comment`, …) → `text-color` / `background-color` / `bold` /
  `italic` / `underline` (plus `selected-text-color`, unused here).
- `editor-colors.BackgroundColor` and/or top-level
  `background-color` / `text-color`: the palette's canvas colors.
- `custom-styles`: per-KDE-language overrides. **Ignored**, matching
  Quarto 1's HTML output (its CSS generator reads only `text-styles`).

Quarto 2 does not emit skylighting classes — its tree-sitter
highlighter has its own `hl-*` class vocabulary — so these files are
**translated at render time** into SCSS layers by
`crates/quarto-sass/src/highlight_theme.rs` using a canonical
capture→token mapping table. See
`claude-notes/plans/2026-08-18-highlight-theme-translator.md`.

Adaptive pairs (`<name>-light.theme` / `<name>-dark.theme`, selected
per theme variant when the user writes the bare `<name>`): a11y,
arrow, atom-one, ayu, breeze, github, gruvbox, monochrome. The
adaptive-name list lives in `ADAPTIVE_HIGHLIGHT_STYLES`
(`crates/quarto-sass/src/config.rs`) and must stay in sync with the
pairs present here.

## Updating

When quarto-cli updates its catalog:

1. `cp external-sources/quarto-cli/src/resources/pandoc/highlight-styles/*.theme resources/pandoc/highlight-styles/`
2. Update the provenance commit above.
3. If a new `<name>-light`/`<name>-dark` pair appears, add `<name>`
   to `ADAPTIVE_HIGHLIGHT_STYLES`.
4. `cargo nextest run -p quarto-sass` (catalog-shape tests will flag
   surprises).
