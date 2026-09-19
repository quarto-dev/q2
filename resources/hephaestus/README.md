# Vendored hephaestus resources

Files the `q2` binary embeds to render
[hephaestus](https://github.com/posit-dev/hephaestus) plot documents
(`.hep`) at render time (`crates/quarto-core/src/transforms/hephaestus.rs`,
bd-3qych45b). Vendored here rather than referenced from
`external-sources/` per the External Sources Policy in the root `CLAUDE.md`,
and mirrors `resources/mermaid/`.

## `fonts/` — the bundled Roboto faces

A `.hep` names font families; it does not carry them. Shaping a plot
with whatever the render machine has installed would make the same
document lay out differently on macOS and Linux (different glyph
advances → different tick-label widths → a different layout), so the
transform registers these four faces with hephaestus's font context
once per process and maps `sans-serif` onto them. They are the exact
files hephaestus's own browser clients (`hephaestus-wasm`,
`hephaestus-svg-wasm`) ship, so SVG produced by `q2 render` and SVG the
wasm client draws in a preview agree glyph for glyph.

| File                   | Face          | SHA-256 |
| ---------------------- | ------------- | ------- |
| `roboto-regular.ttf`   | 400, upright  | `e2c8ae73409fb8f0aa10759e09e260f67b86762fa3d4eba540e9308443a4c96a` |
| `roboto-bold.ttf`      | 700, upright  | `985e01f2314f8ead815ab4863db1a85e012d2917438644c41e8be328742a2d69` |
| `roboto-italic.ttf`    | 400, italic   | `8dfad5487d12df846cd693c47dfbce87f4e33a221d831f2561665decac9716fa` |
| `roboto-bolditalic.ttf`| 700, italic   | `9ea76488b8450059630e926c70749a34d1c3ba17f9fcd257ebf9647db1520753` |
| `OFL-Roboto.txt`       | licence       | — |

- **Source:** `crates/hephaestus-wasm/fonts/` in the hephaestus repository
  at v0.4.1 (commit `dfd84a5`). Those files are themselves static
  instances subsetted from Google Fonts' variable Roboto by that
  directory's `generate.sh`; the coverage (five scripts) and the
  reasoning for static instances over the variable font are documented
  there.
- **Licence:** SIL Open Font License 1.1 (`OFL-Roboto.txt`, which must
  travel with the faces). Roboto declares no Reserved Font Name, so the
  subsetted derivative may keep the family name `Roboto` — which
  matters, because the generated SVG refers to it by name.
- **Not for the theme CSS.** These are for shaping plot text only; the
  document's own typography still comes from Bootstrap / brand.yml.

## Updating

When bumping the `hephaestus` crate version in
`crates/quarto-core/Cargo.toml`:

1. Copy `crates/hephaestus-wasm/fonts/roboto-*.ttf` and `OFL-Roboto.txt`
   from the matching hephaestus tag over `fonts/`.
2. Update the hashes and the commit above.
3. Run `cargo nextest run -p quarto-core -E 'test(hephaestus)'` — the
   determinism and font tests will tell you if the faces changed shape.
