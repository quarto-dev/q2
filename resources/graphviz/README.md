# Vendored Graphviz runtime (Viz.js)

These files are a **local copy** of [Viz.js](https://viz-js.com), a build of
[Graphviz](https://www.graphviz.org) compiled to WebAssembly. q2 uses it to
draw ` ```dot ` diagram blocks in HTML-family output. The files are vendored
here (not referenced from `node_modules/` or `external-sources/`) so the `q2`
binary is self-contained and **rendered sites work offline**. The runtime is
embedded at compile time via `include_str!` (see
`crates/quarto-core/src/transforms/graphviz.rs`) and written into the rendered
site as an ordinary project asset. This follows the repo's External Sources
Policy (see root `AGENTS.md`) and mirrors `resources/mermaid/`.

## Source & version

- **`@viz-js/viz` `3.31.0`**, which builds **Graphviz `16.1.0`**.
- Homepage: https://viz-js.com. Source: https://github.com/mdaines/viz-js.
- `viz-global.js` is copied verbatim from the npm tarball's
  `dist/viz-global.js`
  (`https://registry.npmjs.org/@viz-js/viz/-/viz-3.31.0.tgz`).
- SHA-256 of `viz-global.js`:
  `c9e0b310f9883910e01c66054b48b7ff4be3d8695141116635e72b0af152692e`
- `LICENSE` is copied from the `release-viz-3.31.0` tag of the source
  repository. The npm tarball does not ship a licence file.

## Licences

- The Viz.js wrapper is MIT licensed (see `LICENSE`).
- `viz-global.js` also contains Graphviz and Expat in object-code form, as its
  header comment states. Graphviz is distributed under the Eclipse Public
  License; Expat is MIT licensed. The notice q2 must carry for the Graphviz
  object code has **not been confirmed with the team yet** (plan
  `claude-notes/plans/2026-11-13-graphviz-support.md`, bd-ijlapt70).

## Files

| File            | Source                          | Purpose                                             |
| --------------- | ------------------------------- | --------------------------------------------------- |
| `viz-global.js` | npm tarball `dist/viz-global.js`| Graphviz runtime (self-contained; sets `globalThis.Viz`) |
| `LICENSE`       | source repository `LICENSE`     | Viz.js MIT licence                                  |

## Use the right dist file

The package ships three builds. **Vendor `dist/viz-global.js`.** It is a
classic script that assigns `globalThis.Viz`, with the WebAssembly binary
embedded in the JavaScript, so it loads from a plain `<script src>` with no
second request.

Do *not* vendor `dist/viz.js`: it is an ES module. A module script does not
load from a `file://` page, so a rendered `.html` opened from disk would show
no diagram.

## The file is text, but treat it as bytes

`viz-global.js` is valid UTF-8 (which `include_str!` requires), but the
embedded WebAssembly binary is stored as a string of raw control characters
and non-ASCII characters. Two consequences:

- **No line-ending conversion.** `.gitattributes` marks the file `-text`. A
  CRLF checkout would change bytes inside the embedded binary.
- **It must be decoded as UTF-8 in the browser.** The emitted `<script>` tag
  carries `charset="utf-8"`, so the file is read correctly even from a page
  that declares another encoding.

## Guards

Unit tests in `crates/quarto-core/src/transforms/graphviz.rs`:

- `vendored_bundle_matches_pinned_version`: the header comment must name
  `VIZ_VERSION`, and the embedded `graphvizVersion` must equal
  `GRAPHVIZ_VERSION`.
- `vendored_bundle_is_self_contained`: no `import(`, no `.wasm` reference, and
  the `Viz` global is assigned. The file does contain one `fetch(` call. It is
  an Emscripten fallback path that never runs, because the binary is embedded,
  so the guard does not forbid `fetch(`.
- `vendored_bundle_matches_npm_package`: byte-for-byte equality with
  `node_modules/@viz-js/viz/dist/viz-global.js`. Unlike mermaid, `@viz-js/viz`
  **is** an npm dependency of the workspace (the preview component in
  `ts-packages/preview-renderer` imports it), so this check runs whenever
  `npm install` has been run.

## Updating

1. Bump `@viz-js/viz` in `ts-packages/preview-renderer/package.json` (it is
   pinned to an exact version) and run `npm install` from the repo root.
2. Copy the new build and licence:
   ```bash
   cp node_modules/@viz-js/viz/dist/viz-global.js resources/graphviz/viz-global.js
   gh api repos/mdaines/viz-js/contents/LICENSE?ref=release-viz-<version> \
     --jq .content | base64 -d > resources/graphviz/LICENSE
   ```
3. Update `VIZ_VERSION` and `GRAPHVIZ_VERSION` in
   `crates/quarto-core/src/transforms/graphviz.rs`.
4. Update `VIZ_VERSION` in
   `ts-packages/preview-renderer/src/q2-preview/blocks/GraphvizCodeBlock.tsx`.
5. Update the versions and SHA-256 above, and `resources/vendored.toml`.
6. Re-run `cargo xtask verify`.

A new Graphviz version can change diagram layout. Read the Graphviz release
notes before bumping.
