---
title: 'EXR-6: Jupyter presents from its record (the flip)'
date: 2026-10-28
description: 'Makes the extracted presenter, fed by the execution record, produce the Jupyter engine markdown, deletes the splice-text code in execute_blocks_inner, and rewires stored-ipynb replay onto the same presenter, with the cell wrapper shape preserved byte for byte.'
status: draft
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (EXR-6).
**Depends on:** [EXR-5](2026-10-28-exr-05-jupyter-records-shadow.md) (a verified
`present(record) == live` baseline), [EXR-1](2026-10-28-exr-01-presenter-extraction.md),
and [EXR-4](2026-10-28-exr-04-record-format.md) (the record types, the builder and the
reader). Both EXR-4 and EXR-5 are hard dependencies: the flip needs EXR-5\'s `record`
field and coalescing and EXR-4\'s `RecordBuilder` and ids.
**Blocks:** [EXR-17](2026-10-28-exr-17-legacy-removal.md). Soft prerequisite of
[EXR-14](2026-10-28-exr-14-preview-splice.md)\'s end-to-end checks: preview parity rests on
EXR-5's shadow `present(record) == live` equivalence, so the id-based splice does not
wait for the flip. Per the dependency table in the epic, it also blocks EXR-18.
**Lane:** A (`workspace-4`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3.

## Goal

Additive step 4: the deliberate flip. After this plan, the Jupyter engine
executes units, builds a record, and returns `present(record, document)` as its
markdown. The record is the interface between execution and presentation for
Jupyter, live and on replay, and the hand-written text splicing in
`execute_blocks_inner` and in the stored-ipynb replay engine is gone.

## Background

Verified 2026-10-28 on `feature/execution-records-epic` (main `2ee0bd966`).

- **Two copies of the splice loop.** The live loop builds the output string from
  `input[last_end..block.start]` gaps plus `render_cell` output
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs:284-371`). The stored
  replay engine has its own copy of the same loop
  (`jupyter/stored.rs:93-202`), calling the same `render_cell`, `FigureWriter`,
  `CellVisibility` and `partition_cell_options` imported from `text_execute.rs`
  (`stored.rs:33-37`).
- **Blank-line discipline lives in the splice.** A fenced div cannot interrupt a
  paragraph, so both loops add a newline when the text before a cell ends in a
  single `\n` (`text_execute.rs:295-300`, `stored.rs:146-149`). The presenter
  must own this rule after the flip.
- **The `.cell` wrapper is a cross-engine contract.** `render_cell`
  (`text_execute.rs:562-583`) emits one `Div.cell` per executed cell, or the empty
  string when echo and every output are suppressed (579-581). The preview capture
  splice replaces a live engine cell with exactly one wrapper, and the CSS targets
  `.cell .cell-output-* pre code` (doc comment 548-551). The legacy splice
  (`capture_splice`) stays until EXR-17, so this shape is frozen
  (`2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`).
- **Error policy is execution semantics and stays in the engine.**
  `first_cell_error` and the `error:` abort run at execution time
  (`text_execute.rs:336-350`); stored replay deliberately skips them
  (`stored.rs:175-178`). After the flip the engine still aborts before
  presenting.
- **`.ipynb` alignment goes through the source map, not order.**
  `owning_code_cell_index` maps each fence's `code_start` into the converter's
  per-cell virtual file (`stored.rs:209-220`), so a literal fence in a markdown
  cell stays inert. The replay side must keep this alignment when it builds a
  record from notebook cells.
- **Stage routing.** `.ipynb` input without `execute.enabled: true` constructs
  `IpynbReplayEngine` directly in `EngineExecutionStage`
  (`stage/stages/engine_execution.rs:349-440`), and the push compiles out on
  WASM because `engine::jupyter` is native-only (bd-9zmpzw3w; EXR-1 moves the
  presenter and the replay engine out of that gate).
- **`ReplayEngine` replays an `ExecuteResult`.** `crates/quarto-core/src/engine/replay.rs`
  compares the QMD handed to `execute()` byte for byte with `engine_capture.input_qmd`,
  hard-fails on any difference, and otherwise returns the recorded `ExecuteResult`
  verbatim (`claude-notes/instructions/replay-engine.md`). Today that result contains the
  finished markdown. Because this plan keeps presentation inside the engine
  (`present_document` runs in `execute_blocks_inner`), a replayed Jupyter result is only
  reproducible after the flip if `ReplayEngine` itself runs the presenter on the stored
  record; EXR-5 stores that record on the trace in the trace-only field
  `engine_capture.record` (verified 2026-10-29 that `EngineCapture.result` is opaque
  `ExecuteResult` JSON and `ExecuteResult.record` is `serde(skip)`).
- **Existing coverage to keep green:** `jupyter_integration.rs`,
  `ipynb_stored_replay.rs` (stdout, HTML, error, literal fence, PNG, execute
  demand), `engine_output_parity.rs`, `capture_splice_engines.rs`,
  `capture_splice_seam.rs`, `ipynb_content_processor.rs`, `replay_engine.rs`.
- Not verified: whether any caller outside these files depends on the string
  produced by the splice for anything other than the markdown (the figure
  directory is reported separately through `fig.wrote_any`, `text_execute.rs:376-379`).

## Design decisions (from the epic; do not re-litigate)

- The flip is a deliberate, single-purpose change with one review. Rollback is a
  revert of the flip commit; there is no runtime switch, because a switch is two
  mechanisms alive at once (the bd-mu9i0bct class of bug).
- Presentation reads options from the document at present time (`echo`, `output`,
  `include`, `warning` over `execute_scope`), never from the record.
- The legacy `EngineCapture` payload for preview is unchanged by this plan; the
  `.cell` wrapper shape is preserved exactly until EXR-17.
- The Rust presenter is the long-term single presenter (epic decision 5).
- **The driver consumes the stage\'s unit list** (epic decision 38): `present_document`
  takes the units from `ExecutionContext.units`, with ids already filled, each unit\'s
  `start`, `end` and `code_start` byte offsets and its `line` re-located into the text this
  engine received by EXR-2\'s `locate_units` (this plan does not describe that step again);
  there is no scan in the presenter.
- **`ReplayEngine` and traces (2026-10-29 review, D2).** `ReplayEngine`, `--replay` and
  the trace's `engine_capture` block stay. After the flip a Jupyter replay returns the
  stored record (EXR-5 puts it in `engine_capture.record` on the trace) and runs the
  presenter again on it: the replay adapter calls `present_document` with the trace\'s
  `input_qmd` as the document text, the units from running the stage\'s scan-and-fill
  function (EXR-2 `scan_units` plus EXR-4\'s id function) on it, and the current
  `execute_scope` and target format. The record `ReplayEngine` returns is byte-identical to what was recorded, but
  the markdown is recomputed, so replay output depends on the current presenter and on
  the target format; a trace is a snapshot of what the engine produced, not of what the
  user saw. No presented-markdown snapshot and no presenter-version field are stored
  (Gordon: no stronger guarantee needed). A Jupyter trace recorded before EXR-5 landed
  has no `engine_capture.record` and is unsupported: replay fails with a clear "re-record
  this trace" error. (Traces recorded after EXR-5 and before the flip do carry a record,
  because EXR-5 writes it in shadow mode; they replay through the presenter.)
  The hard-fail miss policy is unchanged. Engines that still return markdown (knitr and
  marimo) keep the markdown replay path. knitr is not flipped in this
  epic at all (epic decision 6).

## Out of scope

- Preview and hub consumers of the record (EXR-13, EXR-14, EXR-15).
- Removing the legacy structural splice (`capture_splice.rs`; EXR-17).
- Julia (its flip is in EXR-8). marimo has no flip in this epic: EXR-9 is additive and
  shadow-tested, and marimo leaves the legacy path at EXR-17. knitr has no flip in this epic:
  EXR-11 and EXR-12 only record, and its records feed the consumers (the follow-on knitr
  epic changes how knitr is presented; epic decision 6).
- Any change to cell output markup. Output bytes are frozen by the goldens below.

## Phased checklist (tests first)

### Phase 0: freeze the baseline before touching it

- [ ] Capture golden markdown from the **pre-flip live path** for every EXR-5
  shadow fixture plus the `ipynb_stored_replay.rs` fixtures, committed as files
  under `crates/quarto-core/tests/fixtures/` (one directory, one `.md` per
  fixture, written by a documented regeneration command that is not run in CI).
  This must happen before any deletion: after the flip the live comparator no
  longer exists, and the goldens become the oracle.
- [ ] Golden test `jupyter_markdown_matches_goldens` (integration), red only by
  deliberate difference; a self-check that every golden file is consumed by at
  least one test.
- [ ] Wrapper-shape tests, named: `cell_wrapper_shape_unchanged_for_visible_cell`,
  `hidden_cell_emits_empty_string`, `blank_line_before_cell_div_after_paragraph`,
  `figure_names_keyed_to_executed_cell_position_with_hidden_cells`.
- [ ] Trace inventory (before the flip): list every committed trace fixture and test that
  replays a Jupyter `engine_capture` (`replay_engine.rs`, `capture_splice_engines.rs` and
  others; which are Jupyter versus knitr is not verified). Each Jupyter one that predates
  EXR-5 is re-recorded after EXR-5 lands, and the list goes into this plan.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 1: the document-level presenter driver

`present_document` is a new driver, so it is built in small steps, each with its test
first. All tests use hand-built records and no kernel.

- [ ] Test first: gap copying and adjacent cells (a document of prose and two cells; the
  output is gaps plus each cell\'s presentation). Then add the signature in the EXR-1
  presenter module,
  `present_document(record, document_text, units, scope, figure_sink)` returning a
  `PresentResult` with `markdown` and `files_written`, with the one pass that copies gaps
  and substitutes each unit\'s presentation by byte range. Units are the stage\'s list
  (design decisions above).
- [ ] Test first: a cell at the document start and one at the end; ceded fences left
  verbatim. Make them pass.
- [ ] Test first: the newline rule (a cell after a paragraph ending in a single `\n`
  gets a blank line; `blank_line_before_cell_div_after_paragraph` is the integration twin).
  The driver owns the rule.
- [ ] Test first: `begin_cell` is called for every executed unit including hidden ones
  (`figure_names_keyed_to_executed_cell_position_with_hidden_cells`, driver-level).
  Implement it.
- [ ] Test first: an inline unit replaced in place (from EXR-3), and an inline unit does
  not consume a cell number. Implement it.
- [ ] Test first: a unit with no matching record cell is left verbatim. Implement it.
- [ ] `PresentResult.files_written` replaces `fig.wrote_any` for supporting-files
  reporting (`text_execute.rs:373-379`); test first that a figure-writing record reports
  files and a text-only one does not.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 2: flip the live path

- [ ] `execute_blocks_inner` executes units, enforces the `error:` policy, builds
  the record (EXR-5), and returns `present_document(...)` output as markdown.
  The string-building code (`text_execute.rs:284-371`) is deleted in the same
  commit.
- [ ] `jupyter_markdown_matches_goldens` green; `jupyter_integration.rs`,
  `engine_output_parity.rs`, `capture_splice_engines.rs` and
  `capture_splice_seam.rs` green and unmodified.
- [ ] `legacy_capture_payload_is_byte_identical_after_flip`: the preview capture
  `result.markdown` for each fixture equals the pre-flip golden.
- [ ] Real-kernel run of the gated suites on a machine with ipykernel; record the
  command and the counts in this plan.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 3: rewire stored replay

- [ ] Build a record from the notebook's code cells with EXR-4's ipynb reader
  (cell index from `owning_code_cell_index`, content-hash ids computed from the
  cell source), then present with the same driver. `stored_result_for_cell`
  (`stored.rs:229`) and the second splice loop (`stored.rs:93-202`) are deleted;
  `IpynbReplayEngine` becomes a thin adapter and keeps its name, its
  `intermediate_files`, and its non-registration in `EngineRegistry`.
- [ ] `ipynb_stored_replay.rs` and `ipynb_content_processor.rs` green and
  unmodified, including `markdown_cell_literal_fence_stays_inert_while_real_cell_replays`
  and `stored_png_output_written_as_figure_file`.
- [ ] Add `ipynb_replay_and_live_share_one_presenter`: a notebook replayed and
  the same cells executed live (stubbed executor) produce identical markdown.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 3b: `ReplayEngine` and traces

- [ ] Tests first (named):
  - `replay_jupyter_trace_presents_same_markdown_as_live` (record a trace from a stubbed
    executor, replay it, markdown equal to the live run for the same target format);
  - `replay_engine_returns_byte_identical_record`;
  - `replay_trace_recorded_before_exr5_fails_with_rerecord_error` (a trace with a Jupyter
    `engine_capture` and no `engine_capture.record` fails with the "re-record" message,
    never a silent fallback), and `replay_trace_recorded_between_exr5_and_flip_replays`
    (a trace that carries a record replays through the presenter);
  - `replay_id_mismatch_falls_back_to_content_hash` (a sugared cell whose replay id differs
    from the recorded id still matches by content hash);
  - `replay_knitr_trace_unchanged` (markdown replay path still byte-identical);
  - `replay_miss_policy_still_hard_fails` (one changed byte in the input QMD);
  - `replay_output_follows_target_format` (one trace, two target formats, the markdown
    differs where the presenter's mime choice differs; documents the consequence above).
- [ ] `ReplayEngine` (or a thin Jupyter replay adapter it delegates to) reads the trace\'s
  `engine_capture.record` and calls `present_document`; entries without a record on a
  non-Jupyter engine keep returning the stored `ExecuteResult`.
- Gate (after the tests and the adapter): `cargo clippy -p quarto-core --all-targets -- -D warnings`
  plus `cargo nextest run -p quarto-core`.
- [ ] Update `claude-notes/instructions/replay-engine.md`: the trace carries the record,
  replay re-presents it, replay output follows the current presenter and target format,
  and Jupyter traces recorded before EXR-5 (no `engine_capture.record`) must be
  re-recorded.

### Phase 4: cleanup and verification

- [ ] `format_outputs`, `render_cell` and `FigureWriter` are referenced only from
  the presenter module; `rg` check recorded in this plan. Remove the temporary
  re-exports used during EXR-1.
- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings`;
  `cargo nextest run -p quarto-core`; report the pass and skip counts against the
  live baseline taken at the start of Phase 2, and account for the delta (the
  expected delta is the removed duplicate replay tests plus the new golden and
  driver tests).
- [ ] Recommend, without running unprompted, the nextest run on direct dependents
  of `quarto-core`, and a manual `q2 preview` check that a Jupyter document still
  splices (the legacy path).
- [ ] Document the one command that regenerates the goldens (a note in
  `claude-notes/instructions/`, linked from the golden fixtures directory), and say in
  it that the goldens are otherwise never regenerated (epic decision 28).
- [ ] Render this plan with `q2 render --strict` before any push.

## Exit criteria

- Jupyter markdown, live and replayed, comes from `present_document` and matches
  the pre-flip goldens byte for byte.
- No splice-text code remains in `text_execute.rs` or `stored.rs`.
- Preview capture output is unchanged; the legacy splice tests pass unmodified.
- A recorded Jupyter trace replays through the presenter and matches the live markdown
  for the same target format; a Jupyter trace recorded before EXR-5 fails with the
  re-record error.
- Rollback is documented as a single revert, and has been rehearsed once locally
  (revert, run the golden suite, reapply).

## Risks and open questions

- **Byte drift at gap boundaries.** The newline rule and the empty-wrapper rule
  are the likely places to drift; the named wrapper tests exist for them.
- **Timing.** Markdown is now produced after all units ran, not progressively.
  The engine returned a whole string before as well, so no caller sees the
  difference; confirm no progress callback reads partial text.
- **Mid-document failure.** On an `error: false` abort the engine returns an error
  before presenting, exactly as today. A test pins that no partial record is
  emitted to the observer on failure.
- **Replay units in a multi-engine document.** The trace\'s `input_qmd` is the text one
  engine received (after earlier engines\' reconciliation), so a fresh scan of it gives cell
  indices that differ from the ids in the record (epic decision 38). Settled (settled arbitrarily, Gordon 2026-10-31: revisit if wrong): record-based
  replay uses the ids stored in the record cells. A fresh scan of a legacy `input_qmd` fills ids
  best-effort without author texts, so a sugared cell\'s replay id can differ from its
  recorded id; that cell fails the id match and falls back to the content hash (epic decision
  34), so it still matches. A single-engine trace is unaffected.
- **Hidden figure numbering under `include: false`.** Covered by the Phase 0
  fixture; if EXR-3 inline units interleave, their `begin_cell` handling must not
  consume cell numbers.
- **Traces stop being frozen output.** Replay re-presents, so a presenter change alters
  what an old trace replays to, and a bug that lived in the old presenter no longer
  reproduces. Accepted (D2); the engine-produced record is what stays fixed.
- **Replay adapter inputs.** `present_document` needs the scope and target format at
  replay time (epic decision 27: the record does not carry them). `ExecutionContext`
  has `format` (`engine/context.rs:51`) and `execute_scope` (`:76`, doc `:63-75`) and the source
  arrives as the `input` argument, but `ReplayEngine::execute` ignores its context
  today (`replay.rs:125-130`), so the first task here is to confirm the replay path
  passes them through. If it does not, the missing inputs go in the trace, never the
  record.
- **Goldens stay (epic decision 28).** The Phase 0 goldens are kept after EXR-17.
  When it lands they move under the presenter\'s tests and are regenerated only by
  one documented command (the Phase 4 task below writes it).

## References

- `crates/quarto-core/src/engine/jupyter/text_execute.rs`, `stored.rs`
- `crates/quarto-core/src/engine/replay.rs`, `claude-notes/instructions/replay-engine.md`
- `crates/quarto-core/src/stage/stages/engine_execution.rs`
- `crates/quarto-core/tests/integration/jupyter_integration.rs`,
  `ipynb_stored_replay.rs`, `engine_output_parity.rs`, `capture_splice_engines.rs`
- [The epic](2026-10-28-execution-records-epic.md);
  [Plan 7c](2026-07-08-plan7c-ipynb-content-processor.md)
