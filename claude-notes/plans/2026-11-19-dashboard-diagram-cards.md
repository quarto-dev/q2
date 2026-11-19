# Dashboard: diagram blocks get cards

## Overview

In a `format: dashboard` render, a Graphviz (```` ```dot ````) or Mermaid
(```` ```mermaid ````) block got no card. Bare in a row it was a loose
`<pre>` in the grid; alone in a `.card` div the div passed through with no
header or body (title lost, Mermaid drawn at 16x16). Quarto 1 treats
`{dot}` / `{mermaid}` as cells, so each becomes a card.

Cause: `mermaid-render` and `graphviz-render` run before
`DashboardTransform` (which is deliberately last) and replace the code
block with a raw HTML `<pre>`. The card builder ports Q1
`hasRealLookingContent`, which ignores raw blocks, so `make_card` returned
`None` for a card holding only a diagram, and a bare diagram was never a
card candidate.

Fix: the diagram transforms own a predicate for their `<pre>` raw block;
the card builder counts it as real content, and `build_cards` makes a bare
diagram its own card, as it does for a code cell.

Second cause, found while measuring: with the cards fixed, Mermaid still
drew at 16x16 (or squashed) in every dashboard card. `quarto-dashboard.js`
adds `hidden` to `<html>` in the head and removes it on
`DOMContentLoaded`; q2's Mermaid loader ran `mermaid.run()` inline at the
end of the body, so Mermaid measured labels in a hidden document. Quarto 1's
`mermaid-init.js` waits for `load`. The loader now waits for
`DOMContentLoaded` while the document is loading (and runs at once when
injected later). Graphviz lays out without DOM measurement and was fine.

## Work Items

- [x] Failing tests: `dashboard_format.rs` (`diagram_blocks_become_cards`,
      `raw_html_alone_is_still_not_a_card`), corpus fixture
      `contract/c44-diagram-cards.qmd`
- [x] Predicates `is_mermaid_pre` / `is_graphviz_pre` next to their producers
- [x] `has_real_looking_content` accepts them; `build_cards` cards a bare one
- [x] Corpus: c44 as `divergence`, `.q2.skeleton` = Q1 skeleton of the
      brace-form document minus the `cell` class
- [x] End-to-end render and browser measurement against Quarto 1.9.38
- [x] Mermaid loader waits for `DOMContentLoaded` while loading
      (`script_runs_diagrams_after_the_document_has_parsed`)
- [x] Smoke fixture `crates/quarto/tests/smoke-all/dashboard/diagram-cards.qmd`
- [x] Workspace tests (20310 pass; the corpus and smoke-all harnesses fail only on missing R bsicons/gt/flextable and Python great_tables), clippy -D warnings, xtask lint

## Measurement (2026-11-19, 1400x900, Quarto 1.9.38 with `{dot}`/`{mermaid}`)

Card rectangles `[x, y, w, h]`, identical in Q1 and q2:

| Card | Rect | Body |
|---|---|---|
| bare dot | 17,57,675,404 | 18,58,673,402 |
| bare mermaid | 709,57,675,404 | 710,58,673,402 |
| "Flow card" (dot) | 17,479,675,404 | 18,523,673,359 |
| "Mermaid card" | 709,479,675,404 | 710,523,673,359 |

q2 diagrams, all inside their card body: dot 203x59 and 369x59, mermaid
360x61 twice (the same as a plain `format: html` render). Q1 draws dot at
768x480, wider than the card (clipped), and mermaid at about 207x70.

## Divergence (c44)

Quarto 1 cards carry a `cell` class (bare: on the card; in a `.card` div:
on the card body), because its diagrams are executable cells. q2 diagram
blocks are not cells, so q2 cards have no `cell` class and no
`cell-output-display` wrapper divs around the diagram. Grids, cards, fill
classes, titles and expand buttons are the same.

## Not covered

- `//| title:` on a diagram block (Q1 sets the card title from it) still
  warns Q-2-47. Open question for Andrew.
- A diagram with `fig-cap` arrives inside a figure, not as a bare raw
  block, so it still joins a loose flow card; Q1 gives the cell a fill
  card. Not checked against Q1.
- Mermaid on a dashboard page that is not shown at load (`display: none`
  tab pane) still measures hidden. Q1 has the same limitation. Not checked.
