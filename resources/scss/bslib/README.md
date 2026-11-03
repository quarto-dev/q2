# bslib component SCSS and htmltools fill CSS

Vendored from quarto-cli, which vendors them from the R packages bslib
(0.5.1 + consolidated components) and htmltools (0.5.7).

- quarto-cli tag: `v1.11.6`
- `bslib-scss/` ← `src/resources/formats/html/bslib/bslib-scss/`
- `components/scss/` ← `src/resources/formats/html/bslib/components/scss/`
- `htmltools/fill.css` ← `src/resources/formats/html/htmltools/fill.css`

Byte-identical copies. Quarto 1 compiles them into every Bootstrap page
(`format-html-scss.ts`, `bslibComponentRules` / `htmlToolsRules`); Q2
compiles them into dashboard pages only (dashboard plan D4,
`claude-notes/plans/2026-11-03-dashboard-format-port.md`). Their `@import`s
(`spacer`, `tab-fill`, `color-utilities`, `../sass-utils/color-contrast.scss`)
are resolved by the dashboard layer loader in `crates/quarto-sass`, not by the
Sass load path.

## Updating

Copy the same paths from a newer quarto-cli tag (`git show <tag>:<path>`),
bump the tag above and `version` in `resources/vendored.toml`
(`bslib-components`), and rerun the dashboard corpus
(`crates/quarto-core/tests/fixtures/dashboard-goldens/`).
