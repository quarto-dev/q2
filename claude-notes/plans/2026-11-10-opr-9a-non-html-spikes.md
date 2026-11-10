---
title: 'OPR-9a: spikes for the plain-render switch (a note)'
date: 2026-11-10
description: 'Six short measurements that settle the open decisions of the old OPR-7d before the other pieces are built: the format matrix for Julia on every target, a prototype of the presenter fix, a prototype of the interleaving normalizer and its pin, zip extraction, reserved option keys, and the browser replay path. Produces one research note; commits no prototype code.'
status: ready
depends-on: []
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9a-non-html-spikes` · **Lane B** (`workspace-4`) · **Blocks:** 9b, 9c, 9d, 9e.
**Needs:** Julia 1.12 with Plots, R 4.3 with knitr, `pandoc`, `typst`; a `cargo build --bin q2`; for S6 possibly `hub-client` tooling.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Why this plan exists

The old 7d plan held a normalizer, a pinned entry, zip comparison, five new Julia target classes, a presenter fix, a
flip and a preview chain. The review that split it found five things nobody had measured. This plan measures them
first, so the later pieces (9b to 9g) are written from facts and the decisions that need Gordon are put to him once,
with a recommendation each. It writes one research note and no production code.

## Facts already known (do not redo; marks are from the review)

- Julia gate on the integration tip (ran): 11 identical, 1 allowlisted, 4 blockers (`julia/capture-facts`,
  `julia/fig-subcap`, `julia/presentation-attrs`, `julia/records-plot`), 1 both-failed (expected). `records-plot` and
  `presentation-attrs` differ by interleaving only; `fig-subcap` also differs by 7j (not landed); `capture-facts` has four
  kinds of difference (the `data-prompt="true"` attribute, the `&gt; ` prefix, numeric `echo`, stderr blocks), 51 diff lines.
- The recorded Plots output on gfm, epub, pptx, html and revealjs carries `application/pdf`, `image/png`, `image/svg+xml`
  and `text/html`; on typst and docx only the two images (ran, `.quarto/execution-records/doc.ipynb`).
- The presenter picks `text/html` first on every target (`format_outputs_with`, `presenter/cell.rs`); Quarto 1 picks by
  target (`display-data.ts:45`). gfm: plain writes `figure-gfm/cell-4-output-1.svg`, recorded inlines a base64 PNG. pptx: plain
  has `ppt/media/image1.svg`, recorded has no media (ran). docx and typst match apart from the typst directory name.
- The harness reads every output with `read_to_string`, so docx, pptx and epub fail on both sides and are reported as
  "both failed"; `COMPARABLE` gates only the knitr loop (ran).
- Flipping `EngineExecutionStage::new()` fails three tests in `quarto-core` and `quarto`: two `engine_record_stage` tests and
  `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` (a real regression, see 9c) (ran).
- A nextest run rebuilds `target/debug/q2` with whatever the tree held. After any experiment run `cargo build --bin q2` on
  a clean tree before you measure the plain route (the review lost an hour to a flipped binary that made both routes equal).

## Rules for a spike

- No prototype code is committed. Make a prototype as a patch in your session scratchpad, measure, then `git checkout -- .`
  and confirm `git status` is clean. Say which binary each measurement used and that it was built from which tree state.
- Scratch dirs live in the session scratchpad, not `/tmp`. No pushes. Use `cargo nextest run`, never `cargo test`, never
  pipe it through `tail`.
- Every claim in the note carries a mark. A spike that cannot answer its question says so; it does not guess.

## The spikes

**S1. Format matrix (cheap).** *Question:* for each target, what does the Julia runner emit, and what does each route write?
*Method:* copy `crates/quarto-core/tests/fixtures/opr-parity/julia/records-plot/doc.qmd` and the default Julia environment\'s
`Project.toml` and `Manifest.toml` (the one with Plots; see `plots_environment` in `opr_parity_runner.rs`) into two scratch
dirs, plain (`project: type: default`) and recorded (`execute: record: true`). Render `--to` html, revealjs, dashboard, gfm,
commonmark, markdown, docx, pptx, odt, epub, typst (with `keep-typ: true`), pdf, and every other identifier `format.rs`
lists as renderable that your tools allow; say which you skipped and why. Per format record: the mime keys of the Plots
output (jq over the record), the files each route writes under `doc_files`, the media inside zip outputs, and a one-line
verdict (match, or differs and why). Find where the Julia runner or the engine chooses a bundle per target (search the Julia
engine subtree and `ts_record.rs`) and say whether that is a runner or a core decision. *Decides:* the target classes 9b must
handle, the starting contents of the Julia target allowlist in 9e, and whether 9b needs a runner change.

**S2. Prototype the presenter fix (moderate).** *Question:* after the fix, do plain and recorded match beyond interleaving on
every format, and what else changes? *Method:* in a scratch state apply OPR-9b\'s change (a target field on `CellOptions`,
the `text/html` branch gated by `display_data_mime_type`, the figure directory named from the format) and the two-edit
OPR-7j prototype (its diff is in the 7j plan) if 7j has not landed. Rebuild, repeat the S1 matrix, and run the Julia part of
the harness with gfm, typst, revealjs, docx, pptx added temporarily to the `julia/fig-subcap` and `julia/records-plot`
manifest entries (revert). Run `cargo nextest run -p quarto-core` and list every test that fails. *Decides:* 9b\'s test list
and scope; whether gfm and pptx can graduate in 9e; the number and names of Jupyter results that change on non-HTML targets
(for the landing report and OPR-8\'s notes).

**S3. Normalizer prototype (moderate).** *Question:* does the design in 9d work on real pages, and which parser? *Method:* prototype
`julia-interleaved-cell` against the four real pages (with the S2 state for `fig-subcap`) and the seven negative controls in
9d, once with `scraper` (parse and round-trip both sides first; `scraper` keeps no source spans, so splicing back is not
possible) and, if that fails trap 1, once with `lol_html` (a normal dependency that preserves untouched bytes). Produce the
post-normalizer `capture-facts` diff and say how many lines it has and how they group by kind. *Decides:* the parser; where the
pin lives (a TOML literal string, or a file beside the allowlist); whether 51 lines is signable or the entry should show a
count per kind with the full diff attached to the report.

**S4. Zip extraction (cheap).** *Question:* what must be extracted and stripped to compare docx, pptx and epub? *Method:* write
the draft hand-run document (a `println`, a labeled figure from a hardcoded PNG, a labeled table, a bare expression, and a
`fig-subcap` group cell), render it to docx, pptx and epub on both routes, extract `word/document.xml`, the slide XML and the epub
xhtml, and list the noise (relationship ids, timestamps, `docProps`, media names, slide order). Include the draft document in
the note so 9e can commit it. *Decides:* 9d\'s extractor and its normalization list.

**S5. Reserved option keys (cheap).** *Question:* is `quarto-unit-id` the only author-reachable key the epics made reserved?
*Method:* read how a `#|` option on a Julia or Jupyter cell reaches `metadata.quarto` (`ts_record.rs`, the Julia runner, the
cell-option partition); try `#| quarto_attrs:`, `quarto_expansion`, `quarto_file_path`, `quarto_output_kinds` and the other
`quarto_*` keys of `presenter/knitr_inputs.rs`, `engine_attrs.rs` and `expansion.rs` on a Julia and a Jupyter cell; read the inline
span id path (`span_records.rs`, `quarto_record_span`) for a collision an author could cause. *Decides:* the scope of 9c (one
key or a list).

**S6. Browser replay (static first).** *Question:* does the flip add a runtime path to the WASM client? *Method:* find who builds
the pipeline in `wasm-quarto-hub-client`, whether `want_record` is true there, and whether `coalesce` already ran on that path
before the flip (`record/coalesce.rs:55-66` can fail with `UnknownCell` or `DuplicateCell`). Only if that is unclear, build with
`cd hub-client && npm run build:wasm` and run the two vitest files (`recordPayload.wasm.test.ts`,
`captureSpliceHtml.wasm.test.ts`) against a flipped build. *Decides:* whether 9g\'s WASM box is "compile and smoke" or needs a test.

## Deliverable

One note, `claude-notes/research/2026-11-NN-opr-9a-spike-results.md` (render it with `q2 render --strict`), with a section per
spike: question, method, what you ran, result, what it decides. End with **Decisions for Gordon**, each with your recommendation:

1. gfm: graduate (if S2 shows parity) or gate for Julia. 2. Does the Julia target gate also apply when `want_record` is true
(`--trace`, `record: true`, preview, hub)? 3. The post-flip rule for the four gate-time entries (recommended in 9g: remove them at the
flip, keep the normalizer code and tests, assert identity with an empty allowlist). 4. The pin\'s form (S3). 5. Any Jupyter result S2
shows changing that Gordon did not expect.

## Checklist (every box is `[unattended]`; the decisions in the note are Gordon\'s)

- [ ] S1 run; matrix table in the note.
- [ ] S5 run; reserved-key list in the note (done when 9c\'s scope is one sentence).
- [ ] S4 run; extractor spec and the draft fixture in the note.
- [ ] S2 run; patch kept in the scratchpad, tree clean afterwards, the S1 matrix re-run, failing tests listed.
- [ ] S3 run; parser verdict and the post-normalizer `capture-facts` diff in the note.
- [ ] S6 answered (static, or the WASM run).
- [ ] The note\'s "Decisions for Gordon" is complete; render it; commit; land with the protocol\'s rebase and fast-forward.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- a spike cannot be run because a tool is missing (R, Julia, Plots, pandoc, typst): report which, do not skip it silently;
- the working tree is not clean after you revert a prototype, or a build leaves `target/debug/q2` in an unknown state you cannot
  rebuild;
- a measurement contradicts a "fact already known" above: say so first, because other pieces rely on it;
- you would need to change production code to answer a question: stop and ask.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9b, 9c and 9d as startable when 9a is `complete`, and say which decisions in the note Gordon must answer first.

## Exit criteria

The note is on the integration branch, every spike has an answer or an honest "could not", the decisions for Gordon are
listed with recommendations, and the working tree was clean after every prototype.

## Notes from the run

(Fill in at landing.)
