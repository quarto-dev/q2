# Vendored mermaid runtime

These files are a **local copy** of [mermaid](https://mermaid.js.org) used to
render ` ```mermaid ` diagram blocks in HTML-family output. They are vendored
here (not referenced from `node_modules/` or `external-sources/`) so the `q2`
binary is fully self-contained and **rendered sites work offline** — the runtime
is embedded at compile time via `include_str!` (see
`crates/quarto-core/src/transforms/mermaid.rs`) and written into the rendered
site as an ordinary project asset. This follows the repo's External Sources
Policy (see root `CLAUDE.md`) and mirrors `resources/revealjs/`.

## Source & version

- **mermaid `11.12.0`** — MIT licensed (see `LICENSE`).
- Homepage: https://mermaid.js.org
- Copied verbatim from the npm tarball's `dist/mermaid.min.js`
  (`https://registry.npmjs.org/mermaid/-/mermaid-11.12.0.tgz`).
- SHA-256 of `mermaid.min.js`:
  `07e37dfa97b337ccc85365d57eddf99b9706f09db3b59b260d0333b23b343c4b`

## Files

| File            | Source (npm package) | Purpose                                    |
| --------------- | -------------------- | ------------------------------------------ |
| `mermaid.min.js`| `dist/mermaid.min.js`| mermaid runtime (self-contained; sets `globalThis.mermaid`) |
| `LICENSE`       | `LICENSE`            | mermaid MIT license                        |

## Use the right dist file — this is the easy mistake

The package ships several builds. **Only `dist/mermaid.min.js` is
self-contained.** In particular, do *not* vendor `dist/mermaid.esm.min.mjs`:
that file is a ~26 KB entry stub that statically imports 10 chunks and
*dynamically* imports ~25 more — one per diagram type — from
`dist/chunks/mermaid.esm.min/` (146 files, ~13 MB). Vendoring it would leave
diagrams broken offline, and the breakage would be **per diagram type**: a page
with a flowchart could work while a page with a gantt chart silently failed.

This is exactly the bug that motivated vendoring in the first place
(`bd-mermaid-runtime-not-bundled-vxejw159`), so it is worth restating. Two unit
tests in `crates/quarto-core/src/transforms/mermaid.rs` guard it:

- `vendored_bundle_matches_pinned_version` — the bundle's own embedded
  `version:"…"` string must equal `MERMAID_VERSION`.
- `vendored_bundle_is_self_contained` — the bundle must contain no `import(`
  and no `chunks/` reference.

## Updating

When bumping mermaid:

1. Download the tarball and extract `dist/mermaid.min.js` and `LICENSE`:
   ```bash
   npm pack mermaid@<version>
   tar xzf mermaid-<version>.tgz package/dist/mermaid.min.js package/LICENSE
   cp package/dist/mermaid.min.js resources/mermaid/mermaid.min.js
   cp package/LICENSE resources/mermaid/LICENSE
   ```
2. Update `MERMAID_VERSION` in `crates/quarto-core/src/transforms/mermaid.rs`.
3. Update `MERMAID_VERSION` in
   `ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx`
   — a test on each side pins the two together.
4. Update the version and SHA-256 above.
5. Re-run `cargo xtask verify`.

The version and self-containment guards above will fail loudly if steps 1 and 2
disagree, so a half-finished bump cannot land silently.

## Why this is vendored rather than pulled from `node_modules`

Unlike `resources/revealjs/`, mermaid is **not** an npm dependency of this
repo — nothing in `hub-client/` or `ts-packages/` installs it (the preview
component currently loads it from a CDN; see `bd-1vwtdwtq`). Adding it purely to
anchor a drift test would pull in 66 MB across 793 files for every developer's
`npm install`. Instead the guards above are self-contained, and the drift test
in `mermaid.rs` (`vendored_bundle_matches_npm_package`) *skips* when
`node_modules/mermaid` is absent — so it starts working automatically if a
future change (likely `bd-1vwtdwtq`) adds mermaid to the npm dependency graph.
