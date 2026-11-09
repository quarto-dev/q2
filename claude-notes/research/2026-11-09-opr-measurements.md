---
title: 'OPR-0c: measurements and spikes (routes, cost, cache, figures, marimo)'
date: 2026-11-09
description: 'What a plain render of Julia and knitr does today, what an in-memory record costs, whether knitr cached chunks can fall back with no record, how the figure crossrefs behave on each route against Quarto 1, and where marimo stands. Evidence for the plain-render plans OPR-7a, 7b and 7c.'
---

**Plan:** [OPR-0c](../plans/2026-11-08-opr-0c-measurements-and-spikes.md) ·
**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md).
Binary: `target/debug/q2` built from `feature/one-presenter` at the claim commit (`9680c4efa`), R 4.3.2 with knitr,
Julia 1.12.7, Quarto 1 (`quarto` on PATH, 99.9.9). No product code changed. Marks: (ran), (read), (not done).

## 1. Plain-render route (ran, plus read)

Documents: one cell each (`x = 1 + 1` then `x`; the knitr one in R). Two renders each: plain `q2 render`, and the same
with `execute: {record: true}` in the front matter.

| Document | Wrapper in the HTML | Record file |
|---|---|---|
| Julia, plain | `div id="cell-1" class="cell" data-quarto="{unit, language, file, line}" data-execution_count="1"` | none |
| Julia, `record: true` | `div class="cell"` | `.quarto/execution-records/j1.ipynb` |
| knitr, plain | `div class="cell"` | none |
| knitr, `record: true` | `div class="cell"` | `.quarto/execution-records/k1.ipynb` |

- **Julia:** the plain route is the engine's own `toMarkdown` (the `id`, `data-quarto` and `data-execution_count`
  attributes are its markup); the recorded route is the core presenter. Confirms `core_presents`
  (`engine/ts_record.rs`, `engine == "julia" && want_record && units non-empty`).
- **knitr:** the two HTML files are identical for this document, so the HTML cannot say which route made it. The
  evidence for the route is the record: no `.quarto/execution-records` on the plain render, one on the recorded render,
  and R's finished markdown (`k1.knit.md` in the log) on the plain render. The plain route is R's markdown; the recorded
  route is the presenter. (read for the code path, `plan_capture` in `engine/knitr/target_route.rs`; ran for the files.)

## 2. Julia plain against recorded (ran)

Three documents, both routes, HTML between `main` and `/main`, base64 image data and absolute paths normalized.

| Document | Differences, complete list |
|---|---|
| Plot (`using Plots; plot(1:5, (1:5).^2)`) | Wrapper: `id="cell-1"` and `data-quarto` (plain only); `data-execution_count="1"` on the output div (plain only). |
| Error (`error: true`, a `println` then `error("boom")`) | Wrapper attributes as above; no `data-execution_count`. The error trace's last line differs by two (`err.qmd:9` against `:11`), but only because the recorded copy of my fixture has two extra front-matter lines. Not a route difference. |
| Several expressions, prose between two cells, the second with `echo: false` | Wrapper attributes on both cells (`cell-1` and `cell-3` ids, so the id is the unit-ordinal-derived number, not a per-cell counter); `data-execution_count` on the outputs of cell 1 and cell 2 (plain only). |

Nothing else differed: the same code, the same output text, the same classes. **Figure names did not differ here because
the figures are inline base64; the claim that names move to `cell-N-output-M.png` was not exercised (not done).**
The only differences are three attributes on the wrapper and the output div.

Fixture note: `Plots` is not in the runner's default environment; each plot document got a copy of
`~/.julia/environments/v1.12/{Project,Manifest}.toml` beside it.

## 3. Cost of an in-memory record (ran, with caveats)

Documents of 1, 10 and 100 cells, each cell: a vector, a summary or a sum, and a plot; plain against
`execute: {record: true}`; one untimed warm-up render, then three timed ones; `/usr/bin/time -l` (wall and maximum
resident set of the `q2` process; the R and Julia children are not in the memory figure). Medians of three:

| Engine | Cells | Plain wall | Record wall | Delta | Plain RSS | Record RSS | Written under `.quarto` (record run) |
|---|---|---|---|---|---|---|---|
| knitr | 1 | 1.78 s | 1.95 s | +0.17 s | 109 MB | 125 MB | 48 KB |
| knitr | 10 | 2.75 s | 3.25 s | +0.5 s | 159 MB | 159 MB | 385 KB |
| knitr | 100 | 4.36 s | 6.20 s | +1.84 s (+42%) | 194 MB | 192 MB | 3.8 MB |
| Julia | 1 | 12.15 s | 8.81 s | noise | 67 MB | 66 MB | 160 KB |
| Julia | 10 | 30.2 s | 28.1 s | noise | 67 MB | 75 MB | 1.5 MB |
| Julia | 100 | 104.7 s | 154.5 s | about +50 s (+47%) | 233 MB | 225 MB | 14.6 MB |

Plain renders wrote only the manifest (4 KB) to `.quarto` in every row.

- **Knitr:** the delta grows with the record size, about 18 ms a cell at 100 cells in a debug build, 42% of the
  render. Peak memory of the `q2` process did not move measurably (the figures are written once to the record and the
  copy is freed).
- **Julia:** the 1- and 10-cell rows are dominated by the Julia server (compilation and the warm worker); their deltas
  are inside the spread (7.2 to 11.9 s at 1 cell for the recorded runs) and prove nothing. Only the 100-cell row separates,
  at about +50 s, with a 14.6 MB record. Treat the 100-cell Julia figure as a rough bound: three runs, the spread
  was 133 to 165 s on the record side.
- **What the delta contains:** `serde_json::to_value(&record)` for the observer, plus writing the file. I could not separate
  the two, because a render with `record: true` does both. No setting builds the record without the serialization:
  `wants_execution_record` (preview recorder, trace observer, hub) all reach the same `want_record` branch of
  `engine_execution.rs`, which serializes and emits the value before it looks at `record_requested`. (read.)
  A plain render that builds the record for the presenter alone would pay the coalesce but could skip both the
  serialization and the file; that code does not exist today.
- **Verdict on "expensive":** for knitr, measurable but modest (a fraction of a second per ten cells in a debug build).
  For Julia, not resolvable at this noise; the 100-cell row suggests roughly the same proportion, and Julia's own
  time is large. This is not obviously the "expensive" outcome of the decision table.

Caveats: debug (optimized plus debuginfo) build; one machine; the first Julia sweep ran before a disk-full event, and the
100-cell Julia rows were rerun after (the first attempt, taken while the shared Julia server was in a bad state,
failed in 0.1 s and was discarded). `ArgumentError: startpath must be non-empty` out of the runner's
`evaluate_raw_cells!` appeared on every Julia render for a few minutes after the disk filled, then cleared when the
shared server restarted. I did not diagnose it; it is not a result of this plan.

## 4. Cached knitr chunks, the OPR-7b spike (ran at the knitr level; not through q2)

Product code may not change in this plan, and `capture.R` is embedded in the binary, so the prototype is plain R with
knitr: an `evaluate` hook wrapper plays the recorder, the cache stays **on**, and a chunk whose `cache` option is
truthy is not recorded. Each document is knitted twice into the same directory (a miss, then a hit); "ran" is whether
the `evaluate` hook fired for the chunk, "recorded" is whether the prototype would have recorded it.

| Case | Miss: ran / recorded | Hit: ran / recorded | Markdown, miss against hit |
|---|---|---|---|
| (a, c) one chunk `cache=TRUE` (level 3) | a / none | none / none | identical |
| (c) `cache=1` | a / none | none / none | identical |
| (c) `cache=2` | a / none | none / none | identical |
| (b) mixed: two live chunks (one with a figure), two cached chunks (one with a figure, one text) | all four / the two live | the two live / the two live | identical; the cached figure (`c1-1.pdf`) is present on the hit |
| (d) `dependson='d1'` | both / none | none / none | identical |
| (d) `autodep=TRUE` on two chunks | both / none | none / none | identical |
| (e) document-level `cache = TRUE` (every chunk) | both / none | none / none | identical |

Findings:

- On a hit, knitr never reaches the chunk's `evaluate` hook for a cached chunk at any of the three levels, so such a
  chunk is not recorded on a hit even without the opt-out; the opt-out is only needed for the miss run, so the miss and
  hit runs of a document present the same way.
- A mixed document is correct at the knitr level: the live chunks are recorded, the cached chunks fall out of the
  record, and the cached chunk's output (text and figure file) reappears unchanged on the hit.
- `dependson` and `autodep` behave like plain `cache=TRUE`: the whole dependent set falls out of the record together.
- A document-level `cache: true` makes every chunk a candidate, so the whole document would fall back to R's markdown.
  That is quiet and correct, but it means the document gets no record at all (the same as today's cache-refused case).

**Not done:** the splice of a mixed document through q2 (the presenter's output for the recorded chunks together with
R's markdown for the cached chunks, and figure numbering across the two), and the knitr epic's pinned test
`a_cache_written_without_capture_is_not_replayed_by_a_recorded_run`. The prototype shows that the knitr side can be made
consistent; it does not show that q2 composes the two correctly. That is OPR-7b's own work and its first test.

## 5. Figure matrix (ran, two engines of three)

One document per engine, with three cases: one figure with `fig-cap` (`fig-one`); two figures with `fig-subcap` and
`layout-ncol: 2` (`fig-two`); a table with `tbl-cap` (`tbl-one`); and the references `@fig-one`, `@fig-two`,
`@fig-two-1`, `@fig-two-2`, `@tbl-one`. Routes: q2 plain, q2 `record: true`, Quarto 1 (`quarto render`, real kernels).
Text of each reference:

| Reference | Quarto 1 (knitr and Julia) | q2 knitr, plain and recorded | q2 Julia, plain | q2 Julia, recorded |
|---|---|---|---|---|
| `@fig-one` | Figure 1 | Figure 1 | Figure 1 | Figure 1 |
| `@fig-two-1` | Figure 2 (a) | Figure 2 | Figure 2 | unresolved |
| `@fig-two-2` | Figure 2 (b) | Figure 3 | Figure 3 | unresolved |
| `@fig-two` | Figure 2 | Figure 4 | Figure 4 | unresolved |
| `@tbl-one` | Table 1 | Table 1 | Table 1 | Table 1 |

- On q2's knitr and Julia-plain routes the subfigures are numbered as independent figures (2 and 3) and the group gets
  the next number (4); this is the defect of `bd-au5gts6e`, with different numbers from the epic's single case
  ("3, 1, 2") because the fixture differs. Both knitr routes were identical (plain against recorded).
- **New:** Julia **recorded** leaves the group and both subfigure references unresolved (`?fig-two?`), where Julia plain
  resolves them (wrongly, as above). So for Julia the plain-render flip (OPR-7a) would move the group's refs from wrong
  numbers to unresolved. This is the `bd-gbaykhth` shape (the Jupyter-shaped route), now seen on Julia. No new strand
  opened; the two cases have strands.
- **Stored Jupyter notebook: not done.** I did not build a notebook fixture; the earlier one-case result in the epic is
  (relied): Quarto 1 resolves, q2's Jupyter-shaped route leaves all three unresolved.
- Does a route change when OPR-1a and OPR-1b land? The note cannot say from a run; the refactor is meant to keep bytes
  unchanged (the epic's constraint), so the recorded Julia result should not move, and this table is the baseline to
  re-run to check it.

## 6. Marimo (read, one attempted run)

`core_presents` is Julia-only (`engine == "julia"`), so marimo output never goes through the presenter; the preview
splice for marimo takes the legacy capture path (`CapturePayload::Legacy`, `engine/capture_splice.rs`, which handles
marimo's unwrapped `{=html}` island). The marimo integration tests that need the live render are `#[ignore]`d with a
note that the live render fails today (`bd-mts59whz`, read in `marimo_records.rs`). My attempted render (`engine: marimo`,
a `python` cell and then a `marimo` cell, in a scratch directory) did **not** execute the cell: the code came out as a
plain code block with no engine output, in both spellings. I did not find out why (no marimo in this Python
environment is likely), so the run is (not done) and the paragraph rests on reading. Marimo stays a documented
exception to the epic (decision D12).

## Effects on the plans

| Result | Plan | Effect on its boundary |
|---|---|---|
| Plain Julia and knitr take the engine route; a recorded run takes the presenter (sections 1 and 2) | OPR-7a, 7c | Unchanged. For Julia the difference between routes is three wrapper and output attributes, which OPR-7c's parity gate must account for (`id`, `data-quarto`, `data-execution_count`), and the audit must decide if `id="cell-N"` is relied on (a crossref or a CSS target); I did not check. |
| Cost (section 3) | OPR-7a | Stays as written: knitr about +42% at 100 cells (0.5 s at 10) in a debug build, Julia not resolvable and plausibly similar. Not "expensive" in the decision table's sense. OPR-7a should still avoid the extra serialization and the file write when no record was requested (there is no code path for that today, so it is the plan's own work, as already written). Re-measure in a release build in OPR-7a. |
| Cache (section 4) | OPR-7b | Boundary holds at the knitr level: keeping the cache on and not recording cache-asking chunks gives identical miss and hit output at all three levels, `dependson`, `autodep` and a document-level cache. A mixed document through q2's splice is untested, and a document-level `cache: true` means no record at all. OPR-7b needs no Gordon decision on this evidence, but its first test must be the mixed document through q2. |
| Figures (section 5) | OPR-7c | A new Julia-specific item: the recorded route leaves `fig-subcap` group references unresolved where the plain route resolves them (wrongly). OPR-7a flips Julia plain to the recorded route, so the gate should either record this as a known regression with the strand `bd-gbaykhth` or hold the flip for Julia until the group refs resolve. This is a finding for Gordon: it could be a blocker for OPR-7d. |
| Marimo (section 6) | none | Exception stands (D12). |

Files: scratch fixtures and logs stayed in the session scratchpad; nothing else was added.
