---
title: 'OPR-7h: the two Julia plot differences between the plain and recorded routes'
date: 2026-11-09
description: 'Cause of each OPR-7c difference with evidence. The bare-PNG size was a presenter bug and is fixed. The requirejs and jquery scripts are a real gap in how the recorded route handles HTML dependencies, which is not small, so it is handed to Gordon as a proposal.'
---

**Plan:** [OPR-7h](../plans/2026-11-09-opr-7h-plots-scripts-and-png-size.md) ·
**Evidence:** [OPR-7c report](2026-11-09-opr-7c-parity-report.md), causes 3 and 4.
Machine: macOS, Julia 1.12.7 with Plots, Node 24. Marks: (ran), (read).

## B. Bare-PNG size (`julia/plot-website`): presenter bug, fixed

**Cause (read, then ran).** QuartoNotebookRunner reads the PNG header and puts
`{"image/png": {"width": W, "height": H}}` in the output metadata (`png_image_metadata` in its
`cell_processing.jl`). The last value of a cell is an `execute_result` (`evaluation.jl`), not a
`display_data`. The plain route reads that metadata for any output
(`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, `metadataValue`). The presenter read it only for
`DisplayData` (`format_outputs`, the image branch of `presenter/cell.rs`), so a last-value plot lost its size. The
`img-fluid` class on the recorded side is the downstream effect: the `responsive-image` transform tags an image
that has no size. The fixture\'s PNG is 140 by 90 (header bytes `0x8c`, `0x5a`).

**Fix.** One match arm: `CellOutput::ExecuteResult` takes its size the way `DisplayData` does. Test first
(`an_execute_result_image_takes_its_size_from_the_output_metadata_too`): failed with `["![](P)"]` against
`["![](P){width=140 height=90}"]`, passes after. Function changed: `format_outputs` in `presenter/cell.rs` (the
`let (width, height) = match (cell_output, image_entry)` arm only), plus the new test beside
`a_stored_image_takes_its_size_from_the_output_metadata_and_a_cell_option_wins`.

**Check (ran).** The parity runner narrowed to `plot` (`OPR_PARITY_ONLY=plot OPR_PARITY_KNITR=0`): `julia/plot-website`
no longer shows the size difference; it is covered only by the existing proposed entries. The one remaining
unallowlisted case in that run is `julia/records-plot`, for cause A and the capture-facts split below.

## A. requirejs and jquery scripts: a real gap, not small, proposal

**Cause (read, ran).** Not the plot itself. `widgetDependencies` in `ts-packages/quarto-api/src/jupyter/widgets.ts`
sets `jsWidgets` when **any** output in the notebook is `text/html` or `application/javascript`, and
`widgetDependencyIncludes` then writes requirejs, jquery and a `define('jquery', ...)` shim into the head.
For Plots, the `execute_result` carries `application/pdf`, `image/png`, `image/svg+xml` **and** `text/html`
(ran, notebook of `julia/positron-cache`). That `text/html` is only
`<img src="data:image/png;base64,...">`, so the scripts are not needed on these pages. In `julia/records-plot`
the trigger is also the explicit `HTML("<b>bold html</b>")` cell. The 7c report\'s "from the bundle Plots returns"
is right for `positron-cache` and `docs-index-chunk`, but the mechanism is a notebook-wide trigger, not a per-output one.

**The recorded route has no equivalent (read).** Nothing in `presenter/` or the splice stage calls
`widgetDependencies`; the presenter says "Jupyter-widget payloads remain unhandled (follow-up)" in the doc comment
of `format_outputs`. The plain route also strips hoisted HTML-library scripts (plotly) out of the cell outputs
(`widgetDependencies`, "LOAD-BEARING MUTATION"), so the recorded route would also emit those in the body.
So this is case (2) of the plan, a presenter and record gap: on a page whose `text/html` output needs requirejs or
jquery (a widget, itables, plotly), the recorded page has no `require` and the output is broken.

**Not done: a browser check.** I did not install PlotlyJS or load a page in a browser. The evidence is that the head
of the recorded page for `records-plot` has no requirejs, and the plain page does. A script using `require([...])`
would raise a ReferenceError there. This is inference from the two HTML files (ran), not an observed failure.

**Why not a small fix.** The scripts and libraries come from a notebook-level scan, not from one cell, and they
go into the document head (`include-in-header`), so the record must carry them, or the presenter must recompute
the scan from the stored outputs and write includes. Either choice is a record-format or cross-stage change, and the
same question exists for `application/vnd.jupyter.widget-view+json` and the widget state blob. The plan says to stop.

**Proposal for Gordon.**

1. Make the presenter (or the splice stage) apply the same notebook-level scan to the stored outputs of Jupyter-style
   engines and fold the resulting head includes into the document meta, the way `fold_capture_includes_into_meta` does for
   marimo. Move the scan to a shared place (a Rust port of `widgetDependencies`, or the record carries the include
   fragments). Add a presenter step that drops hoisted library scripts from the body.
2. Until then, do **not** sign an allowlist entry for the script lines: a rule that hides them would also hide the
   broken case. If Gordon wants the gate at zero for static Plots pages only, the honest rule is one that fires only
   when every `text/html` output in the document is a bare `<img>` data URI; I did not write it.

## Also seen (not mine, not changed)

`julia/records-plot` and `julia/docs-index-chunk` also show the per-expression split of a two-expression cell
(`println` then a value): the OPR-4d behavior already called out as `julia/capture-facts` in the 7c report.

## Gates

- `cargo nextest run -p quarto-core --lib presenter::`: 270 passed (ran).
- `cargo nextest run -p quarto-core --no-fail-fast`: 8157 passed, 3 failed, 36 skipped (ran). The failures are
  `book::bibliography::...::merged_bibliography_uses_the_references_chapter_lang`,
  `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package` (reads `node_modules`, no `npm install` in this
  workspace) and `book_refactor_guard::typst_smoke_orange_book`. None touches image sizes; I did not run them on the
  integration branch to confirm they fail there too.
- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean (ran).
- Windows cross-clippy: cannot run on macOS (the C build of `tree-sitter` / wasmtime fails under `cc`); the change has
  no platform code.
- `cargo xtask lint`: one known, unrelated finding (`positron_cache.rs:360`).
- `cd hub-client && npm run build:wasm`: succeeded (ran).
- Snapshots and goldens changed: none (0 files).
