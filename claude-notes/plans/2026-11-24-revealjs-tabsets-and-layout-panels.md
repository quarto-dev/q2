---
title: 'Layout gaps, RevealJS slices: tabsets and figure panels'
date: 2026-11-24
description: 'Ports two Quarto 1 layout features that RevealJS decks lacked in q2: working tabs for `.panel-tabset`, and rows and cells for `layout-ncol`, `layout-nrow` and `layout`. The panel engine also serves `format: html`.'
status: draft  # implemented and measured locally on andrew/revealjs-layout; decisions D1 to D9 need sign-off
---

**Strands:** bd-y5j0m776 (RevealJS tabsets, open) is slice D. Slice E has no strand of its own: the layout engine that the float taxonomy plan left unfiled was never filed. **Related:** bd-ewfppclm (older duplicate of bd-y5j0m776), bd-9fz5fweg (figures, floats and layout-panel CSS), bd-hcp8m3ve (float and layout class taxonomy, closed), bd-plcqhfcn (subfloats), bd-mlxvysee (dashboard card layout), bd-47afd5ro (tabsets in the preview), GitHub issue 1309.

**Earlier slices:** [A, B and C](2026-11-13-layout-gaps-revealjs-absolute-aside-html-columns.md), merged as PR 1248.

## Overview

Two things a Quarto 1 author writes in a deck came out stacked in q2.

- **Slice D.** A `.panel-tabset` showed its tab titles as headings, one pane under the other.
- **Slice E.** A Div with `layout-ncol`, `layout-nrow` or `layout` kept the attribute as `data-layout-ncol` and its children stacked. The same was true in `format: html`.

Each slice is one commit and can be its own pull request.

## Checklist

- [x] Confirm both gaps on `origin/main` (`661f79b30`, 2026-11-24) with the fixtures.
- [x] Check open PRs, issues and strands for overlapping work. None. Issue 1309 (filed 2026-11-24) reports the panel gap and the subfloat numbering together.
- [x] Slice D: Tabby markup for decks, vendored Tabby, init script, styles, tests, fixture.
- [x] Slice D: browser comparison with Quarto 1.9.38.
- [x] Slice E: the layout engine, the panel DOM, CSS for both formats, tests, fixtures.
- [x] Slice E: browser comparison with Quarto 1.9.38, deck and HTML page.
- [ ] `q2 preview` for both slices. Not done; see "Not done".
- [ ] hub-client changelog. Not needed: neither slice changes `hub-client/`.
- [ ] `cargo xtask verify`. Only the Rust legs were run; see "Tests".
- [ ] Sign-off on decisions D1 to D9.
- [ ] Strand housekeeping, which needs a go-ahead: close bd-y5j0m776 and bd-ewfppclm, file the layout-engine strand, note the panel CSS as done on bd-9fz5fweg.

## The Quarto 1 contract, condensed

Paths are in `external-sources/quarto-cli/`.

### Tabsets in a deck

- `src/resources/filters/customnodes/panel-tabset.lua`. With Bootstrap the renderer is `bootstrapTabs()`. With any other HTML format it is `tabbyTabs()`: a `ul.panel-tabset-tabby` whose id is `tabset-N`, one `li > a` per tab with `href="#tabset-N-M"`, the default tab marked `data-tabby-default`, then `div.tab-content` holding one `div#tabset-N-M` per tab.
- The Tabby library (`src/resources/formats/html/tabby/js/tabby.js`) adds `role`, `aria-selected` and `hidden` when it starts. Quarto 1 patches its copy so that in a deck the tabs stay in the Tab order and arrow keys are left to slide navigation.
- Tabby is started from `formats/html/templates/quarto-html-after-body.ejs`, one instance per tabset. The reveal support plugin (`formats/revealjs/plugins/support/support.js`, `handleTabbyClicks`) stops a tab click from reaching reveal.js.
- Styles: `formats/html/_quarto-rules-tabby.scss`, compiled into the reveal theme, and `.reveal .panel-tabset [role="tab"]` padding in `formats/revealjs/quarto.scss`. The slide list spacing in `formats/revealjs/styles.html` also lands on the tab list.

### Figure panels

- `src/resources/filters/layout/layout.lua`. `partition_cells` splits a Div's blocks into cells and a preamble (cell code, stderr, annotations). `layout_cells` arranges the cells: `layout-ncol` fills rows, `layout-nrow` derives the column count, `layout` is a JSON array of rows of widths where a negative width is a spacer.
- `src/resources/filters/layout/width.lua`. `widthsToPercent` shares a row by the widths its cells ask for. `parseLayoutWidths` turns numbers into percentages of their row and repeats the last row when there are more cells than slots.
- `src/resources/filters/layout/html.lua`. The DOM: `div.quarto-layout-panel`, `div.quarto-layout-row` (plus `quarto-layout-valign-*`), `div.quarto-layout-cell` with `style="flex-basis: 50.0%;justify-content: flex-start;"`. A figure div with layout attributes is rendered as a figure and its outer attr is replaced by the id and `quarto-layout-panel`.
- Alignment (`common/layout.lua`, `layoutCellAlignment`): `layout-align` sets every cell. Without it, a cell with a bare image or a table is centered and any other cell starts at the left. In Quarto 1.9.38 a captioned image is a Figure block by then, so its cell starts at the left.
- CSS: `src/resources/formats/html/_quarto-rules.scss`, lines 38 to 103. The reveal theme compiles the same rules.

## Gap table

Measured on 2026-11-24 with q2 at `661f79b30` and Quarto 1.9.38, in a 1400 by 900 viewport. Numbers are slide units (position relative to the slide section, divided by the deck scale) as x, y, width, height.

| Contract item | q2 before | After | Quarto 1 |
|---|---|---|---|
| D1. Tab list | No list. Two `h2` headings. | `ul` at 0, 89, 1050, 91 | 0, 89, 1050, 91 |
| D2. Tabs | none | 0, 105, 141, 74 and 141, 105, 142, 74 | 0, 105, 141, 74 and 141, 105, 143, 74 |
| D3. Panes | Both shown, stacked | First at 0, 212, 1050, 52. Second hidden. | Same |
| D4. Click on the second tab | nothing to click | Second pane shown at 0, 212, 1050, 52, first hidden, slide index unchanged | Same |
| E1. `layout-ncol=2`, two captioned images | Images at x=0, y=101 and y=219, stacked | Cells at 0, 89, 515 wide and 535, 89, 515 wide | Same x, y and width |
| E2. `layout-ncol=3`, five images | stacked | Rows of 3 and 2. Cells 337 wide at x=0, 357, 713, then 350 wide at x=0, 370 | Same |
| E3. `layout="[[40,-20,40],[100]]"` | stacked | 404 wide at x=0, spacer, 404 wide at x=646. Second row 1050 wide | Same |
| E4. `layout-valign="bottom"`, `layout-align="right"` | ignored | `align-items: flex-end` on the row, `justify-content: flex-end` on the cells | Same |
| E5. `format: html`, `layout-ncol=2` | Two figures 799 wide, stacked | Cells 390 wide, side by side | 390 wide, side by side |
| E6. knitr cell, `layout-ncol: 2`, two plots | Code and plots interleaved, stacked | Code first, then cells at x=293 and x=703, 390 wide, images 383 by 273 | Identical to the pixel |

The raw output is in `~/Desktop/Projects/q2-layout/measure/` on the author's machine (`tabs-after-d.txt`, `panels-q1.txt`, `panels-after-e.txt`, `html-panels-after-e.txt`), with the scripts that produced it.

## What still differs from Quarto 1

- **Cell height in a deck.** A cell holding a captioned image is 106 slide units tall in q2 and 152 in Quarto 1, so a second row starts 46 units higher. The cause is inside the figure, not in the panel: q2 writes `figure > img` and Quarto 1 writes `figure > p > img`, and the paragraph carries a margin and a line box. That markup difference belongs to bd-9fz5fweg. The image itself is centered as in Quarto 1 since 2026-11-25: the deck theme lays a `.quarto-figure-center > figure` with a direct image out as a flex column and centers the image, and ports Quarto 1's `> figure > div` rule for crossref floats. Before that rule the cell was 118 units tall, because the inline image sat in a line box.
- **Sub-figure numbering.** In a figure div with an id, the children are still "Figure 1" and "Figure 2" and the group is "Figure 3". Quarto 1 writes "(a)", "(b)" and "Figure 1". The cells also lack `quarto-layout-cell-subref` and `data-ref-parent`. This is bd-plcqhfcn.
- **Nested tabset ids.** q2 numbers an outer tabset before the one nested in it. Quarto 1 numbers the inner one first. q2's HTML tabsets already did this.
- **Tab order attributes.** Quarto 1.9.38 ships an older Tabby that gives tabs `tabindex` 1, 2 and so on. The copy vendored here is the one in quarto-cli `main`, which gives each tab `tabindex="0"` and fixes Shift+Tab order.
- **List spacing in a cell.** A bullet list is 104 units tall in q2 and 144 in Quarto 1, on any slide. The slide list rules of `styles.html` are not ported.

## Design decisions

All need sign-off.

- **D1. One Tabset node, two renderers.** The parse half (`PanelTabsetTransform`) now runs for decks, and the resolve half picks Bootstrap or Tabby markup by format. Minimal HTML stays excluded, but only for non-deck formats.
- **D2. Vendor Tabby from quarto-cli, not from upstream.** The copy is byte-identical to quarto-cli at `1ad9b3dbcf609351f58c9c7e672232d91a087f2e`, with Quarto 1's reveal patches. Registered in `resources/vendored.toml` under the `quarto-cli` group.
- **D3. Tabby ships only with a tabset.** Quarto 1 loads it in every deck. Here the resolve transform stores `js:revealjs:tabby` and `js:revealjs:tabby-init` when it resolved at least one tabset.
- **D4. Tab bar spacing is written for the tab list only.** Quarto 1's slide list rules (`ul` bottom margin, `li` top margin) are what put the tabs 16 units below the top of the list. Those rules are not in q2. Their effect on `ul[role="tablist"]` is written out, so the tab bar matches. Porting the general rules would move every list in every deck and is left as its own change.
- **D5. Panels are built in `Finalization`, after `crossref-render`.** This follows the float taxonomy design. A figure div is then its rendered `Div > Figure > Div` shape, so the rows go into the figure's content wrapper.
- **D6. Which child Div is a cell.** In Quarto 1 a child Div is used as the cell and a custom node is wrapped. In q2 the custom nodes are already Divs by then, so a Div with one of the classes `quarto-figure`, `quarto-float`, `quarto-layout-panel`, `callout`, `panel-tabset` or `columns` is wrapped and any other Div is the cell.
- **D7. Panels and subfloats.** Panels are built for any Div, with or without cross-referenced children. Subfloat identity is not touched. When bd-plcqhfcn lands, the cell classes and the "(a)" numbering are its to add.
- **D8. HTML and RevealJS only.** Dashboards are left out, because a dashboard reads its own layout from Divs (bd-mlxvysee). The Pandoc formats keep using the vendored Quarto 1 Lua filters.
- **D9. Caption size selector.** The reveal rule for `figure > figcaption` selected `.reveal .slide`, which matches nothing in q2. It now selects `.reveal .slides section`, the convention of slices A to C, and captions are 0.7em as in Quarto 1. This changes every captioned figure in a deck.

A bad `layout` value (not a JSON array, or rows mixed with widths) is a warning and a one-column panel. In Quarto 1 it is a Lua error that stops the render.

## Tests

- Unit: `transforms/panel_tabset_resolve.rs` (Tabby markup), `transforms/layout_panel.rs` (rows, widths, spacers, alignment, cell reuse, nesting).
- Integration: `tests/integration/revealjs_features.rs` (tabsets), `tests/integration/layout_panels.rs`.
- smoke-all: `revealjs/tabset.qmd`, `revealjs/layout-panels.qmd`, `layout/panels.qmd`.
- Run on 2026-11-24: `cargo nextest run -p quarto-core -p quarto-sass -p quarto`, `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings`, `cargo xtask lint`. Results are in the commit messages.

## End-to-end example

```bash
cargo run --bin q2 -- render revealjs-layout.qmd
```

The slide with `::: {layout-ncol=2}` and two captioned images now holds, as read from the output file:

```html
<div class="quarto-layout-panel" data-layout-ncol="2">
<div class="quarto-layout-row">
<div class="quarto-layout-cell" style="flex-basis: 50.0%;justify-content: flex-start;">
<div class="quarto-figure quarto-figure-center">
```

and the tabset slide holds:

```html
<ul id="tabset-1" class="panel-tabset-tabby"><li><a data-tabby-default href="#tabset-1-1">TabA</a></li><li><a href="#tabset-1-2">TabB</a></li></ul>
```

## Not done

- **`q2 preview`.** Not rebuilt and not measured. Both tabset transforms and `layout-panel` are in the preview's exclude list, so the preview shows tabsets and panels stacked, as before. Tabsets wait on bd-47afd5ro. Panels could run in the preview for plain Divs, since the panel is native Divs with a `style`, but a figure div is still a custom node there.
- **`cargo nextest run --workspace` and the hub-client legs of `cargo xtask verify`.**
- **Image width transfer for sub-figures** (`transfer_float_image_width_to_cell`), `tbl-parent` panels without layout attributes, and `quarto-layout-cell-subref`.
- **`.reveal .slide` selectors elsewhere.** `.reveal .slide .footer` in `quarto-revealjs.scss` has the same mismatch as D9 and was not checked.
