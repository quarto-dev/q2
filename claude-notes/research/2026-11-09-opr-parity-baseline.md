---
title: 'OPR-0b: parity baseline for the plain-render switch'
date: 2026-11-09
description: 'What the parity harness measured on 2026-11-09, before any switch: 56 identical pairs, 12 explained by proposed allowlist entries, 7 differences nothing explains yet, and 253 knitr-corpus documents that q2 rejects before any engine runs. Input to OPR-7c.'
---

**Plan:** [OPR-0b](../plans/2026-11-08-opr-0b-parity-corpus-and-harness.md) ·
**Evidence for:** OPR-7c (applies the gate), OPR-7a and OPR-7d.
Machine: macOS, R 4.3.2 with knitr, Julia 1.12.7 with Plots, `uv`; product code at the claim commit of OPR-0b (the
integration tip `f1ea3cf41` plus the claim). Nothing in the product changed. Marks: (ran) unless noted.

Command (ran; one run, about 7.5 minutes):

```bash
OPR_PARITY_REPORT=report.md cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

`OPR_PARITY_ONLY=<substring>` narrows it to matching documents and `OPR_PARITY_KNITR=0` leaves out the existing knitr corpus.

## How the pairs were made

Each document is copied twice into scratch directories and rendered through `render_document_to_file`: plain, and with
`execute: {record: true}` added to the directory\'s `_quarto.yml` (project metadata, so a `.ipynb` needs no front matter
edit). The harness compares the normalized output text and the **set of figure file names**. Noise removed before
comparing: `data:image` payloads, absolute paths, and the ids that change on every render (flextable `cl-` classes, gt
table ids). A pair that differs is rendered once more from scratch, and the report says when the first run differed from
the confirming one. The first Julia render after a server start printed a stray stderr line once; that was transient,
which is why the confirming run exists.

## Counts

| Class | Cases |
|---|---:|
| identical | 56 |
| allowlisted (proposed entries) | 12 |
| unallowlisted difference | 7 |
| document rejected by q2 before any engine runs (same on both routes) | 253 |
| both routes failed at execution, same message | 2 (one expected) |
| routes disagree on success | 0 |
| skipped | 0 |
| formats not compared (docx and epub are zip files) | 8 |

Nothing was skipped: R, Julia with Plots, Node and `uv` were all present. The knitr documents of the margin-layout set
needed ggplot2, flextable and gt, and all three were installed.

## The 7 differences nothing explains

These are **not** seeded in the allowlist. Each is a real difference between the two routes, found by this harness.
Gordon decides each one in OPR-7c.

1. **`julia/fig-subcap`** (the group case). The plain route builds a figure float for the group and each member and
   names the files by label (`fig-two-output-1.png`); the recorded route writes plain figures named `cell-N-output-M.png`
   and, per OPR-0c, leaves the group references unresolved. This is the `bd-gbaykhth` and `bd-au5gts6e` blocker (OPR-7d
   decision 3), deliberately not an allowlist candidate. New detail: the plain route names these files by label, not by
   the cell counter, so the proposed `julia-figure-names` entry does not cover them.
2. **`julia/records-plot`, `julia/positron-cache`, `julia/docs-index-chunk`** (Plots). The plain route emits three
   `script` elements (requirejs and jquery from jsDelivr, plus a small `define(\'jquery\')` shim) that the recorded route
   does not. They come from the `text/html` bundle Plots returns. Whether a page needs them (a static GR plot does not;
   an interactive backend would) was not assessed.
3. **`julia/docs-index-chunk`**, second difference. In the plain output the code block\'s first highlighted line starts
   on a new line after the opening `code` tag; in the recorded output it does not. Cosmetic in a browser (a leading
   newline in a `pre` is dropped), but byte-different.
4. **`julia/engine-reordering`.** A cell that prints `Test Passed` through `@test` shows the text plain on the plain
   route and with ANSI color escapes (green, bold) on the recorded route. This one is a visible defect of the presenter
   route: escape sequences reach the page.
5. **`julia/plot-website`.** The plain route writes the PNG with `width` and `height` attributes (140 by 90); the
   recorded route writes `class="img-fluid"` instead and no size.
6. **`knitr-corpus/structure/chunk_in_blockquote`.** R\'s markdown keeps an empty blockquote shell that the recorded route
   does not. This is the same divergence the knitr corpus already allowlists as a blocker for the baseline presenter
   (`allowlist.toml`, `structure/chunk_in_blockquote`, epic decision 23), now seen at page level on the real path. It is
   not in the proposed allowlist because the knitr entry belongs to a different comparison; Gordon can carry it over in
   OPR-7c.

## What the proposed allowlist explained

- `julia-cell-id` (julia wrapper id, scope `julia/`, proposed): explains 7 cases
- `julia-data-quarto` (julia wrapper data-quarto, scope `julia/`, proposed): explains 7 cases
- `julia-data-execution-count` (julia output data-execution_count, scope `julia/`, proposed): explains 7 cases
- `julia-cell-output-class` (julia figure output class, scope `julia/`, proposed): explains 1 cases
- `julia-figure-names` (julia figure file names, scope `julia/`, proposed): explains 1 cases
- `knitr-empty-cell-wrapper` (knitr empty cell wrapper, scope `knitr`, proposed): explains 5 cases

- `julia/engine-keep-ipynb [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `julia/engine-no-keep-ipynb [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `julia/engine-sleep [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `julia/engine-source-ranges [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `julia/fig-png-bare [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count, julia-cell-output-class, julia-figure-names
- `julia/records-error [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `julia/records-in-use [html]`: julia-cell-id, julia-data-quarto, julia-data-execution-count
- `knitr-corpus/markdown-stress/empty_output [html]`: knitr-empty-cell-wrapper
- `knitr/margin-layout/custom-geometry-asymmetric [html]`: knitr-empty-cell-wrapper
- `knitr/margin-layout/custom-geometry-narrow [html]`: knitr-empty-cell-wrapper
- `knitr/margin-layout/custom-geometry-wide [html]`: knitr-empty-cell-wrapper
- `knitr/margin-layout/fullwidth-figure [html]`: knitr-empty-cell-wrapper

## Identical (56)

- `jupyter/fig-subcap [html]`
- `knitr-corpus/engines/execute_cache_front_matter [html]`
- `knitr-corpus/engines/handled_languages [html]`
- `knitr-corpus/markdown-stress/ansi_and_unicode [html]`
- `knitr-corpus/markdown-stress/backtick_runs [html]`
- `knitr-corpus/markdown-stress/block_start_lines [html]`
- `knitr-corpus/markdown-stress/final_newline [html]`
- `knitr-corpus/markdown-stress/long_line [html]`
- `knitr-corpus/markdown-stress/whitespace [html]`
- `knitr-corpus/rich-output/df_print_default [gfm]`
- `knitr-corpus/rich-output/df_print_default [html]`
- `knitr-corpus/rich-output/df_print_default [revealjs]`
- `knitr-corpus/rich-output/df_print_default [typst]`
- `knitr-corpus/rich-output/kable [gfm]`
- `knitr-corpus/rich-output/kable [html]`
- `knitr-corpus/rich-output/kable [revealjs]`
- `knitr-corpus/rich-output/kable [typst]`
- `knitr-corpus/rich-output/knit_asis [gfm]`
- `knitr-corpus/rich-output/knit_asis [html]`
- `knitr-corpus/rich-output/knit_asis [revealjs]`
- `knitr-corpus/rich-output/knit_asis [typst]`
- `knitr-corpus/rich-output/prefer_html [gfm]`
- `knitr-corpus/rich-output/prefer_html [html]`
- `knitr-corpus/s9/02_inline_rich [html]`
- `knitr-corpus/s9/03_inline_block_widget [html]`
- `knitr-corpus/structure/adjacent_chunks [html]`
- `knitr-corpus/structure/chunk_in_list [html]`
- `knitr-corpus/structure/first_and_last [html]`
- `knitr-corpus/structure/several_figures [html]`
- `knitr/cache-mixed (hit) [html] (pending OPR-7b)`
- `knitr/cache-mixed (miss) [html] (pending OPR-7b)`
- `knitr/exec-scope [html]`
- `knitr/exec-scope-include [html]`
- `knitr/exec-scope-output [html]`
- `knitr/fig-subcap [html]`
- `knitr/margin-layout/custom-geometry-asymmetric [typst]`
- `knitr/margin-layout/custom-geometry-narrow [typst]`
- `knitr/margin-layout/custom-geometry-wide [typst]`
- `knitr/margin-layout/fig-column-margin [html]`
- `knitr/margin-layout/fig-column-margin [typst]`
- `knitr/margin-layout/fullwidth-figure [typst]`
- `knitr/margin-layout/margin-figure-cell-option [html]`
- `knitr/margin-layout/margin-figure-cell-option [typst]`
- `knitr/margin-layout/margin-subfigure-ggplot2 [html]`
- `knitr/margin-layout/margin-subfigure-ggplot2 [typst]`
- `knitr/margin-layout/margin-table-flextable [html]`
- `knitr/margin-layout/margin-table-flextable [typst]`
- `knitr/margin-layout/margin-table-flextable-crossref [html]`
- `knitr/margin-layout/margin-table-flextable-crossref [typst]`
- `knitr/margin-layout/margin-table-gt-r [html]`
- `knitr/margin-layout/margin-table-gt-r [typst]`
- `knitr/margin-layout/margin-table-gt-r-crossref [html]`
- `knitr/margin-layout/margin-table-gt-r-crossref [typst]`
- `knitr/margin-layout/tbl-column-margin [html]`
- `knitr/margin-layout/tbl-column-margin [typst]`
- `marimo/minimal [html]`

Highlights: marimo is identical (decision D12 holds); the stored-notebook group case is identical on both routes (both
leave the group references unresolved, the `bd-gbaykhth` shape); `knitr/fig-subcap` is identical (both number the
sub-figures as separate figures, `bd-au5gts6e`); and the mixed `cache: true` document is identical on a cache miss and on
a hit (pending OPR-7b: its expected result changes there, so this equality is today\'s, not the target).

## What the existing knitr corpus could not show

253 of the 104-entry corpus\'s cases never reach either route. `q2 render` rejects the document first:

- 132 documents use old-style chunk headers (`{r, echo=FALSE}`), error Q-2-36;
- 121 documents fail with a generic parse error on the same kind of header;
- 1 document (`engines/shell_chunk`) names the jupyter engine, whose runtime is not available on this machine (an
  environment gap, and the one failure not marked expected).

The knitr corpus was built to drive the engine directly, below q2\'s parser, so most of it cannot be exercised through
`q2 render`. Plain-versus-recorded parity for those documents is covered by the existing knitr gate at the
engine level (KNR-6), not by this harness. The 31 knitr-corpus cases that did render give the identical and allowlisted
rows above. If Gordon wants end-to-end coverage of the header-style fixtures, they would have to be rewritten with `#|`
options; that is outside this plan.

`julia/engine-error` fails on both routes with the same message, as expected.

## Formats not compared

- `rich-output/kable [docx]`
- `rich-output/kable [epub]`
- `rich-output/df_print_default [docx]`
- `rich-output/df_print_default [epub]`
- `rich-output/knit_asis [docx]`
- `rich-output/knit_asis [epub]`
- `rich-output/include_graphics [docx]`
- `rich-output/include_graphics [epub]`

Typst is compared through the `.typ` file (`keep-typ`), not the PDF; the other LaTeX-family targets stay on the engine
route (decision D5) and are not part of the corpus.

## Evidence notes

- (ran) the full run above, and four earlier runs while building the harness (they found the generated-id noise, the
  `.Rmd` input problem and the rejected-document class).
- (ran) harness tests: 18 tests, including a markdown-only document rendered both ways and a rendered pair with one
  changed paragraph.
- Not done: running the harness on Windows, and a Quarto 1 comparison of the seven differences.
