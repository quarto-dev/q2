# Dashboard SCSS layer

`quarto-dashboard.scss` is Quarto 1's dashboard Sass layer, a byte-identical
copy.

- quarto-cli tag: `v1.11.6`
- source: `src/resources/formats/dashboard/quarto-dashboard.scss`

Compiled into dashboard pages after the bslib component layer
(`resources/scss/bslib/`). Plan:
`claude-notes/plans/2026-11-03-dashboard-format-port.md`.

## Updating

`git show <tag>:src/resources/formats/dashboard/quarto-dashboard.scss`, bump the
tag here and `version` in `resources/vendored.toml` (`dashboard-runtime`).
