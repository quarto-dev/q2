# Vendored reveal.js assets

These files are a **local copy** of [reveal.js](https://revealjs.com) used to
render `format: revealjs` presentations. They are vendored here (not referenced
from `node_modules/` or `external-sources/`) so the `q2` binary is fully
self-contained — the files are embedded at compile time via `include_str!`
(see `crates/quarto-core/src/revealjs/`). This follows the repo's External
Sources Policy (see root `AGENTS.md`).

## Source & version

- **reveal.js `6.0.2`** — MIT licensed (see `LICENSE`).
- Homepage: https://revealjs.com
- Copied from `node_modules/reveal.js/dist/` (the npm package the hub-client
  already depends on, keeping render and preview on the same reveal.js major
  version).

## Files

| File              | Source (`dist/`)      | Purpose                                  |
| ----------------- | --------------------- | ---------------------------------------- |
| `reset.css`       | `reset.css`           | reveal.js CSS reset                      |
| `reveal.css`      | `reveal.css`          | reveal.js core layout/controls styles    |
| `reveal.js`       | `reveal.js`           | reveal.js core library (UMD; global `Reveal`) |
| `theme/white.css` | `theme/white.css`     | the `white` theme (Tier-1 default)       |
| `LICENSE`         | `../LICENSE`          | reveal.js MIT license                    |

## Updating

When bumping reveal.js (e.g. a hub-client `reveal.js` dependency bump), re-copy
the same files from `node_modules/reveal.js/dist/`, update the version above,
and re-run `cargo xtask verify`. Additional themes / plugins are added in later
phases of the revealjs epic (`claude-notes/plans/2026-06-08-revealjs-presentations.md`).

## Plugins

| File                                    | Source                                                                 | Purpose |
| --------------------------------------- | ---------------------------------------------------------------------- | ------- |
| `plugins/line-highlight/line-highlight.js`  | quarto-cli `src/resources/formats/revealjs/plugins/line-highlight/` | Quarto 1's `QuartoLineHighlight` plugin: `code-line-numbers="1,3"` and stepped `"\|1\|2-3"` |
| `plugins/line-highlight/line-highlight.css` | same                                                                | its styles |

Copied unchanged from quarto-cli at commit
`703927316dcecea438ac3569fb3b70bc401ac441` (MIT; the plugin is adapted from
reveal.js's own highlight plugin, also MIT). To refresh, copy both files again
and update that commit here and in `resources/vendored.toml`. `assemble.rs`
registers `window.QuartoLineHighlight` with `Reveal.registerPlugin` before
`Reveal.initialize`. The plugin
reads `data-code-line-numbers` from `div.sourceCode` and expects Pandoc's
per-line spans, which the HTML writer emits for numbered blocks.
