---
title: 'OPR-0c: measurements and spikes (cost, cache, figures, plain-render routes)'
date: 2026-11-08
description: 'Produces the evidence that decides the shape of the plain-render plans: what an in-memory record costs, whether knitr cached chunks can fall back with no record in a mixed document, how the figure matrix behaves on each route against Quarto 1, and what a plain Julia and knitr render really does today. Notes only; no product code.'
status: in-progress
depends-on: []
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/0c-measurements` · **Lane D** (`workspace-6`) · **Blocks:** OPR-7b; informs OPR-7a, 7c, 7d.
**Needs:** R 4.3 with knitr, Julia 1.12 (works offline), the `quarto` on PATH (Quarto 1 dev checkout, 99.9.9) for
comparisons, and `target/debug/q2` built in your checkout.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/0c-measurements feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/0c-measurements
```

## Goal

Answer five questions with runs, not reading. The deliverable is one note,
`claude-notes/research/2026-11-xx-opr-measurements.md` (use the real date), plus a short "Effects on the plans"
section. **Do not change product code.** Scratch fixtures and scripts live in your scratchpad; copy only the final
fixtures the note names into the note\'s own directory if they are small.

## Checklist (every box is `[unattended]`)

- [ ] **Plain-render route, confirmed by running.** Render a small Julia document and a small knitr document with a
      plain `q2 render` and with `execute: {record: true}`. Record which route each took (the evidence: for Julia
      whether the engine\'s `toMarkdown` or the core presenter produced the markup; for knitr whether R\'s markdown
      or the presenter did; a record file written or not). (read, 2026-11-08) The code says a plain render takes
      the engine-owned route for both (`core_presents`, `engine/ts_record.rs:500`; `plan_capture`,
      `engine/knitr/target_route.rs:78`). Confirm.
- [ ] **Julia plain against recorded.** Diff the two HTML outputs (strip image data). (relied, one tiny document)
      The plain route emits `<div id="cell-1" class="cell" data-quarto="{...file,line}" data-execution_count="1">`
      and the recorded route emits `<div class="cell">`; figure names move from the engine\'s to
      `cell-N-output-M.png`. Produce the complete list of differences on at least three documents, including one with
      a plot, one with an error, and one with several expressions.
- [ ] **Cost of an in-memory record.** (read) Setting `want_record` makes the engine stage serialize the whole record
      (`serde_json::to_value(&record)` near `engine_execution.rs:882`) and emit it to the observer, a second copy of
      every inline base64 image; the unit scan and id fill already run on every render. Measure on knitr and
      Julia documents of increasing size (1, 10, 100 cells, with plots): wall time and peak memory, plain against
      `record: true`, three runs each, and report what was written under `.quarto`. Say whether an existing way to
      build the record in memory without that serialization exists (the preview, hub and trace observers already
      build one with no file). (relied, 2026-11-08) Earlier measurement: about 0.1 s more for a 2-chunk knitr
      document in a debug build; Julia unresolved at one-second resolution.
- [ ] **Cached knitr chunks (the OPR-7b spike).** Decision D5: a chunk that asks for the cache uses knitr\'s
      finished-markdown fallback and emits **no record**; it degrades quietly. Today `capture_install`
      (`engine/knitr/resources/rmd/capture.R`) turns the cache off for every chunk
      (`knitr$opts_hooks$cache <- function(options) {... options$cache <- FALSE ...}`), so cached chunks are
      recorded. Prototype in scratch: keep the cache **on** for chunks that ask for it, skip recording for those
      chunks, and report: (a) does the first run (miss) and the second run (hit) produce the same document? (b) does a
      mixed document (some structured chunks, some cached text) come out correct, including figures and numbering?
      (c) levels 1, 2 and 3 (`cache=TRUE` is level 3; a level-3 hit returns the cached string before the chunk hook
      runs, `block.R:100-101` of knitr); (d) `dependson` and `autodep`; (e) a document-level `execute: cache: true`
      (every chunk becomes a candidate). Note the knitr epic\'s S4 result (cache off while recording) and its
      pinned test `a_cache_written_without_capture_is_not_replayed_by_a_recorded_run`.
- [ ] **Figure matrix.** Render each case through `q2 render` plain, through the recorded route
      (`execute: {record: true}`), and through Quarto 1 (`quarto render --no-execute` on stored notebooks, or the real
      kernel when available): one labeled figure with `fig-cap`; two figures with `fig-subcap` and `layout-ncol: 2`
      and crossrefs to the figure and each sub-figure; a table with `tbl-cap`. Routes: stored Jupyter notebook,
      knitr, Julia. Record the crossref text each route produces. (ran, 2026-11-08, stored notebook and knitr, one
      case) Quarto 1 gives "Figure 1", "Figure 1 (a)", "Figure 1 (b)"; q2\'s Jupyter-shaped route leaves all three
      refs unresolved (`bd-gbaykhth`); q2\'s knitr route, plain and recorded alike, gives "Figure 3", "Figure 1",
      "Figure 2" (`bd-au5gts6e`). These are out of scope for the epic; the matrix says whether any route changes
      when OPR-1a and OPR-1b land. Do not file new strands for these two cases; they exist.
- [ ] **Marimo route (one paragraph).** Confirm by reading and one run that marimo uses the legacy capture splice
      (`CapturePayload::Legacy`, `engine/capture_splice.rs`) and not the presenter; state in the note that it is a
      documented exception to the epic (decision D12).
- [ ] Write "Effects on the plans": for each result, which plan changes and how (the epic\'s decision-point table
      says what each outcome means). Render-check the note with `target/debug/q2 render --strict`.

## Exit criteria

- Every box above has a recorded result with the exact commands run, marked ran, read or not done.
- "Effects on the plans" names, for each of cost, cache and figures, the plan it affects and whether that plan\'s
  boundary stays as written (OPR-7a, OPR-7b, OPR-7c).
- No file outside `claude-notes/research/` changed.

## Notes from the run

(Fill in at landing.)
