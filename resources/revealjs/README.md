# Vendored reveal.js assets

These files are a **local copy** of [reveal.js](https://revealjs.com) used to
render `format: revealjs` presentations. They are vendored here (not referenced
from `node_modules/` or `external-sources/`) so the `q2` binary is fully
self-contained — the files are embedded at compile time via `include_str!`
(see `crates/quarto-core/src/revealjs/`). This follows the repo's External
Sources Policy (see root `CLAUDE.md`).

## Source & version

- **reveal.js `6.0.0`** — MIT licensed (see `LICENSE`).
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
