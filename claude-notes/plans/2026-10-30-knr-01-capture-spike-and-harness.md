---
title: 'KNR-1: knitr capture spike and differential harness'
date: 2026-10-30
description: 'Captures knitr evaluated pieces on the q2 side, maps them to nbformat, presents them with the unmodified shared presenter, and compares the result with knitr own markdown over a generated corpus, classifying every difference. The report decides the scope of the rest of the knitr execution-records epic.'
status: complete  # 2026-11-01; report: research/2026-11-01-knr-01-corpus-report.md
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-1).
**Depends on:** [EXR-1](2026-10-28-exr-01-presenter-extraction.md) (the extracted,
WASM-clean presenter) and [EXR-2](2026-10-28-exr-02-unit-scanner.md) (the shared unit
scanner, which `capture_one_unit_per_chunk_in_order` checks against). It may start as soon
as those two are available on the integration line; it does not wait for the rest of the
parent epic (epic decision 1, Gordon, 2026-10-31). EXR-10, EXR-11 and EXR-12 are not
needed: the harness runs on the current markdown path and keys chunks by a harness-local
ordinal (Phase 1B), not by a record id.
**Start condition (as of 2026-10-31, EXR-1 to EXR-18 are all drafts and no presenter code
exists on the integration line).** Until EXR-1 and EXR-2 land, do everything that needs
neither: spikes S1 to S5 and S7 to S9 (S7 up to its substitution step), the classifier,
the corpus manifest and generator, the control run and the normalizer, and Phase 1B. Do
not stub the presenter or the scanner. Phase 1C, the runner, `capture_one_unit_per_chunk_in_order`,
S7\'s substitution step and the report wait for EXR-1 and EXR-2; leave their checkboxes
open. "Available" means EXR-1 Phase 3 (the presenter functions moved) and EXR-2 Phase 2 (the
scanner) are merged on the integration line, not only drafted; EXR-1 Phase 4 and 5 are not
needed. The presenter entry points are whatever EXR-1 exports: today\'s `render_cell`,
`FigureWriter` and `CellVisibility` are `pub(super)` in `engine/jupyter/text_execute.rs`
and EXR-1 replaces `FigureWriter` with a `FigureSink` trait (`FsFigureSink`), so this plan
says "the presenter" and "the figure sink". If the exports are not reachable from
`crates/quarto-core/tests/integration/`, raise it with the parent session; do not widen
visibility here.
**Blocks:** every other KNR plan (KNR-2 to KNR-7), which are outlines until this plan
reports.
**Estimated sessions:** 4 to 6. This is a guess made before any spike ran; it covers nine
spikes plus a generator, classifier, normalizer, capture, Rust mapper, baseline, runner and
report. S1 alone (about 18 fixtures, three mechanisms, three knitr versions) may take two
to three of them; re-estimate after S1 reports.

> **For implementers:** TDD, task by task; steps use `- [ ]` checkboxes. Tests that need
> R skip when it is missing. The helpers `rscript_available`, `knitr_r_package_available`
> and `skip` are private to each test file (see
> `crates/quarto-core/tests/integration/knitr_label_reinject.rs:37-64`, and five more
> copies), so the new modules get one shared copy in
> `crates/quarto-core/tests/integration/knitr_corpus_support.rs`; the existing files are
> not touched. Every new module that calls R starts with
> `#![cfg(not(target_arch = "wasm32"))]`, like the existing ones. Paths built in tests use
> `Path::join` (or `std::env::join_paths` for `PATH` and `R_LIBS`), never a `/` or `:`
> literal. The live render path does not change in this plan: everything is additive and
> opt-in.

## Goal

Answer, with measurements, three questions:

1. **Can the pieces be captured** (which capture point, on which knitr versions, for
   which chunk shapes), and which chunks cannot be?
2. **How far is the unmodified shared presenter from knitr's own markdown**, per
   construct? This is the baseline KNR-2 closes.
3. **How are the differences distributed** across *identical*, *syntax-level*,
   *semantic but looks the same*, *blocker* and *intentional divergence*?

The output is a committed report and an updated epic, with KNR-2 to KNR-7 rewritten
from the numbers.

## Background

Read on 2026-10-30 in `external-sources/knitr`. That tree is a **development snapshot**
(`DESCRIPTION` Version 1.52.5, `NEWS.md` headed 1.53, `git describe` v1.52-69), so the line
numbers below match no release: `evaluate(` is at `block.R:261` in v1.44, `:262` in v1.50
and `:313` in the snapshot. Installed locally: knitr 1.50, rmarkdown 2.29. Cite released code
by tag (`v1.50:R/block.R:262`); Phase 1S re-reads every line it depends on in the released
versions it tests. Behavior known to differ across versions, to confirm in S1: in 1.50 a
numeric `eval` echoes the masked lines with a `##` prefix and the snapshot does not; in 1.44
to 1.46 a level-1 or level-2 cache hit goes through `fix_evaluate`; plot alt text, `log.echo`
and engine-modified options passed to the chunk hook are newer than 1.50. These four come
from reading the history, not from a run; S1 confirms each or removes it.

- **Where knitr builds a chunk's output.** `eng_r` runs `evaluate()` (`block.R:313`),
  filters and rearranges the pieces (`:352-384`), then `unlist(sew(res, options))`
  (`:392`), then the chunk hook (`:396`), then applies `include` (`:425`). Non-R engines
  go through `block_exec` (`:214`). The document is the `"\n"` join (`one_string`,
  `utils.R:1130`, applied at `output.R:295`) of per-group results (the group loop is
  `output.R:346-370`).
- **`sew` is the main formatting entry point, not the only one.** It is an S3 generic with
  methods for `character`, `message`, `error`, `recordedplot`, `knit_asis`, `list` and
  others (`NAMESPACE:11-22`); `engine_output(options, out = LIST)` routes list-shaped
  engine output through it (`engine.R:94`). Most engines call `engine_output(options, code,
  out)` with a string (19 call lines in `engine.R`) and have no pieces. The chunk's before
  and after hook strings (`run_hooks`, `block.R:267`, `:393-395`) enter the output outside
  `sew`.
- **q2 already patches knitr's namespace, and already replaces `sew`.** The R code in
  `resources/rmd/patch.R` is byte-identical to Quarto 1's. Its overrides are:
  `htmltools::htmlPreserve` (sets `htmltools.preserve.raw`), `parse_block` on older knitr,
  `add_html_caption` (`patch.R:142-149`), `sew` itself (`patch.R:150-213`, a manual S3
  dispatcher that special-cases `knit_asis`, `knit_image_paths` and `rgl` plots),
  `wrap_asis_output` (`patch.R:99-139`: the `cell-output-display` wrapper, the caption after
  a widget, `{=html}` fencing of table output, the `results='asis'` bare rule) and
  `valid_path` (`patch.R:285-292`). "Wrap `sew`" therefore means wrapping
  q2's replacement. The version branches in q2's R code are `patch.R:6` (1.37.2),
  `patch.R:152` (1.32.8) and `patch.R:300` (`<= 1.45`); `hooks.R` has none (its mentions of
  1.44 at `:332`, `:793` and `:842` are comments). `rmd.R:8` stops below knitr 1.44, but
  only for `.R` input (the spin path), and `execute.R:774` picks `has_crop_tools`, so
  neither sets a floor for `.Rmd` and `.qmd` documents. No hard pin was found.
- **The current engine path.** `KnitrEngine::execute` (`engine/knitr/mod.rs:157`)
  preprocesses inline spans, runs `execute.R` through `rmarkdown::render(run_pandoc =
  FALSE)`, reads the output markdown, and applies `postprocess_markdown` (`:225`). The
  result type is `KnitrExecuteResult` (`engine/knitr/types.rs:97`); adding a
  `#[serde(default)]` field is wire-additive.
- **The presenter.** After EXR-1, `render_cell(language, code, &result, &mut FigureWriter,
  CellVisibility)` and `format_outputs` are target-neutral Rust. Today it handles
  `stream`, `error`, `execute_result` and `display_data` text, html, png, jpeg and svg;
  `text/markdown`, `text/latex` and widget payloads are known gaps (EXR-1, Background).
- **The oracle is nondeterministic in known places.** In stock knitr, preserve placeholder
  ids are random (`htmltools::extractPreserveChunks`); q2's patch sets
  `htmltools.preserve.raw`, so those markers may not appear in q2's markdown at all (not
  established). `htmlwidget-<hex>` ids and `plotly` `cur_data` ids differ between runs
  (reported by review, not re-run). Figure directories and temp paths vary; timestamps and unseeded
  random numbers in user code are likely sources. The normalizer list is built from a
  control run (Phase 1A), not from this guess.
- **No R in CI.** A grep of `.github/workflows/*.yml` found no R setup (see the epic).
- **Not verified:** reticulate's Python engine; how `knit_asis` objects carry their kind
  (raw html, markdown); behavior on knitr 1.44 to 1.49 (history read, nothing run).

## Design decisions (from the epic; do not re-litigate)

1. The capture is **additive and opt-in** in this plan: the live path and its output are
   unchanged, and captured pieces come back as an extra field.
2. knitr's finished markdown is the **oracle only**. It is not stored in any record this
   plan produces.
3. Comparison levels, cheapest first: **bytes**, then **parsed AST**, then **rendered
   output**. Classes: *identical*, *syntax-level*, *semantic but looks the same*,
   *blocker*, *intentional divergence* (epic decision 5).
4. The baseline presenter is the **unmodified** shared presenter. Every gap it shows is
   input to KNR-2; none is fixed here.

## Out of scope

- Fixing differences (KNR-2), the record format and hashing (KNR-3), inline expressions
  (KNR-4), Positron (KNR-5), flipping the live path (KNR-6).
- Any change to rendered output of existing documents.

## Phase 1S: spikes first (each ends in a written decision in this plan)

- [x] **S1, capture point and mechanism.** Two questions, both decided here. *Where* in
  knitr's pipeline to capture (epic decision 10 chooses before the filters; this spike can
  overturn it), and *how* to hook that point. knitr's pipeline for an R chunk is
  `evaluate()` (`block.R:313`), then **filters** by `echo`, `results`, `warning`, `message`,
  `fig.keep` and `fig.show` (`:352-371`), then merging (`:382`), then `sew` (`:392`), where
  `sew.recordedplot` writes the figure files and calls the plot hook (`output.R:657-677`).
  Line numbers are the snapshot's; S1 re-reads them in 1.44, 1.50 and the latest release.
  - **Where.** *(a) Wrap `sew`* (q2's replacement of it): sees pieces **after** the
    filters, so the record depends on presentation options (a chunk run with `message:
    false` has no message piece), and the unit hash must cover those options, which is close
    to the old all-options rule. *(b) Capture before the filters*: the record is
    kernel-like (everything the code produced, in execution order), so the hash can leave
    presentation options out, and the presenter reimplements the filters (KNR-2, Phase 2D).
    *(c) Both*: capture at (b) and let knitr's normal path run, using its markdown as the
    shadow oracle only.
  - **The capture must not change what R does.** Do not force `stop_on_error`: use knitr's
    own value (`block.R:317-319` derives it from `error` and `include`), so a chunk that
    errors without `error=TRUE` aborts the render as today and no record is made. Do not
    force `keep_warning` or `keep_message` by default: `warning`, `message`, `error` and
    `include` are execution-relevant (epic decision 10). Test forcing `keep_*` only
    together with a live path that re-filters, since logical `warning` and `message` are
    enforced by `evaluate()` and not by the later filters (reported by review, not re-run; confirm),
    and record what `warning=NA` and `message=NA` do under forcing.
  - **Plots are selected at capture time.** With `fig.keep='none'` knitr opens no recording
    device (`block.R:259`, `keep != 'none'`), so there is nothing to capture, and the
    default `high` merges low-level additions into the plot they modify (`merge_low_plot`,
    `block.R:570`: `plot(); lines(); plot()` gives two files, not three). Capture applies
    knitr's own merge for the `fig.keep` in effect (epic decision 10); S1 checks whether
    `fig.show` can stay a presentation option.
  - **How (for (b) and (c)), in this order.** (1) Wrap the `evaluate` hook
    (`knit_hooks$get('evaluate')`, `block.R:303`, the value called at `:313`): no change to
    knitr, and it sees the pieces before the filters, before numeric-`eval` masking is
    stripped (`:335`), before `fig.alt` is merged (`:348`) and before the `eval.after`
    options are resolved (`:343-345`). It is not called for `eval=FALSE`, blank code or
    cache hits (`:308-311`), so recording runs with knitr's cache off (KNR-3, option a) and
    `eval=FALSE` and blank chunks become source-only units taken from the document.
    (2) A replacement copy of `eng_r` through `assignInNamespace`: about 190 lines (the size
    comes from review and is not measured), sees
    everything, version-fragile. (3) Wrapping `sew` is available only to (a). The capture
    lives in its own file beside `patch.R` and never edits the existing overrides.
  - **Fixtures**, each recorded for what every candidate sees and misses: plain;
    `include=FALSE`, also list-nested; `eval=FALSE`; `results='asis'` and `'hold'`;
    `fig.show='hold'`; `error=TRUE`; `error=TRUE` with `include=FALSE` and a failing chunk
    (reported by review, not re-run, to abort the whole knit); `error=FALSE` and a failing chunk;
    `warning=FALSE`, `warning=NA`, `message=FALSE`, `message=NA`; `fig.keep` set to `none`,
    `high` and `all` over `plot(); lines(); plot()`; a `knit_asis` table; numeric `echo`;
    numeric `eval`; cache levels 1, 2 and 3, each run twice; a `fig.cap` set by an
    expression.
  - **Versions.** Run on knitr 1.44, 1.50 (installed, with evaluate 1.0.4 and xfun 0.52) and
    the latest release. Only 1.50 is installed, so S1 starts with setup, which needs network
    access to CRAN and its archive (S1 cannot run offline):
    - [x] An R script in the corpus directory (run with `Rscript`, so it is portable)
      that builds a scratch library for each version under a directory the caller passes in.
      knitr 1.44 predates evaluate 1.0, whose piece classes changed, so the script also pins
      the `evaluate` and `xfun` versions that the knitr version was released with (read from
      the CRAN archive dates), and the report says which combination each column measured.
      Rust callers set `R_LIBS` with `std::env::join_paths`.
    - [x] The S1 fixtures are committed in the corpus directory under `s1/` and are part of
      the corpus manifest (Phase 1D), not throwaway.
    - [x] A harness (an R script that runs one fixture under one candidate mechanism and one
      knitr version and writes one JSON file per cell) and a results table in this plan:
      one row per fixture, one column per mechanism and version, each cell "sees" or
      "misses" with a short note.
    Decide the capture point, the mechanism and the minimum knitr version, and write them
    in a subsection headed `#### S1 decision` directly under this item, with three lines
    (`Where:`, `How:`, `Minimum knitr:`) and the results table beneath, so that KNR-3 can
    cite it. The recommendation going in is (b) through mechanism (1), because it is the one
    consistent with the epic's decision 6 and with Positron interop; confirm or overturn it
    with the experiment.

#### S1 decision

Run 2026-10-31. Every finding below was **verified by running it** unless marked "read" (a
source reading of the cited file and line) or "not run".

- **Where:** before the filters, at the output of `evaluate()`, with knitr's own path left
  running (candidate (c)); epic decision 10 holds and is not overturned. Everything that
  `evaluate()` itself enforces stays execution-relevant (hashed): logical `warning` and
  `message`, `error`, `include`, `eval`, `fig.keep` (plots are selected at capture), `cache`.
  `echo`, `results`, numeric `warning` and `message`, and `fig.show` can stay presentation
  options.
- **How:** mechanism (1), a wrapper around `knit_hooks$get('evaluate')`, which records the
  pieces and then applies `knitr:::rearrange_figs(res, keep, idx, 'asis')` to select plots
  (an internal function; if it ever goes away, re-implement it from the exported
  `knitr::is_low_change`). The chunk hook supplies the final options and fallback markdown.
  `eval=FALSE` and blank chunks (the hook is not called) become source-only units taken from
  the document, and knitr's cache is off while recording (S4 confirms). No mechanism
  changed R's behavior, so the stop-and-raise condition did not arise.
- **Minimum knitr:** 1.44, the lowest version tested (`rmd.R:8` also stops below 1.44 for
  spin input). Versions below 1.44 and 1.45 to 1.49 were not run.

**Columns measured.** 1.44 = knitr 1.44 + evaluate 0.21 + xfun 0.40; 1.50 = knitr 1.50 +
evaluate 1.0.4 + xfun 0.52 (the installed set, newer than what was current at its release,
2025-03-16); 1.52 = knitr 1.52 + evaluate 1.0.5 + xfun 0.62. highr 0.11 and yaml 2.3.10 come
from the system library; R 4.3.2, macOS arm64. The 1.44 pins are the evaluate and xfun
current at its release (2023-09-11), read from the CRAN archive; the script is
`crates/quarto-core/tests/fixtures/knitr-corpus/scratch-libs.R`. The harness ran
`knitr::knit()` directly (not `rmarkdown::render`), with q2's `patch.R`, `execute.R` and
`hooks.R` sourced unchanged and a synthetic HTML-target `format` standing in for the one
Rust passes, so the rmarkdown layer is the untested part. The harness and the driver are committed as `s1/harness.R` and
`s1/run-matrix.R` (the harness also drives the `s2/`, `s5/`, `s7/`, `s8/` and `s9/`
fixtures through environment variables: `S1_RENDER`, `S1_TO`, `S1_DFPRINT`, `S1_NOCACHE`).
The raw per-cell JSON is not committed; it is regenerated by the driver, under
`<out-root>/<version>/<fixture>/<mechanism>/`. Cells are the same on all three
versions unless noted; the only 1.44 difference is the trailing newline of source text
(evaluate 0.21 omits it on the last expression).

**Mechanisms.** (1) the evaluate-hook wrapper. (2) the original `eng_r` body with one
capture call injected right after the `res = ... evaluate(...)` step, by editing the body and
using `assignInNamespace`; this stands in for a replacement copy and sees what a copy would
see at that point (the anchor was found unchanged in 1.44, 1.50 and 1.52). (3) a wrapper
around q2's replacement `sew` that records the list passed from `eng_r`. The chunk hook is an
observer on `knit_hooks$get('chunk')`. "src" is a source piece, "plot" a recorded plot.

| Fixture | (1) evaluate hook | (2) `eng_r` injection | (3) wrap `sew` | Chunk hook |
|---|---|---|---|---|
| 01 plain | sees one src piece per top-level expression, plus stdout pieces | sees the same | sees adjacent sources already merged into one group (`merge_class`, 1.50 `block.R:313`) | called; finished markdown only |
| 02, 02b `include=FALSE`, also inside a list | sees all pieces, message included | sees the same | sees all pieces: `sew` still runs, and `include` only discards the result (1.50 `block.R:356`) | called; returns the full text |
| 03 `eval=FALSE`, blank chunk | **misses**: the hook is not called | sees a source-only piece for `eval=FALSE`, an empty list for blank | sees the same as (2) | called (empty text for blank) |
| 04 `results` asis, hold, hide | sees every piece, including stdout under `hide` | sees the same | sees the filtered list: `hold` merged, `hide` has no stdout | called |
| 05 `fig.show='hold'` | sees execution order (src, src, plot, src, out, src, plot) | sees the same | sees plots moved to the end | called |
| 06 `error=TRUE` | sees the error piece and the pieces after it | sees the same | sees the same, sources merged | called |
| 07 `error=TRUE`, `include=FALSE`, failing | knit aborts (`stop_on_error` is 2); no pieces | same | same | not reached |
| 08 `error=FALSE`, failing | knit aborts (same with capture off); no pieces | same | same | not reached |
| 09 `warning` or `message` logical `FALSE`, or `NA` | **misses** the dropped class: `evaluate()` never emits it (`NA` goes to the R console) | same | same | called |
| 09 numeric `warning=1`, `message=2` | sees every warning and message | sees the same | sees only the selected ones | called |
| 10 `fig.keep` none, high, all, first, last over `plot(); lines(); plot()` | none: **no plot** (no device is opened); the others: 3 plots, unmerged | same | none 0, high 2 (merged), all 3, first 1, last 1 | called |
| 16 `fig.keep=2`; `fig.show` hold, asis, hide | 3 plots, whatever `fig.show` is | same | `fig.keep=2` gives 1 plot; hold moves plots to the end; hide still writes the file | called |
| 11 `knit_asis` (kable, `asis_output` markdown and HTML) | sees a `knit_asis` piece | same | same, except `results='hide'` removes it before `sew` | called |
| 12 numeric `echo` | sees all four sources | same | sees only the chosen sources, merged | called |
| 13 numeric `eval` | sees the masked line as a source piece starting with `## ` | same | same | called |
| 14 `cache=1`, second run (hit) | **misses** | sees the cached pieces | sees them (`sew` runs) | called |
| 14 `cache=2`, second run | **misses** | sees the cached pieces; the plot piece is a digest list, not renderable | same | called |
| 14 `cache=3`, second run | **misses** | **misses** | **misses** | **misses**: not called either |
| 15 `fig.cap` as an expression | sees `fig.cap` as an unevaluated call | same | sees the resolved string | sees the resolved string |

**What the numbers say.**

1. **Capture changes nothing.** The markdown with any of the four observers is byte-identical
   to the control on all 264 comparable cells (19 fixtures, three versions, both runs of the
   cache fixtures; 07 and 08 abort identically with capture off).
2. **Logical `warning` and `message` are enforced inside `evaluate()`.** Forcing
   `keep_warning` and `keep_message` to `TRUE` changed the live markdown on fixture 09 in all
   three versions: the chunks with `FALSE` and `NA` started printing their warnings and
   messages into the document, because the later filters act only on numeric values
   (`filter_evaluate`, 1.50 `block.R:298-299`, read). Under forcing, `NA` chunks print nothing
   to the R console. So forcing is unsafe while knitr's own path produces the document; it
   would only be safe after KNR-6 if the presenter re-filters, which this spike could not
   test. Decision: do not force; `warning` and `message` stay in the hash.
3. **Plots.** At the evaluate hook `plot(); lines(); plot()` is three plots, and `fig.keep`
   none is zero (read, 1.50 `block.R:213`: the device is opened with recording off). Capture-time
   `rearrange_figs(..., 'asis')` selected exactly the plots `sew` sees on all 114 chunk
   comparisons (three versions), so plot selection can happen at capture. `fig.show` can stay
   a presentation option: `hold` only moves plots to the end, `hide` still writes the file
   (`sew.recordedplot` saves before the check, 1.50 `output.R:603-609`, read, and the figure
   directories confirm it), and `rearrange_figs` selects the same plots whatever `fig.show`
   is (`high_hold` and `high_asis` both write two files). `fig.show='animate'` was not run
   (needs ffmpeg or gifski).
4. **Pieces are finer than what knitr shows.** `evaluate()` returns one source piece per
   top-level expression; `merge_class` (`block.R:313` in 1.50) merges adjacent sources, and
   merges messages and warnings under conditions, after the echo and `results` filters.
   Dropping stdout (`results='hide'`) makes all sources adjacent and so one group. The record
   therefore holds per-expression sources and the presenter reimplements the merge; this
   feeds S2's decision on source groups.
5. **`error=TRUE` with `include=FALSE` aborts the whole knit**, as the review reported
   (fixture 07), and so does a failing chunk with `error=FALSE` (08); neither makes a record,
   and the capture does not alter that.
6. **Final options exist only later.** The evaluate hook sees `fig.cap` as an unevaluated
   call (`fig.cap` is in `eval.after`, so a chunk that sets it by an expression needs the
   chunk's own variables); `sew` and the chunk hook see the resolved string and `fig.num`.
   A chunk-hook observer is therefore a recording point for the final options (S3 decides
   what it records). The header spelling is a bare expression, `fig.cap=paste(...)`:
   `fig.cap=quote(paste(...))` yields the caption "paste" in stock knitr and fails under
   q2's hooks (the control run errors with "invalid type of argument\[1\]: 'language'").
7. **Cache.** A level-1 or level-2 hit never reaches the evaluate hook, so (1) misses it; a
   level-2 hit has only digests for plots, and `save_plot` reuses the file on disk or purges
   the cache with an error if it is missing (1.50 `plot.R:141-148`, read); a level-3 hit
   returns the cached string before anything runs (1.50 `block.R:94-101`, read), so not even
   the chunk hook is called. This is the input to S4.
8. **Plan claims checked.** (a) A numeric `eval` echoes masked lines with a `##` prefix on
   1.44, 1.50 and 1.52; the stripping is only in the development snapshot (`strip_mask` and
   `mask.eval` appear in no release through 1.52, grep). (b) In 1.44, 1.45 and 1.46 a level-1
   or level-2 hit goes through `fix_evaluate` (1.44 `block.R:259`, read); 1.50 and 1.52 do
   not; 1.47 to 1.49 not read. The structure of the captured pieces on a hit is the same
   either way. (c) `log.echo` and plot alt-text merging (`merge_fig_alt`) are absent from
   releases 1.44 to 1.52, so they are "newer than 1.52", not "newer than 1.50". Engine-modified
   options passed to the chunk hook: not checked. (d) `eng_r` is 165, 170 and 180 source lines
   in 1.44, 1.50 and 1.52 and 195 in the snapshot, so "about 190 lines" is true only of the
   snapshot.
9. **Implementation note.** With evaluate 1.0 and later the piece list has class
   `c("evaluate_evaluation", "list")`; with 0.21 it is a plain list. Test it with
   `inherits(x, "list")`, never `identical(class(x), "list")` (my first `sew` wrapper did
   the latter and recorded nothing).
10. **Mechanism (2) is not needed.** Its only extra reach over (1) is `eval=FALSE`, blank
    chunks and level-1 or level-2 hits; the document already supplies the first two, and the
    cache is off while recording. It stays a fallback if the hook ever disappears.
- [x] **S2, piece to nbformat mapping.** For each piece class decide the nbformat
  output: text to `stream` stdout, message and warning to `stream` stderr, error to an
  `error` output (`ename`, `evalue`, `traceback`), `recordedplot` to `display_data`
  `image/png` (or `svg`/`pdf` per `dev`), `knit_image_paths` (`include_graphics`) to
  `display_data`, and `knit_asis` to `display_data` with `text/html` or `text/markdown`.
  Source order is settled with Gordon (2026-10-31): under the default `collapse=FALSE`
  knitr alternates source and output (a source group, its output, the next source group),
  and a notebook cell has one source followed by outputs, so the record carries the
  position of each source piece relative to the outputs; S2 decides the encoding. Decide
  how a record marks "raw markdown, not stdout" for `results='asis'`, and how knitr's own
  figure file path is carried (output metadata). Also decide whether a message or warning
  is recorded as R produced it or as knitr formats it (`output.R:614-654`: the `Warning in
  <call>:` prefix, `xfun::str_wrap` at the R `width` option, `comment_out`), because the
  `width` option is state the record does not otherwise hold. Write the table into this
  plan, headed `#### S2 decision`. S2 must also fix two things Phase 1B and 1C depend on:
  (i) the **wire shape** R emits, in full: the JSON key (`pieces`), the per-piece fields
  (class tag, text, plot path, message call, `knit_meta`, source position) and how the
  figure-path carriage works, written as a JSON example plus the Rust type that reads it, as a per-chunk field parallel to
  EXR-11\'s `chunks` (which carries finished markdown for fallback units), not an extension
  of it; KNR-3 merges the two per unit
  (`KnitrExecuteResult` has no `Serialize`; only deserialization is tested); and (ii) **how
  a chunk with several source groups is presented**: `render_cell` takes one source string
  and one output list, so S2 decides between one presenter call per source group (empty
  code for the continuation groups) and one call per chunk (a recorded divergence), and
  the baseline in Phase 1C does what S2 says. Requirements added on 2026-10-30 (epic decision 19): (1) a `knit_asis` also
  carries `knit_meta`, the HTML dependencies an htmlwidget needs, which knitr collects
  separately from the text (`output.R:576`) and q2 reads at the end of the run
  (`execute.R:205-244`). The mapping must say where the record carries them, a corpus
  htmlwidget fixture must show them surviving the round trip, and S2 records the
  filter-dependent rules: `results='hide'` drops `knit_asis` before `sew` runs
  (`block.R:362`, since `knit_asis` inherits `character`), so its `knit_meta` is not added,
  while `include=FALSE` still runs `sew` and adds it. A dependency set rebuilt from the
  record must equal the live one. (2) Test the mime choice separately for HTML, markdown
  and Typst/docx targets (LaTeX is a fallback, epic decision 13), since the text is already
  in the target format's dialect. (3) For each `knit_asis` kind (kable and `df_print`
  tables, htmlwidgets, raw HTML, plain markdown), record what q2's `wrap_asis_output`
  (`patch.R:99-139`) does and where the Rust presenter must reproduce it: the caption after
  a widget (`sew.knit_asis`, `output.R:586-596`), the `{=html}` fencing, the hidden class
  and the `output_label_placeholder`.
#### S2 decision

Run 2026-10-31 on knitr 1.50 through the real `rmarkdown::render` path (the harness now has a
render mode that builds q2\'s own `knitr_options()` and an `rmarkdown::output_format` the way
`execute.R` does; on `01_plain` its markdown equals knit mode byte for byte), targets `html`,
`typst`, `docx` and `gfm`, and `df_print` `default`, `kable`, `paged` and `tibble`. Fixtures
are committed under `s2/`. Verified by running unless marked "read" or "reported, not
re-run"; 1.44 and 1.52 were run only for the `knit_meta` test (identical result).

**Mapping, piece to record.** The record holds standard nbformat outputs; a knitr source piece
is not an output.

| knitr piece | Record | Notes |
|---|---|---|
| source (one per top-level expression) | the cell source plus a position, see Source order | the presenter re-merges adjacent sources (`merge_class`, S1 finding 4) |
| character (stdout) | `stream`, `name: stdout`, `text` | `results='asis'`, `hold` and `hide` stay presentation options; the record needs no "raw markdown" mark for stdout (see below) |
| message | `stream`, `name: stderr`, `text` as R produced it | messages end in a newline, so knitr never wraps them (`msg_wrap`, 1.50 `output.R:549`) |
| warning | `stream`, `name: stderr`, text as knitr formats it before `comment_out`: `Warning in <call>: <msg>` or `Warning: <msg>`, wrapped by `xfun::str_wrap` | wrapped at the R `width` option when the message has no newline: with `options(width = 40)` a long warning folds at 40 (fixture 05). Recording the formatted text keeps `width` out of the record; formatting happens at capture, where the state equals what `sew` sees |
| error | `error`: `ename` the first class (`simpleError`), `evalue` the message, `traceback` one element, `as.character(condition)` (the text knitr prints, e.g. `Error in f(): boom`) | `error=TRUE` only; without it the knit aborts (S1) |
| recordedplot | `display_data`, `image/png` (or `svg`/`pdf` by `dev`), bytes read from knitr\'s own file; `metadata.quarto_file_path` is that file\'s document-relative path (renamed from `quarto_knitr.file`, 2026-11-02, epic decision 25) | first device only when `dev` is a vector; the files come from the chunk\'s `plot_files` (read: `sew.recordedplot` appends them, S1) |
| `knit_image_paths` (`include_graphics`) | one `display_data` per path, same shape | the piece carries `made.png` itself; `plot_files` does not list it |
| `knit_asis`, text matching q2\'s HTML test | `display_data`, `text/html` | the test is `wrap_asis_output`\'s own: starts `<tag ` or `<tag>`, ends `</tag>`, and is not `<div class="kable-table">` (`patch.R:117-122`, read) |
| `knit_asis`, anything else | `display_data`, `text/markdown` | kable and `asis_output` markdown |
| `knit_asis_htmlwidget` | `display_data`, `text/html` with q2\'s `{=html}` fence removed (the piece text is `\n```{=html}\n...\n```\n` because of q2\'s `htmlPreserve`); dependencies in `metadata.quarto_knitr.knit_meta` | the presenter re-adds the fence as it does for any `text/html` |
| any other class | the whole chunk becomes a fallback unit | `Other` in the wire type |

**Why stdout needs no "raw markdown" mark.** `results='asis'` makes knitr treat the stdout
piece as raw (`sew.character`, `output_asis`), which is a presentation decision made from the
option; the record keeps a plain `stdout` stream and the presenter emits it bare. Only an R
object that declares itself `knit_asis` is raw whatever the options say, and that is the
`display_data` rows above.

**`knit_meta` (epic decision 19).** A widget piece carries its dependencies in its `knit_meta`
attribute (8 per plotly widget). Carriage: `jsonlite::serializeJSON` of that list in
`metadata.quarto_knitr.knit_meta`, the call `execute.R:244` already makes for the whole run.
Test (`09_widget_filters`: a normal, a `results='hide'`, an `include=FALSE` and an
`echo=FALSE` widget; render mode; 1.44, 1.50, 1.52): the live `knit_meta` has 24 entries;
rebuilding it from the pieces of every chunk whose `results` is not `hide` gives 24 entries,
element-wise identical to the live set, and each element survives a `serializeJSON` round
trip unchanged. So `results='hide'` drops the dependencies (the piece is removed before
`sew`) and `include=FALSE` keeps them, as the plan expected, and the presenter, which knows
`results`, applies that rule when it rebuilds the set. Entries repeat across widgets (each
plotly widget lists the same 8); deduplicating at record level is optional. The dependencies
hold paths into the R library, not bytes, so a set rebuilt on another machine only works if
the same packages are installed there.

**Source order and the wire shape.** knitr alternates source and output; nbformat has one
source then outputs, and `stream` and `error` outputs have no `metadata` field (reported, not
re-run: nbformat 4 schema), so a position cannot ride on the outputs. Encoding: the cell
`source` is the authored text (the unit hash is recomputed from it), the code `evaluate()`
received is `quarto_echoed_code` when it differs (epic decision 21, amended 2026-11-03; the
first version of this paragraph had it as the cell `source`), and the cell metadata holds
`quarto_output_anchors`, an array parallel to `outputs` whose entry is the number of source
lines that precede that output. Equal neighbors form one source group; no anchors means all
the source first. (Verified by the KNR-3 corpus gate, 2026-11-03: numeric `eval` shows the
masked lines as `## ` source and `ref.label` supplies the referenced chunk\'s code, so both
need `quarto_echoed_code`; `echo` numeric is applied at present time.) The R side emits,
next to EXR-11\'s `chunks`, a per-chunk field `pieces`:

```json
{
  "pieces": [
    {
      "ordinal": 1,
      "label": "plain",
      "code": "x <- 1:3\nx\nprint(\"hi\")\n",
      "options": {},
      "pieces": [
        {"class": "source", "text": "x <- 1:3\n"},
        {"class": "source", "text": "x\n"},
        {"class": "output", "text": "[1] 1 2 3\n"},
        {"class": "warning", "text": "Warning in f(): boom", "call": "f()"},
        {"class": "error", "text": "Error in f(): boom\n", "ename": "simpleError",
         "evalue": "boom", "call": "f()"},
        {"class": "plot", "files": ["doc_files/figure-html/plain-1.png"]},
        {"class": "image_paths", "paths": ["made.png"]},
        {"class": "asis", "text": "<div>..</div>", "mime": "text/html",
         "widget": true, "knit_meta": "<serializeJSON string>"}
      ],
      "files": ["doc_files/figure-html/plain-1.png"]
    }
  ]
}
```

`options` is S3\'s. Rust reads it with these types (field names as in the JSON; `message`
and `output` are the same shape as `warning` and `source`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct KnitrCapturedChunk {
    pub ordinal: u32,
    pub label: String,
    #[serde(default)] pub code: String,
    #[serde(default)] pub options: serde_json::Value,
    pub pieces: Vec<KnitrPiece>,
    #[serde(default)] pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum KnitrPiece {
    Source { text: String },
    Output { text: String },
    Message { text: String },
    Warning { text: String, #[serde(default)] call: Option<String> },
    Error { text: String, ename: String, evalue: String, #[serde(default)] call: Option<String> },
    Plot { files: Vec<String> },
    ImagePaths { paths: Vec<String> },
    Asis { text: String, mime: String, #[serde(default)] widget: bool,
           #[serde(default)] knit_meta: Option<String> },
    Other { classes: Vec<String> },
}
```

`KnitrExecuteResult.pieces: Vec<KnitrCapturedChunk>` is `#[serde(default)]`, so the field is
wire-additive and `pieces_field_deserializes_with_and_without` can be written.

**How a chunk with several source groups is presented (decision).** One presenter call per
**chunk**, with the code concatenated and the outputs in order; the baseline in Phase 1C does
this, and the interleaving is a recorded divergence that KNR-2 closes with the anchors.
Reason, from reading `render_cell` (`text_execute.rs:562-582`) and from knitr\'s own markdown
(`07_groups`, `08_figures`): `render_cell` emits one `::: {.cell}` wrapper per call, and knitr
emits one wrapper per chunk with code and outputs alternating inside it. A call per source
group would emit one wrapper per group, and an empty-code continuation call would emit a
wrapper holding only outputs, which breaks the one-wrapper-per-cell contract (epic decision
12). Three other divergences the baseline will show, all seen in the real markdown: (1)
`results='asis'` chunks have **no `.cell` wrapper at all** (bare `cell-code` blocks and raw
text, `04_asis`), so the wrapper contract needs an exception there; (2) `collapse=TRUE`
moves the output into the code block as `## ` lines, and `echo=FALSE` leaves a wrapper with
outputs only (`07_groups`); (3) several plots give one code block and one captioned figure
per plot, with `fig.cap` recycled and `fig.alt` as a `fig-alt` attribute (`08_figures`).

**What `wrap_asis_output` does per kind, and what the presenter reproduces** (markdown
read from the real runs, html target):

| `knit_asis` kind | knitr markdown | Presenter must |
|---|---|---|
| kable (pipe table) | `.cell-output-display` wrapper around the table; the caption is the cell\'s `tbl-cap` attribute (`{#tbl-demo .cell tbl-cap='A demo table'}`), not appended text | wrap in `cell-output-display`; leave the caption to the cell layer |
| `df_print: kable`, html | `<div class="kable-table">` plus the pipe table, unfenced, in the wrapper | keep the div unfenced |
| `df_print: paged`, html | paged-table div and JSON script in a five-backtick `{=html}` fence, in the wrapper | fence HTML that passes the html test; `execute.R` appends the pagedtable dependency |
| raw HTML (`asis_output`) | five-backtick `{=html}` fence, in the wrapper | same fence rule |
| markdown (`asis_output`) | bare text in the wrapper | wrap only |
| htmlwidget | wrapper with the `{=html}` block, then the caption paragraph inside it; with a label the wrapper is `{#fig-widget .cell-output-display}` | caption after the widget, label as the wrapper id (`output_label_placeholder`) |
| any, under `results='asis'` | no wrapper, text bare (even raw HTML stays fenced) | the bare rule, and no `.cell` wrapper (above) |

Not exercised: `output.hidden`, `html-table-processing: none`, and `prefer-html`.

**What depends on the target format (feeds KNR-3 decision 8).** `kable()` text is the same
pipe table on all four targets. `df_print` is not: `kable` gives the `kable-table` div on html
and a bare pipe table on typst; `paged` gives an HTML paged table on html and falls back to
the typst pipe table (`execute.R` forces `kable` outside html); `tibble` and `default` give
the same stdout text everywhere. An htmlwidget is HTML only: on typst, docx and gfm knitr
tries a screenshot and fails here with "Chrome debugging port not open after 10 seconds",
so structured widget pieces exist only for html (and, untested, `prefer-html`). The mime
choice itself is content-based (the HTML test above), not target-based, so it needs no
per-target branch. Pieces therefore do differ per format for `df_print` and widgets, which
supports the recommendation in KNR-3 decision 8 (i): the format is part of the structured
profile\'s hash input.

**Open under bd-sr73.** The file paths above stay paths. If Gordon decides on base64, the
`plot` and `image_paths` rows change to `data` in the `display_data` and the `files` field
drops out; nothing else here changes.

Every spike writes its decision in a subsection headed `#### Sn decision` directly under
its checkbox (S1 and S2 above name the contents; the others state what to decide), so the
other plans and the exit criteria can cite them by heading.

- [x] **S3, resolved options.** Capture the resolved chunk options for each chunk (after
  `opts.label`, `opts_hooks` and `eval=params$x`). `fig.cap`, `fig.scap` and `fig.alt` (the
  snapshot adds `fig.note`) are in knitr's `eval.after` list and are evaluated in the
  chunk's environment after the code runs (`block.R:343-345`), so the final options exist
  only at the chunk hook or after that point; capture them there (S3 also records whether
  this makes the chunk hook a recording point for structured chunks, which the epic leaves
  to S3 and S7), and recompute `fig.num`
  and the plot numbering (`block.R:374-379`, `plot_counter` in `sew.recordedplot`) from the
  captured list. List the options that cannot be serialized (functions, environments) and
  decide the rule (drop, string-ify, or flag). Decide one spelling for option names in the
  record: knitr writes `fig.width`, EXR-4 uses Quarto names (`fig-width`), and the plans
  must say which. Fixture: a `fig.cap` set by an expression.
#### S3 decision

Run 2026-10-31 through the render-mode harness (q2\'s own `knitr_options()`), chunk-hook
observer, knitr 1.44, 1.50 and 1.52, fixtures `s8/01_setup_state` and the S1 matrix. Verified
by running unless marked "read".

- **The recording point for final options is the chunk hook.** It is called for every
  executed chunk, including `include=FALSE`, `eval=FALSE` and blank ones (S1 table), and it
  sees the resolved options: `fig.cap` as a string (S1 finding 6), `fig.num`, the template and
  `opts_hooks` results. `fig.num` equals the number of plots that survive selection on all
  120 chunk comparisons across three versions, so the plot numbering is **not recomputed**:
  read `fig.num` there. The evaluate hook supplies the pieces and the chunk hook the options
  and the join: the evaluate hook stores its pieces in a per-chunk slot, the chunk hook emits
  the chunk record and clears it. When the evaluate hook was not called (`eval=FALSE`, blank),
  the chunk hook emits a source-only record from `options$code`. The chunk hook is not called
  for a level-3 cache hit, which S4 removes by turning the cache off.
- **One spelling: whatever the chunk hook sees, which is knitr\'s dotted spelling for knitr
  options and Quarto\'s dashed spelling for Quarto-only ones.** q2\'s R layer rewrites every
  known knitr option from `fig-width` to `fig.width` (`normalize_options`, `hooks.R:815-896`,
  read; confirmed: `#| fig-width: 5` arrives as `fig.width` 5, while `code-fold` and
  `layout-ncol` keep their dashes). Consequences, for KNR-2 and KNR-3: the Rust header layer
  must apply the same rewrite (a copy of that table) before layering the current `#|` options
  over the recorded ones, and the structured profile\'s list and EXR-4\'s flat keys must
  match names after the same rewrite, because a document may write either spelling.
- **Serialization rule.** Record the resolved options as a JSON object with these rules:
  drop options whose value is `NULL` (14 of 69 in 1.50; absent and `NULL` mean the same);
  drop the ones derived from the document text or from state outside the unit: `code`,
  `params.src`, `original.params.src`, `yaml.code`, `original.code`, `hash`, `fig.path`,
  `cache.path`, `fig.cur`; convert a language value to its deparsed string and flag it (none
  arrived at the chunk hook, because `eval.after` options are already resolved there); drop
  functions and environments and list their names in an `unserializable` array. In the run
  only `fig.process` (set in a setup chunk) was a function; it is execution-relevant, so the
  hash cannot see its body, which is a known limit (a function-valued option changed in a
  setup chunk does not re-execute that chunk; the setup chunk\'s own unit goes stale, KNR-3
  decision 10). Everything else in the 69 is logical, number or string.
- **The closed presentation-only list for the structured profile**, starting from
  `names(knitr::opts_chunk$get())` plus the names q2\'s R layer treats as known
  (`hooks.R:335-378`, read). Anything not named here counts in the hash, which is also how
  unknown options (forwarded as attributes, `hooks.R:379-396`) and `attr.*` or `class.*`
  extensions are handled.

  ```text
  echo fenced.echo chunk.echo results collapse prompt comment strip.white highlight size
  background indent class.source class.output class.message class.warning class.error
  attr.source attr.output attr.message attr.warning attr.error
  fig.cap fig.scap fig.subcap fig.alt fig.align fig.show fig.pos fig.env fig.lp fig.link
  out.width out.height out.extra
  classes panel column fig-column tbl-column cap-location fig-cap-location tbl-cap-location
  code-fold code-summary code-overflow code-line-numbers
  lst-cap lst-label layout layout-nrow layout-ncol layout-align layout-valign
  keep-hidden html-table-processing
  ```

  Execution-relevant, so not on the list (KNR-3\'s provisional set confirmed, with these
  additions): `warning`, `message` (logical forms are enforced inside `evaluate()`, S1
  finding 2, and one option name cannot be split), `tidy` and `tidy.opts` (they rewrite the
  code `evaluate()` sees), `label` (it names the figure files), `fig.ncol`, `fig.sep`,
  `animation.hook`, `interval`, `aniopts`, `ffmpeg.*`, `external`, `sanitize`, `purl`,
  `split`, `autodep`, `dependson` and every `cache.*`. `fig.show` joins the presentation
  list (S1 finding 3). **Not decided here:** `output` (Quarto\'s cell-level `output: false`
  and `asis`; at document level `execute.R:324-331` turns `output: false` into
  `results='hide'`, `fig.show='hide'`, `warning=FALSE` and `message=FALSE`, so the cell-level
  mapping needs a run before it can go on the list) and `out.width.px`, `out.height.px`
  (derived from `fig.width` and `dpi`; not recorded). `echo` numeric and `warning` or
  `message` numeric forms are presenter-side filters (S1), but they share a name with the
  logical forms, so the names stay as classified.
- [x] **S4, cache.** Read: `cache=TRUE` is level 3, and a hit returns the cached *output
  string* before the chunk runs (`block.R:100-101`), so no pieces exist. Levels 1 and 2
  cache the *piece list* (`res.orig`, `:337-340`), but a hit reads it with
  `cache$output(hash, 'list')` inside the expression that otherwise calls `evaluate()`
  (`:308-311`), so a capture at the `evaluate` hook never sees a hit. Level 2 stores
  `remove_plot(res, ...)` (`:339`, "when cache=2, we do not really need plots"), so a
  level-2 hit has no renderable plots even if it were reached. A document-level `execute:
  cache` sets the global `opts_chunk$cache` and an `opts_hooks$cache` (`execute.R:378-402`),
  so every chunk is then a cache candidate. Confirm by running each level twice and
  record what each capture mechanism sees on the hit. The expected decision, for KNR-3
  with the numbers, is that knitr's cache is off for the recording run.
#### S4 decision

Run 2026-10-31, on the S1 matrix (knitr 1.44, 1.50, 1.52), fixtures `14_cache_level1/2/3`,
each run twice in fresh R processes. Verified by running unless marked "read".

- **Rule: knitr's cache is off for the recording run** (KNR-3 option (a), the expected
  decision). Rust must force it every time, whatever the document says: q2's existing
  `opts_hooks$cache` that sets `options$cache <- FALSE` (`execute.R:394-399`) does the job,
  but `execute.R` installs it only when `execute: cache` is exactly `false`
  (`identical(cache, FALSE)`), so the recording run needs its own unconditional copy.
- **What a hit looks like without the rule** (table in S1, rows 14): level 1 and level 2
  hits never reach the evaluate hook, so mechanism (1) misses them; a level-2 hit holds only
  digests for plots and reuses the figure file already on disk; a level-3 hit returns the
  cached string before anything runs, so not even the chunk hook is called and the chunk
  yields no record at all (1.50 `block.R:94-101`, read).
- **What the rule does:** with the hook installed, the evaluate hook and the chunk hook fire
  on both runs at all three levels, in all three versions (18 cells), no cache directory is
  created, and the markdown of the second run equals the first (checked for level 3 on 1.50).
- **Costs, accepted:** a document that relies on `cache=TRUE` re-runs those chunks every
  time it executes, and results that depend on state outside the document are recomputed.
  Existing cache directories are neither read nor purged. This changes runtime for such
  documents, and KNR-3 should say so in user-facing notes.
- **Not run:** a document-level `execute: cache: true` together with the hook (it sets
  `opts_chunk$cache`, `execute.R:392`; the hook runs on resolved options, so the header
  case above covers it by reading, not by running), and `cache.lazy`, `dependson` and
  `autodep` interactions.
- [x] **S5, odd chunks.** Observe `child=`, a shell chunk, a SQL chunk, `engine_output(out
  = LIST)`, reticulate Python if available, and a document that overrides
  `knit_hooks$set(output = ...)`. For each: structured, synthesizable (code plus stdout),
  or fallback; and whether an overridden hook can be detected.
#### S5 decision

Run 2026-10-31, render mode, html target, knitr 1.50 only, fixture `s5/01_odd_chunks` (a
`child=` chunk with `child.Rmd`, a bash chunk, an SQL chunk over RSQLite 2.4.1 in a scratch
library, a custom engine that calls `engine_output(options, out = LIST)`, and a setup-chunk
override of `knit_hooks$output`). Observers: the evaluate hook, the chunk hook, and a hook
comparison. Verified by running unless marked.

| Case | Evaluate hook | Chunk hook | Verdict |
|---|---|---|---|
| `child=` parent chunk | not called | **not called** | invisible to every observer: fallback by the count rule |
| a chunk inside the child | called, ordinary pieces | called, own label (`child_inner`) | looks like a normal chunk, but the scanner has no unit for it, so the unit count and labels disagree: no record plus a diagnostic (KNR-3 decision 4). Its output is spliced inline in the parent at the parent\'s position, with no wrapper for the parent chunk |
| bash chunk | not called | called | no pieces; synthesizable (code plus stdout string) |
| SQL chunk | not called | called | no pieces; output is a finished table (`<div class="knitsql-table">` with a pipe table, outside any `cell-output` wrapper); fallback or synthesizable as `text/markdown` |
| custom engine, `engine_output(out = LIST)` | not called | called | the list never reaches a hook, but `engine_output` is where it is assembled (`engine.R:94`, read), so wrapping it would recover the pieces; not run |
| reticulate Python (run 2026-11-03, reticulate 1.42.0, knitr 1.50, uv venv with numpy) | not called | called (`engine: python`) | no pieces reach a hook; the chunk hook holds one string with source and output interleaved (`x = 1 + 1\nprint(x)#> 2\nx#> 2`), so output cannot be split from code. Fallback now. Synthesizable later: reticulate assembles its pieces and hands them to `getOption("reticulate.engine.wrap", eng_python_wrap)`, a documented seam, so an overriding wrap could capture them (not run). The default hooks stay untouched, so the hook comparison only names the chunk hook q2 itself replaces |

- **Structured, synthesizable or fallback.** Structured: R chunks only. Everything else is a
  fallback unit for now, because the finished markdown is all the chunk hook holds for it. The
  synthesizable path (wrap `knitr::engine_output` to capture the `code` and `out` it is given)
  is a later improvement, untested here; the table above marks which engines would use it.
- **An overridden hook can be detected, and cheaply.** Take the knit hooks q2 installs at
  render start, and in the chunk hook compare each with `knitr::knit_hooks$get(name)` using
  `identical` (closures held by reference compare equal). After `knitr::knit_hooks$set(output
  = ...)` in `override_hook`, the comparison named `output` from that chunk on and nothing
  before it. The override changed the live markdown (`OVERRIDDEN` prefixed every output after
  it), which the presenter would silently ignore, so detection is needed. **Granularity:**
  hooks are global and persist, so from the first flagged chunk (that chunk included) every
  later chunk is a fallback unit, with a diagnostic naming the hook; an override that is
  later reverted is not tracked (flag stays; not tested).
- **Scanner question (KNR-3 decision 6).** The scanner cannot be asked here (EXR-2 is a
  draft); the observation is only that R reports the child\'s chunk where the scanner has the
  parent, so the count-mismatch rule is what catches `child=`, not scanner knowledge.
- [x] **S6, rendered-output seam.** Find the narrowest seam to render a markdown string
  as if an engine had produced it, so level 3 (rendered output) can compare the two
  paths. Candidates: the replay engine from a recorded trace
  (`claude-notes/instructions/replay-engine.md`), or calling the stages after the engine
  directly. Also confirm pampa can parse both markdown strings to ASTs for level 2.
#### S6 decision

Run 2026-10-31 on this branch after the rebase onto the integration line (it has the
presenter and the scanner). The scratch test code is not committed (it lives in the
scratchpad as `tools/knr_s7_scratch.rs`); each claim below was run unless marked "read".

- **Level 2 (parsed AST): `pampa::readers::qmd::read`, compared through
  `pampa::writers::native::write`.** `read(bytes, false, "x.md", &mut std::io::sink(), false,
  None)` returns `(Pandoc, ASTContext, diagnostics)`. All 57 `off` outputs of the S1 and S2
  matrix runs parsed, with no failure and no diagnostics. The native writer prints the AST
  with no source locations, so two documents compare with a plain string comparison. The
  runner should call `read` directly and not the `post_engine_ast` helper in
  `engine_output_parity.rs`, which goes through `record_capture` (the legacy path that
  EXR-5, EXR-6 and EXR-17 change). `semantic_classes` and `assert_blocks_parity` are private
  there and would need the `pub(crate)` promotion the plan describes (read).
- **Level 3 (rendered output): `RenderToFileOptions.engine_registry_override` with a
  fixed-output engine.** A test engine named `knitr` that claims language `r` and returns a
  given markdown string from `execute()` makes `render_to_file(doc, "html", ..)` run the whole
  pipeline after the engine on that string: parse, transforms, render, template. The document
  text is ignored by the engine, so any small `.qmd` works (`.Rmd` chunk headers such as
  `{r label}` do not parse as `.qmd`: Q-2-36). A render took 0.17 to 0.23 s. Run on the oracle
  markdown and on the substituted markdown of S7\'s `01_structure`: both rendered, and the two
  HTML files differ only in the output directory name (`off_files` against
  `substituted_files`, because the output name differs) and in the blockquote (below).
- **The replay engine is not that seam for knitr, as it stands.** `ReplayEngine` claims a
  language only when `language == engine_name` (`engine/replay.rs:107-113`, read), so a trace
  recorded from a `knitr` document does not claim `{r}` chunks. Recorded with `trace: true`,
  an unmodified trace replayed with `q2 render doc.qmd --replay <trace>` gave a page with
  unexecuted `{r}` code blocks and no error, on both the PATH `q2` (nightly 0.36.0) and the
  branch\'s own build. That contradicts the hard-fail policy in
  `claude-notes/instructions/replay-engine.md`. The same file is out of date: the trace field is
  `engine_captures`, a list (schema version 2), not `engine_capture`, and the trace is written
  as `latest.json.gz`. A hand-edited trace was therefore not usable, and the override above is
  the seam to use. Not investigated further, and no bead filed: it is outside this plan.
- **A divergence the comparison found (feeds the normalizer).** On `01_structure` the two
  parsed ASTs are identical except in the blockquote. knitr\'s hook output starts with a blank
  line that has no `> ` prefix, so after `> quote before` the live document ends the quote and
  the chunk lands in a second `BlockQuote`; the substituted document, whose blank
  continuation lines carry the prefix, keeps one. Seen only in a quote (the list case is
  identical, because the hook output carries the indent). The baseline is the substituted form;
  the divergence is an *intentional* one, listed for the Phase 1B normalizer.
- [x] **S7, skeleton by markers (decides KNR-6's design).** In R, make the chunk hook
  return a marker (an HTML comment with a unit id, preceded by `options$indent`) instead of
  the formatted chunk. Check that: knitr's own join builds a document with one marker per
  chunk in the right places (list-nested, blockquote, adjacent, first and last in the file);
  `include=FALSE` drops the marker (`block.R:425`); the markers survive `rmarkdown::render`,
  `extract_preserve_chunks` and `postprocess_markdown`; a level-3 cache hit replays the
  marker but yields no pieces. Fallback chunks must still return real markdown, so the hook
  behaves two ways per chunk: test that mix. Test a level-3 cache written *before* the flip,
  which holds full markdown and no markers, and record what the skeleton does with a cached
  string that has no marker (expected: that chunk becomes a fallback unit). Everything above runs
  with no parent code. The last step waits for Phase 1C (the baseline presenter): substitute
  baseline-presenter output for the markers and compare with the live markdown. Record the
  verdict, under `#### S7 decision`: markers, or Rust assembly from the source (which needs
  agreement with knitr\'s `split_file` on every chunk boundary). The verdict is written when
  the last step is done.
#### S7 decision

**Interim: everything up to the substitution step is done; the verdict (markers, or Rust
assembly from the source) is not written yet and this checkbox stays open.** Run 2026-10-31,
render mode, html target, knitr 1.44, 1.50, 1.52 (the marker skeleton is byte-identical on
all three), fixtures `s7/01_structure` and `s7/02_cache3`. The chunk hook returns a marker
`<!-- knr-unit:N:label -->` for structured chunks and its real markdown for chunks labeled
`fb_` (standing in for fallback chunks). Verified by running unless marked "read".

- **Placement is right, if the marker carries the indent.** One marker per chunk, in the
  order of the chunks, for a chunk first in the file, last in the file, adjacent chunks, a
  chunk in a list item and a chunk in a blockquote. knitr does **not** add the indent to the
  hook\'s return value: without `options$indent` in front, the list marker landed at column 0
  and the blockquote marker lost its `> `, which would end both containers. With the indent
  (two spaces in the list, `> ` in the quote) every marker stays inside its container. So
  the hook returns `options$indent` followed by the marker, as the plan said, and Rust
  must put each line of the presenter\'s output behind the same prefix, which it reads off
  the marker line (everything before `<!--`). The list-item case also keeps an indent-only
  line where a hidden chunk was.
- **`include=FALSE` drops the marker.** Units 5 (`in_list_hidden`) and 8 (`hidden_top`) ran,
  called the chunk hook, and are absent from the document, because knitr discards the hook\'s
  result (`if (options$include) output else indent`, 1.50 `block.R:356`). The unit numbers
  therefore skip, so the skeleton cannot be aligned by counting markers alone; align by the id
  in the marker, with hidden units known from the record.
- **Markers survive the pipeline.** They are intact after `rmarkdown::render` (the markdown
  file is the render output), after q2\'s own `extract_preserve_chunks` (0 preserved chunks,
  6 of 6 markers still present), and `postprocess_markdown` cannot change them: it only
  replaces `<stem>.rmarkdown` with the original file name (`mod.rs:329-346`, read).
- **Mixing works.** The `fb_` chunk kept its full `.cell` markdown among the markers, in the
  right place, so one document can hold both kinds.
- **Cache.** A level-3 cache written with markers replays the **marker** on a hit, with no
  chunk-hook call and so no pieces (all three versions). A level-3 cache written *before* the
  flip replays the **full markdown** with no marker, so that chunk shows up in the skeleton
  as finished text where a marker was expected (count and id mismatch, which the fallback rule
  covers). Both disappear under the S4 rule, since the cache is off while recording and the
  hook always runs.
- **Substitution step (run 2026-10-31, after rebasing onto the integration line, which has
  the presenter).** A throwaway integration test (kept in the scratchpad, not committed)
  read the marker skeleton and the evalhook capture of `s7/01_structure` (knitr 1.50, html,
  render mode), and for each marker called `quarto_core::presenter::cell::render_cell` with a
  `CellResult` built from the chunk\'s pieces (`character` to a `stdout` stream; `message` and
  `warning` to `stderr`; `code` from the capture; `echo` and `warning` from the resolved
  options). Each line of the presenter\'s output got the marker line\'s prefix (everything
  before `<!--`), and blank continuation lines got the prefix trimmed. The fallback chunk
  kept its own markdown. Result: the substituted document equals knitr\'s live markdown
  (`off` run) **line for line once blank lines and trailing whitespace are ignored**, for all
  six structured units, including the chunk in the list item and the one in the blockquote.
  Raw `diff` is not empty: blank lines differ. The presenter closes the wrapper as
  `:::\n:::` without knitr\'s blank line between the two, and knitr leaves an
  indent-only line where the presenter emits none. Both are the baseline presenter\'s
  formatting (Phase 1C territory), not the skeleton\'s. The parsed ASTs (S6 level 2) are
  identical except in the blockquote (see S6 decision).
- **What the substitution did not cover.** One fixture; stdout and stderr text only (no
  plots, errors, `asis` pieces, several source groups or `results='asis'` chunks); only html;
  only 1.50 (the skeleton itself is identical on 1.44 and 1.52).

#### S7 verdict

**Markers.** The chunk hook returns `options$indent` plus a marker, Rust substitutes the
presenter\'s output for each marker line, and every line it adds takes the marker line\'s
prefix. Reasons: placement and containers are right on every position tested; the markers
survive `rmarkdown::render`, `extract_preserve_chunks` and `postprocess_markdown`;
`include=FALSE` and fallback chunks behave as required; knitr\'s own join does the
splitting, so Rust does not need to agree with `split_file` on chunk boundaries; and the
substitution reproduced the live markdown apart from blank lines. Rust assembly from the
source is not needed unless the corpus shows a placement case the markers cannot express.
Conditions KNR-6 inherits: align by the id in the marker, not by counting (hidden units
leave gaps); the cache is off while recording (S4), which also removes the stale-cache
problem; a chunk with no marker where one was expected is a fallback unit; and the
blank-line differences above are settled by the Phase 1C normalizer, not by the skeleton.

- [x] **S8, R-side option state.** knitr options come from more than the chunk header: a
  setup chunk's `knitr::opts_chunk$set(...)`, `opts_template`, `opts.label`, `ref.label`
  and `opts_hooks` (`block.R`, `call_block`, `:9-92`). Capture the *resolved* options per
  chunk and show a fixture where the document text alone would give the wrong presentation
  (for example `opts_chunk$set(echo = FALSE)` in a setup chunk). This is what makes the
  parent epic's "presentation reads options from the document, never the record" rule
  insufficient for knitr (see KNR-6).

#### S8 decision

Run 2026-10-31, render-mode harness, chunk-hook observer recording every option, knitr 1.44,
1.50, 1.52 (same resolved values on all three), fixture `s8/01_setup_state` (YAML `params`, a
setup chunk with `opts_chunk$set`, `opts_template`, an `opts_hooks` function, then chunks
that use them). Verified by running.

- **Document text alone gives the wrong presentation.** `c_inherit` has no header options and
  resolves to `echo=FALSE`, because the setup chunk set it; the text-only rule would show its
  code. Likewise `c_template` (`opts.label='small'`) resolves `echo=TRUE` and
  `fig.height=2`, `c_ref` (`ref.label='c_inherit'`) resolves `echo=FALSE`, `c_params`
  (`eval=params$run`) resolves `eval=FALSE`, and `c_yaml_names` gets `fig.height=2.5` from the
  `opts_hooks` function applied to its `fig.width` of 5. None of these is in the chunk\'s own
  header. This is the failing fixture the plan asked for, so the parent epic\'s "presentation
  reads options from the document, never the record" rule is insufficient for knitr, as
  decision 11 settled.
- **Order of precedence, confirmed.** The `execute:` defaults that `execute.R:308-325` builds
  and passes through `rmarkdown::knitr_options` are lowest: the setup chunk itself resolves
  `echo=TRUE`, `fig.width=7` (the defaults) before its own `opts_chunk$set` runs. The setup
  chunk\'s `opts_chunk$set` outranks them for every later chunk (`c_inherit`: `echo=FALSE`,
  `fig.width=4`), and a chunk header outranks both (`c_header_wins`: `echo=TRUE`). A mid-document
  `opts_chunk$set` affects only later chunks (`c_mid_change` ran with `echo=FALSE`;
  `c_after_mid_change` resolved `echo=TRUE`). So the recorded resolved options already hold
  knitr\'s own order, and layering `execute:` over the record would reverse the precedence for
  every key a setup chunk set: **the settled "no document-scope layering" decision stands.**
- **Setup-chunk edits (KNR-3 decision 10).** Follows from the above: the setup chunk\'s unit
  hashes only its own text, so editing it changes that unit and leaves every later unit\'s
  recorded options at their last-run values. The test KNR-3 asked for is a Phase 3A item; it
  needs the record writer and was not run here.
- **`ref.label` and the code.** `c_ref` has an empty body, and the chunk hook sees its
  `code` as `c_inherit`\'s text with `ref.label` retained; the source in the document is
  empty. The structured unit keeps the authored (empty) `source`, and the code `evaluate()`
  received (S2: `code`) is `quarto_echoed_code` (KNR-3, 2026-11-03). The hash covers the
  authored text: the hash of an empty-bodied `ref.label` chunk does not change when the
  referenced chunk changes, so the record can go stale silently. That is still open: the safe
  rule is to treat a `ref.label` chunk as a fallback unit until the hash can include the
  referenced code, a **recommendation, not run**.
- [x] **S9, inline spans (feeds KNR-4).** Capture each inline expression\'s value from the
  `evaluate.inline` and `inline` hooks (`block.R:634-665`) in R, for both spellings
  (`r expr` and `{r} expr`), including `NULL`, invisible values, numbers that knitr formats
  (rounding, scientific notation, a big mark), `knit_asis` values and spans in a
  list-nested text group. Record: what knitr inserts for each value and where the escaping
  happens (`.QuartoInlineRender`, `execute.R:181-195`); whether a recorded text value
  reproduces the live markdown; whether the id can ride in the stage\'s shared unit list
  instead of being embedded in the expression; and what a unit must carry to apply the
  `NULL` rule per spelling. The result decides KNR-4\'s mechanism and recorded value.

#### S9 decision

Run 2026-10-31, render mode, html target, knitr 1.44, 1.50 and 1.52 (identical results),
fixture `s9/01_inline` written in the form q2\'s preprocessing hands to R: the classic
spelling as `r expr`, the brace spelling as `r .QuartoInlineRender(expr)`. 22 spans: both
spellings of simple values, `NULL`, `invisible()`, assignment, six numbers, markdown
specials, `knit_asis`, vectors, a span in a list item, one in a blockquote, one after a
chunk. The observer wraps the markdown `evaluate.inline` and `inline` hooks, and leaves the
markdown identical to the control on all three versions. Verified by running.

- **Mechanism: wrap `evaluate.inline` and `inline`.** `evaluate.inline` fires for **every**
  span, in document order, including the two that never reach the `inline` hook (`r NULL` and
  `r invisible(3)`: 22 `evaluate.inline` calls, 20 `inline` calls). So the wrapper creates a
  per-span entry at `evaluate.inline` (ordinal, expression text, value class) and fills its
  text from the `inline` hook, leaving it empty when that hook is not reached.
- **Recorded value: the string knitr inserts, not the raw value.** The inline hook formats
  numbers with the R session\'s options and knitr\'s markdown rules, and that formatting is
  not reversible: `1e-10` becomes `0`, `pi` becomes `3.1415927`, `1234567.891` becomes the
  LaTeX-math string `1.2345679\times 10^{6}`, `100000` becomes `10^{5}` in the document
  (`1e+05` before the hook), `c(1, 2, 3)` becomes `1, 2, 3`. A raw number in the record would
  need the formatting reimplemented and the session options (`digits`, `scipen`, `OutDec`)
  recorded; the string needs neither, and equals the live markdown by construction.
- **Where escaping happens.** Inside `.QuartoInlineRender`, before knitr sees the value
  (`execute.R:181-195`): the brace spelling of `"**bold** a_b"` reaches the hook already as
  `\*\*bold\*\* a\_b`, and the classic spelling is inserted as live markdown. A `knit_asis`
  value is escaped too under the brace spelling (`.QuartoInlineRender` tests only
  `is.character`), and bare under the classic one. The record holds the text after this, so
  the presenter inserts it verbatim and never escapes.
- **The `NULL` rule per spelling is applied in R, so a unit needs no spelling.** Classic
  `r NULL` inserts nothing (empty entry); brace `NULL` inserts the text `NULL` (the wrapper
  turns it into a string first); `r invisible(3)` inserts nothing. Because the entry exists for
  every span, the empty ones are recorded as empty strings rather than missing, which the
  count check needs.
- **The id does not have to be embedded in the expression.** The hooks run in document order,
  and in the run the order of calls equals the order of the spans in the text, across a list
  item, a blockquote and a chunk boundary. So the shared unit list can supply ids by position,
  with the expression text in the entry as a cross-check (strip the `.QuartoInlineRender(...)`
  wrapper before comparing). That removes EXR-12\'s embedding of an id in the expression, with
  the same count-mismatch rule as chunks (KNR-3 decision 4) when the scanner and R disagree.
  Not tested: spans inside `include=FALSE` text, two adjacent brace spans (which q2\'s preprocessing already evaluates only once), spans in headers
  or captions that knitr may process in a separate pass, and `child=` documents.
- **Result for KNR-4:** per-span `{ordinal, code, text}`, parallel to the chunk records, created
  at `evaluate.inline` and completed at `inline`; the presentation is substitution of `text`;
  the record is format-neutral only up to the markdown hook, since the hook is the same for all
  pandoc targets (the `\times 10^{6}` form appears on html; typst and docx were not run).

## Order of work

The real dependency order, which the phases below follow. Items on one line are independent.

1. Phase 1S spikes (S1 to S6, S8, S9, and S7 up to its substitution step; no parent code).
   The classifier (1A) can be written at any time.
2. Corpus manifest and generator (1D, first item), with its determinism test.
3. Control run: the corpus twice with capture off on the existing live path (needs R, the
   generator and a small driver; needs no capture and no presenter).
4. Normalizer rules, written from the control run, then the normalizer test and
   `control_run_is_identical_after_normalizer`.
5. Phase 1B capture, then `capture_does_not_change_live_output`.
6. Phase 1C mapper (needs the S2 table and the wire shape) and, once EXR-1 and EXR-2 are
   available, the baseline; then S7\'s substitution step.
7. Runner (needs 1B and 1C), report, Phase 1E.

## Phase 1A: tests first

Tests are written before the code they check, in the order of work above. Each states what
it compares; a test whose input is an S-spike result is written when that spike reports.

No R needed:

- [x] `classify_identical_bytes`, `classify_syntax_level_when_ast_equal`,
  `classify_semantic_when_html_equal`, `classify_blocker_when_html_differs`: the
  classifier over hand-written markdown pairs; each asserts the returned class equals the
  expected one. "Semantic but looks the same" means the rendered HTML is equal, so that
  test is written once S6 names the seam.
- [x] `corpus_generator_is_deterministic`: two generator runs with one manifest and seed
  give byte-equal files; a different seed changes only the sampled subset and filler text.
  Writable now (the generator is the next step).
- [x] `normalizer_rewrites_preserve_ids_and_temp_paths`: each normalizer rule has a test
  pair (input with the nondeterminism, expected output). Written rule by rule after the
  control run, so it has no pass condition before then; each rule is justified by a
  difference measured in that run.
- [x] `pieces_field_deserializes_with_and_without`: `KnitrExecuteResult` parses JSON with the
  new field (the S2 example) and without it, and the field is empty in the second case.
  (The type derives `Deserialize` only; there is no round trip unless `Serialize` is added,
  and this plan does not add it.) Written after S2 fixes the wire shape.
- [x] `pieces_to_nbformat_mapping` (as ten tests in `engine/knitr/pieces_map.rs`, one per table row or rule): the S2 table as unit tests over hand-built pieces; each
  asserts the produced output list equals the table\'s row. Written after S2.

Real R (skip when missing; each module starts with `#![cfg(not(target_arch = "wasm32"))]`):

- [x] `control_run_is_identical_after_normalizer`: two capture-off runs of every corpus
  fixture are identical after the normalizer; a leftover difference is a missing rule.
- [x] `capture_does_not_change_live_output`: the markdown with capture on is identical,
  after the normalizer, to the markdown with capture off, on every corpus fixture. Raw bytes
  cannot be required (random ids differ between any two runs). Fixtures call `set.seed()`
  and avoid `Sys.time()`, so user-code noise is gone before the normalizer starts.
- [x] `capture_one_unit_per_chunk_in_order`: the captured ordinals (Phase 1B) equal the
  positions of the cell units from the shared scanner (EXR-2), in the same order, on a
  nested, a list-nested and an `include=FALSE` fixture. Waits for EXR-2.
  *Done in `knitr_corpus_runner.rs`, on `s7/01_structure` (blockquote, list, adjacent,
  `include=FALSE`, first and last) and `s1/02b_include_false_in_list`: ordinals are 1..n and
  the labels equal the scanner\'s cell labels in order. It passed on the first run; it was not
  seen failing first.*
- [x] `capture_include_false_and_cache_hit_shapes`: asserts the shapes S1 and S4 recorded
  (which pieces exist for `include=FALSE`, and what a cache hit yields). Written after S1
  and S4.

## Phase 1B: capture (R side)

- [x] Before coding, S2\'s wire shape (a JSON example and its Rust type) is in this plan.
- [x] Implement the S1 capture mechanism in a new file beside `resources/rmd/patch.R`,
  never as an edit to its existing `sew` or `wrap_asis_output` overrides, enabled only when
  the engine call asks for it, restoring the original on exit.
  *Done: `resources/rmd/capture.R`. `execute()` takes a new `capture` argument (from
  `KnitrExecuteParams.capture`, omitted from the wire when false; `rmd.R` passes it) and
  sources the file only then. The evaluate-hook and chunk-hook wrappers are installed in the
  knitr options list that `execute()` hands to rmarkdown, so rmarkdown restores the hooks
  itself; nothing global is changed. It also installs the unconditional `cache = FALSE`
  options hook (S4). `patch.R` is untouched.*
- [x] Return per-chunk records `{ordinal, label, code, options, pieces, files}` next to
  `markdown` from `execute.R`, in the shape S2 fixed. The `ordinal` is the chunk\'s 1-based
  position in knitr\'s own chunk sequence and is harness-local: it is only compared with the
  scanner\'s unit order (`capture_one_unit_per_chunk_in_order`); the record id comes later
  from the stage\'s shared unit list (EXR-4, EXR-7, KNR-3). An unrecognized shape yields a
  fallback marker, not an error.
  *Done. Unrecognized pieces and non-R engines give an `other` piece (the chunk becomes a
  fallback unit). `eval=FALSE` and blank chunks, which never reach `evaluate()`, give a
  source-only record from `options$code`.*
- [x] Files per chunk (figures), two steps: R writes knitr\'s own paths into `files`
  (R side, new); Rust reads the bytes through the existing `collect_capture_files`
  (`crates/quarto-core/src/engine/capture_files.rs:62`). *R side done: `files` holds the
  chunk\'s plot files (all devices) and its `include_graphics` paths, and each `plot` piece
  names the first device\'s file. The Rust call is made by the consumer of `pieces` (the
  Phase 1C baseline support module), not in `engine_execution.rs`: the stage serializes the
  generic `ExecuteResult`, which does not carry `pieces`, so there is nothing to collect there.*

**Findings while building it** (verified by running unless noted):

- `knitr`\'s `plot_files` holds each path twice and one entry per device, so the files of the
  n-th plot cannot be found by dividing by the device count. `capture.R` groups the chunk\'s
  entries by file stem, in first-seen order, and takes the first of each group (my first
  version split by device count and gave `keep_all` the files 1, 1, 2 for three plots;
  `capture_several_plots_each_name_their_own_file` now covers it).
- `include_graphics()` output has class `c("knit_image_paths", "knit_asis")`, so
  `knit_image_paths` must be tested before `knit_asis` (the S2 table row stands).
- The code `evaluate()` receives has no trailing newline (`options$code` joined with `\n`),
  while source pieces end in one; the mapper concatenates source pieces, so the record\'s
  `code` and the joined source differ by that final newline.
- knitr normalizes `cache=FALSE` to `0`, so the recorded option is `"cache": 0`.
- `execute.R` serializes with `jsonlite::toJSON(auto_unbox = TRUE)` at default digits, so
  numeric options carry four significant decimals. Not a problem for the options seen so far.
- Capture on versus off, markdown, run by hand through `rmd.R` with a hand-built request
  (scratch driver, not committed): byte-identical on 21 fixtures (S1 01-06, 09-13, 15, 16;
  S2 01, 02, 04-08); the two widget fixtures (S2 03, 09) differ only in random hex ids
  and are identical once those are masked. S1 07, 08 (abort) and 14 (cache, whose behavior
  changes by design under the cache-off rule) were not run. The committed gate
  `capture_does_not_change_live_output` still waits for the normalizer.
- Widget `knit_meta`: the `serializeJSON` string in each widget piece unserializes to 8
  `html_dependency` objects (S2 09: normal, hide, include=FALSE, echo=FALSE, all four
  carry them at capture; the presenter applies the `results='hide'` rule).
- Tests: `engine/knitr/capture_tests.rs` (10, real R). `cargo clippy -p quarto-core
  --all-targets -- -D warnings` clean on the host. `nextest -p quarto-core`: 6876 of 6884
  pass; the 8 failures are `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`
  (vendored `reset.css` differs from `node_modules`) and seven `julia_engine_e2e` tests
  ("Ensuring an updated julia server environment failed"). Neither touches the knitr path;
  I did not run them on the base commit to prove they fail there too.

## Phase 1C: Rust side

- [x] `KnitrExecuteResult.pieces` (`#[serde(default)]`), and the `collect_capture_files`
  call for `files`. *Done: the field and the S2 types (`engine/knitr/types.rs`, commit 03cd91503).
  The `collect_capture_files` call is made where `pieces` are consumed, in the baseline
  (`knitr_corpus_baseline.rs`).*
- [x] The S2 mapper from pieces to nbformat outputs, producing the EXR-1 presenter\'s
  input types directly (no record writer yet; that is KNR-3). Waits for EXR-1.
  *Done: `pieces_to_cell` in `engine/knitr/pieces_map.rs`, in `src/` (not a test module) so
  KNR-3 reuses it; `engine/mod.rs` re-exports it and the piece types, the only visibility
  change (Gordon chose this, 2026-11-01). A fallback chunk (an `Other` piece, an unreadable
  figure, a `pdf` figure) returns `None`. Not in the table, my choices: a warning keeps the
  text as captured (no added newline); `status` is always `Ok` even with an `error` piece;
  the source anchors are not carried (the recorded divergence).*
- [x] The baseline: present each captured chunk with the unmodified presenter and the
  figure sink EXR-1 provides for files (`FsFigureSink`), one call per source group or per
  chunk as S2 decided, and join with the document\'s prose as knitr\'s group structure
  dictates, producing the candidate markdown. Waits for EXR-1.
  *Done: `tests/integration/knitr_corpus_baseline.rs` (5 tests, no R). The scanner\'s cell
  units give the splice offsets; each chunk goes through `pieces_to_cell` and one `render_cell`
  call with `FsFigureSink`, with the figure bytes read through `collect_capture_files`; later
  lines carry the opener\'s container prefix and indent. Visibility comes from the recorded
  options, logical values only (`include`, `echo`, `output`, `warning`). A chunk-count
  mismatch (a `child=` chunk) or a fallback chunk makes the whole document a fallback.*

## Phase 1D: corpus and harness

- [x] Manifest and generator, deterministic, driven by these axes. The manifest is a TOML
  file `manifest.toml` in `crates/quarto-core/tests/fixtures/knitr-corpus/` (the directory
  is the epic\'s decision 16; it does not exist yet), one table per fixture family listing
  its axis values and the S1 fixtures under `s1/`. The seed controls only the sampled
  combinations of a quick subset run and any generated filler text; the full corpus does not
  depend on it, and one manifest plus one seed gives byte-identical files. Fixtures call `set.seed()` and avoid `Sys.time()` and temp paths in output:
  - *Options:* `echo` (logical and numeric), `output`, `warning`, `message` (logical,
    `NA` and numeric), `include`, `eval` (logical and numeric), `error`, `collapse`,
    `prompt`, `comment`, `results` (`markup`, `asis`, `hide`, `hold`), `fig.cap`,
    `fig.subcap`, `fig.alt`, `fig.keep` (`none`, `high`, `all`, `first`, `last`),
    `fig.show` (`asis`, `hold`, `animate`, `hide`), `fig.width`, `dev`, `class.*`, `attr.*`,
    layout, `fenced.echo`, `label`, and unknown options (forwarded as attributes).
  - *Quarto cell options:* `code-fold`, `code-summary`, `code-line-numbers`, `lst-*`,
    `panel`, `column`, `keep-hidden`, and the `#|` YAML-form of every option above.
  - *Setup-chunk state:* `opts_chunk$set`, `opts_template`, `ref.label`, `opts_hooks`, and
    a `knit_hooks$set` override (S8 also uses these).
  - *Target format:* HTML, Typst, docx and gfm, and `prefer-html`, because `kable` and
    `df_print` pieces change by format (LaTeX targets are fallbacks, epic decision 13).
  - *Content that stresses markdown:* backtick runs in output (fence length), lines
    starting with `:::`, `---`, `#`, `>` or list markers, trailing whitespace, tabs, ANSI,
    Unicode, empty output, very long lines, output with and without a final newline.
  - *Structure:* chunks nested in lists and blockquotes, adjacent chunks, chunks first
    or last in the file, several figures in one chunk.
  - *Rich output:* kable and `df_print` tables, an htmlwidget, a `knit_asis` object,
    `include_graphics`, recorded plots on more than one device, and (skipped when the
    package is missing; `DT`, `rgl`, `formatR` and `pagedtable` are not installed locally)
    `rgl`, `html_screenshot` and `knit_embed_url`.
  - *Engines and edges:* a shell chunk, a SQL chunk, handled-language fences (`ojs`,
    `mermaid`, `dot`), `child=`, and `cache` at levels 1, 2 and 3 plus the document-level
    `execute: cache`.

  *Done: `manifest.toml` (sweeps, verbatim documents, the committed S-spike directories) and
  the generator in `tests/integration/knitr_corpus_support.rs`, tested by
  `knitr_corpus_generator.rs` (4 tests, no R). The SQL chunk, `child=` and cache levels 1 to 3
  come from the committed S1 and S5 fixtures, not from the generator. The `error=FALSE`
  abort cases are S1 07 and 08, flagged `expect.abort`. The generator emits `.qmd` and the
  committed files stay `.Rmd`. `toml` was added to quarto-core\'s dev-dependencies.*
- [x] Control run (item 3 of the order of work): a small driver renders every corpus
  fixture twice with capture off, writes the two outputs side by side and lists every
  difference. The normalizer rules come from this list; the list is written into this plan.

  *Done (2026-11-01, knitr 1.50 on this machine, R 4.3.2, html only except the
  `rich-output` documents, 213 (fixture, format) runs, two capture-off runs each).
  Verified by running (logs in the session scratchpad, not committed).*

  - First run (before the outcome handling): 9 of 213 runs flagged. Four were aborts that
    the comparison flagged only because it demanded success (`eval=1`, two forms, and
    two `class.error` fixtures); three were missing packages (`gifski` for
    `fig.show="animate"`, two forms; `RSQLite` for `s5/01_odd_chunks`); two were real
    differences (the widget fixtures, S2 03 and 09). A render that aborts with the same
    message twice is a consistent outcome, and a missing package makes the fixture
    unrunnable here, so the comparison now has three outcomes (ran, aborted, unavailable).
    With that, second run (before the `class.error` fix below): 204 compared, 6 aborted
    identically (those four plus S1 07 and 08, which abort by design), 3 skipped for a
    missing package. After the fix the two `class.error` runs compare instead, so the
    expected split is 206 / 4 / 3; the passing run did not print its counts, so that split is
    inferred, not observed.
  - Differences, all in the two htmlwidget fixtures: (1) the random `htmlwidget-<20 hex>`
    element id; (2) plotly\'s `visdat` id, hex of an object address, 10 to 13 digits with or
    without a leading `f` (my first rule assumed the `f` and was wrong on the second run,
    so it keys on `"visdat":{"<id>"` instead). Nothing else differed: no temp paths and no
    timestamps appear in the output of any fixture, so no path rule exists yet.
  - The aborts: S1 07 and 08, `eval=1` in both forms (`x` is not found because only the
    first expression is evaluated), and two `class.error` fixtures that I had written
    without `error=TRUE` (fixed afterwards with the sweep\'s `fixed` key, which the
    generator test covers; the gate runs after that fix show them compared, not aborted).
  - After the two rules: both gates pass, `control_run_is_identical_after_normalizer` and
    `capture_does_not_change_live_output`. Wall time rose from 68 s (first run, nothing
    else running) to 330 and 447 s (the two tests together with other cargo work), so a
    full run is minutes, and the seeded `sample` exists for quick runs.
- [x] Runner: for each fixture, one R run gives the oracle markdown and the captured
  pieces together (Phase 1B returns both), and the baseline presenter produces the
  candidate. Normalize, and compare at the three levels, recording the class and, for
  non-identical cases, a minimal diff. The AST level ignores source locations and
  attribute order. Where the code lives: the classifier, normalizer, generator driver and
  runner are test-support modules under `crates/quarto-core/tests/integration/`
  (registered in `main.rs`, alphabetized), and the R-side files under the corpus
  directory; none is shipped in `src/`. The helpers in `engine_output_parity.rs`
  (`post_engine_ast`, `semantic_classes`, `assert_blocks_parity`) are private `fn`s;
  promote the ones the runner needs to `pub(crate)` (a one-line change each, with
  `engine_output_parity.rs` otherwise untouched). `post_engine_ast` runs the legacy
  `record_capture` path, which EXR-5, EXR-6 and EXR-17 change or delete, so prefer the S6
  seam over it, and decide per helper in S6. *Intentional divergence* entries live in a
  keyed allowlist file in the corpus directory (fixture, construct, rationale), so a rerun
  re-applies them. Any key or expected string containing a path is built with `Path::join`
  when it is a local path, and uses `/` when it is a wire-format path (`_files/figure-html/...`
  in markdown).
  *Done: `knitr_corpus_{support,run,normalizer,classify,baseline,runner,control,q1}.rs`.
  Deviations: the report entry point is the test `corpus_report` behind `KNR_REPORT`, not
  an xtask (the epic notes it under decision 16); the helpers in `engine_output_parity.rs`
  were not promoted to `pub(crate)`, because the runner uses the S6 seam directly (`pampa`
  read plus the native writer, and a fixed-output engine through `engine_registry_override`),
  so that file is untouched; no intentional-divergence allowlist exists yet, because none has
  been decided (the report says so, and KNR-2 adds the reader with its first entry).*
- [x] Report: a table of counts per class, per option group and per content group; the
  fallback rate per fixture; the top differences by frequency; written to
  `claude-notes/research/` (and a short summary pasted into this plan).
  *Done: `claude-notes/research/2026-11-01-knr-01-corpus-report.md`. Summary: 213 cases;
  202 compared, 0 identical, 48 syntax-level, 0 semantic, 154 blockers (78 with a chunk that
  has two or more source groups); 4 fallbacks (1.9%), 4 aborts, 3 unavailable (missing
  package); the same classes on knitr 1.44, 1.50 and 1.52. The top differences are
  interleaving, options not carried as attributes, knitr text hooks, dropped
  `text/markdown` display data, error text and the widget wrapper class; they are attributed
  by reading representative diffs, not computed, and the JSON from `KNR_REPORT` holds every
  diff.*
- [x] Quarto 1 oracle: decide and write here how Quarto 1 output is gathered. Default: when
  `quarto` on `PATH` reports a 1.x version, render each fixture with it into a scratch
  directory; skip the Q1 step when absent. This feeds the next item and needs no one but
  the executor to decide.
  *Decided and built (`knitr_corpus_q1.rs`): when `quarto` on `PATH` reports `1.x` or `99.9.9`
  (a quarto-cli development checkout; this machine has `99.9.9`), `quarto render doc.<ext>
  --to html -M keep-md:true` and read `doc.html.md` without its front matter; compare it with
  q2\'s current knitr output (not with the baseline), html cases only, behind `KNR_Q1`.
  Result: 189 html cases compared, 183 rendered under Q1: 135 identical, 2 syntax-level, 46
  blockers; 6 Q1 failures (Q1 rejects numeric `echo`, `eval`, `warning` and `message` in
  YAML form).*
- [x] Q1 gaps (epic decision 20): for each difference classified as a q2 gap against
  Quarto 1, search the skein for an existing strand (`braid list`, filtered by the feature
  name; see the braid skill), and file a new strand, with a `discovered-from` link to this
  epic\'s work, only if none exists. Record the strand ids in the report.
  *Done: the 46 blockers are one gap, filed as bd-x8hqtvm0 (Q1 writes `{width=N}` on knitr
  plots from `fig.width` times dpi, halves `include_graphics` width, and sizes widgets
  `100%` by 464px; q2 does none). A skein search for width, figure, image and widget found no
  existing strand. No `discovered-from` link: epic decision 18 gives the epic no strand.*

## Phase 1E: first run and decisions

- [x] Run the corpus on the minimum and the latest supported knitr. Classify by hand
  every difference that is not *identical* or *syntax-level*.
  *Done on 1.44, 1.50 and 1.52 (scratch libraries built by `scratch-libs.R`, selected with
  `KNR_R_LIBS`); the class of every case is the same on all three. The 1.44 run found a bug
  in `capture.R` (`xfun::str_wrap` does not exist under xfun 0.40), fixed. The 154 blockers
  are classified by cause from representative diffs in the report, not one by one by hand;
  that part of the item is only partly met.*
- [x] Check that each spike S1 to S9 has its `#### Sn decision` subsection. The decisions
  the numbers force: capture point and minimum version (S1), the S2 table and wire shape,
  the S3 option serialization rule, the S4 cache rule, the S5 fallback list, the S6 seam,
  the S7 skeleton verdict, the S8 option-state rule and the S9 inline mechanism.
- [x] Update the epic: set the KNR-2 to KNR-7 scope and estimates from the report, add
  the new plan files, and answer or re-state its open questions.

## Exit criteria

- The capture is opt-in, and with R present the live output is identical to before after
  the normalizer on every corpus fixture; the capture-off control run is empty after the
  normalizer (both are the named tests passing).
- The report exists at a path named in this plan, with every non-identical case classified
  (no case without a class) and blockers listed. Corpus coverage: every axis group in
  Phase 1D has at least one fixture, and every S1 fixture is present.
- Each of S1 to S9 has a `#### Sn decision` subsection in this plan.
- The epic\'s dependency table, estimates and open-question list match the report: no
  open question the report answers remains, and the scope lines of KNR-2 to KNR-7 cite the
  report\'s numbers.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` are green; pass and skip counts are reported against
  the live baseline. `cargo clippy -p quarto-core --all-targets --target
  x86_64-pc-windows-msvc -- -D warnings` is green (or the PR says the crate cannot
  cross-check).
- Any new integration tests live under `tests/integration/` and are registered in its
  `main.rs`, alphabetized (the repo\'s integration-test rule).

## Risks and open questions

- **Corpus size versus runtime.** Each fixture needs an R run; budget and a seed-based
  subset for quick runs (the seed samples axis combinations; see the manifest item) are
  decided in Phase 1D.
- **No R in CI** (epic decision 17). The R-dependent tests are written CI-ready (they skip
  without R) but CI stays disabled, so the differential run is local; the classifier and
  normalizer tests still run everywhere.
- **The oracle itself.** Some knitr output is a quirk we would not want to copy; those
  become *intentional divergence* entries, each with a short rationale, not blockers.
  Per the epic's decision 9, Quarto 1 is a second reference: where q2's current output
  lacks something Quarto 1 does, record it as a q2 gap. How Quarto 1 output is gathered
  (needs a Quarto 1 install, so it skips when absent) is a Phase 1D checkbox.
- **Capture point regret.** If capture before the filters proves too fragile across
  knitr versions, S1 may conclude with a split (capture after the filters at `sew`, with a
  larger hash exclusion list, falling back to fallback units where the shape is
  unrecognized); that changes the epic's decision 6 hash rule and must be raised with
  Gordon, not decided silently here. The same holds if no mechanism in S1 captures without
  changing R's behavior.

## References

- `crates/quarto-core/src/engine/knitr/{mod.rs,types.rs,resources/rmd/{hooks.R,execute.R,patch.R,rmd.R}}`
- `external-sources/knitr/R/{block.R,output.R,engine.R}` and `NAMESPACE`
- [EXR-1](2026-10-28-exr-01-presenter-extraction.md),
  [EXR-2](2026-10-28-exr-02-unit-scanner.md),
  [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md),
  [EXR-12](2026-10-28-exr-12-knitr-k2-inline.md)
- bd-sr73 (record size and the figure decision)
