# 01-hephaestus-basic — a `.hep` plot document rendered to SVG

A `format: html` document that references one
[hephaestus](https://github.com/posit-dev/hephaestus) plot document
(`figs/readings.hep`) three ways: as a captioned, cross-referenced
figure; at an explicit size; and via a path that does not exist. It
declares a brand (`brand: brand.yml`) with a dark background and a
light foreground, so the plots come out inverted relative to the plain
document.

`figs/readings.hep` is the document hephaestus's own `document_save`
example writes (two panels: a scatter and dashed trend lines). It is the
same file as `crates/quarto-core/tests/fixtures/hephaestus/basic.hep`.

## What this demonstrates

- **`![](plot.hep)` just works.** No front-matter opt-in. The
  `hephaestus-render` transform reads the document, lays it out with
  hephaestus's renderer-free SVG backend (no GPU involved) and writes
  the SVG under `document_files/figure-html/`, then points the `<img>`
  at it.
- **Re-flow, not scaling.** The sized copy is laid out at 420×260: axis
  ticks and labels are recomputed for that size instead of the 900×420
  picture being shrunk.
- **Fail-soft.** The missing file produces the `Q-19-1` warning (plus
  the resource collector's `Q-5-6`, as for any missing image) and the
  image reference is left as written.
- **Deterministic output.** Text is shaped with the bundled Roboto faces
  (`resources/hephaestus/fonts/`), so the SVG bytes are the same on
  every machine.
- **Brand colors.** `brand.yml`'s `background` becomes the plot's paper,
  `foreground` its ink and `primary` its accent. The built-in
  hephaestus theme derives every chrome color (panel, grid, ticks,
  titles) from those anchors, so the whole plot follows the brand. The
  data series keep their own colors — they come from the plot's color
  scale, not the palette. Delete the `brand:` line from `document.qmd`
  to see the plot's own white-paper theme.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/plots/01-hephaestus-basic
```

The page is written next to the source as `document.html`.

## What to look for

- Two `<img src="document_files/figure-html/readings-….svg">` tags —
  the name is derived from the document bytes and the render size, so
  the two copies have different names — and one untouched
  `<img src="figs/does-not-exist.hep">`.
- The SVG files: text is real `<text>` elements naming
  `sans-serif`, and the root `width`/`height`/`viewBox` match the
  requested size.
- In the terminal: one `Q-19-1` warning and one `Q-5-6` warning for the
  missing file, nothing else.
- Each SVG opens with `<rect … fill="#101820"/>` (the brand
  background) and its titles and tick labels are `fill="#f2f2f2"`.
