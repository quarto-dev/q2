---
title: 'EXR-6: Jupyter presents from its record (the flip)'
date: 2026-10-28
description: 'Makes the extracted presenter, fed by the execution record, produce the Jupyter engine markdown, deletes the splice-text code in execute_blocks_inner, and rewires stored-ipynb replay onto the same presenter, with the cell wrapper shape preserved byte for byte.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (EXR-6).
**Depends on:** [EXR-5](2026-10-28-exr-05-jupyter-records-shadow.md) (a verified
`present(record) == live` baseline: Phase 3 of EXR-5, so all of EXR-5, not only its Phase 2 gate), [EXR-1](2026-10-28-exr-01-presenter-extraction.md),
[EXR-1b](2026-10-28-exr-01b-replay-engine-move.md) (the replay engine at its final path, since this plan
turns it into a thin adapter), and [EXR-4](2026-10-28-exr-04-record-format.md) (the record types, the builder and the
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
  presenter and EXR-1b the replay engine out of that gate).
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

- [x] Capture golden markdown from the **pre-flip live path** for every EXR-5
  shadow fixture plus the `ipynb_stored_replay.rs` fixtures, committed as files
  under `crates/quarto-core/tests/fixtures/` (one directory, one `.md` per
  fixture, written by a documented regeneration command that is not run in CI).
  This must happen before any deletion: after the flip the live comparator no
  longer exists, and the goldens become the oracle.
  Done 2026-11-01: `tests/fixtures/jupyter-markdown-goldens/` (README has the
  command, `JUPYTER_MARKDOWN_GOLDENS_BLESS=1`). Both families go through the engine
  stage (`engine_stage_driver::run_engine_stage`), so a golden is the capture\'s
  `result.markdown`: the 13 shadow fixtures against the real kernel as `<case>.md`,
  and the six replaying `ipynb_stored_replay.rs` notebooks (copied in as
  `ipynb-<case>.ipynb`) as `ipynb-<case>.md`. Two additions to the plan text: each
  shadow case also stores `<case>.record.json`, the record the same run built, so a
  kernel-free test can present it against the golden (Phase 2 uses it); and the
  pipeline\'s `listing-item: date-modified:` (the copied file\'s mtime, today\'s date) is
  normalized to `<date>` in the markdown before comparing, the only value in a
  golden that is not a function of the fixture.
- [x] Golden test `jupyter_markdown_matches_goldens` (integration), red only by
  deliberate difference; a self-check that every golden file is consumed by at
  least one test.
  `tests/integration/jupyter_markdown_goldens.rs`: the notebook half always runs, the
  shadow half when a Python kernel is on `PATH` (skipped with a message otherwise);
  `every_golden_file_is_consumed` fails on a stray or a missing file.
- [x] Wrapper-shape tests, named: `cell_wrapper_shape_unchanged_for_visible_cell`,
  `hidden_cell_emits_empty_string`, `blank_line_before_cell_div_after_paragraph`,
  `figure_names_keyed_to_executed_cell_position_with_hidden_cells`.
  In the same file. Pre-flip they compose `render_cell` and `splice_mixed` as the
  live loop does; Phase 2 re-points them at `present_document`.
- [x] Trace inventory (before the flip): list every committed trace fixture and test that
  replays a Jupyter `engine_capture` (`replay_engine.rs`, `capture_splice_engines.rs` and
  others; which are Jupyter versus knitr is not verified). Each Jupyter one that predates
  EXR-5 is re-recorded after EXR-5 lands, and the list goes into this plan.
  **Inventory (verified 2026-11-01).** No trace fixture file is committed anywhere in the
  repository (no `.json` outside `target/` and `node_modules/` contains an `engine_capture`
  key), so nothing has to be re-recorded. The Jupyter `engine_capture`s that exist are
  built in memory and none carries a record:
  - `engine/replay.rs` unit tests (`sample_capture`, `engine_name: "jupyter"`,
    `record: None`): they test the generic result replay and are the ones Phase 3b
    revisits (a Jupyter capture without a record must then fail with the re-record error,
    so they move to a non-Jupyter name or gain a record).
  - `stage/trace.rs:782` (`test_json_trace_observer_routes_engine_capture_aux_to_typed_slot`):
    a Jupyter-named capture JSON routed into the trace slot; routing only, never replayed.
  - `capture_splice_engines.rs` records live Jupyter and knitr captures (kernel-gated) and
    feeds `CaptureSpliceStage`, never `ReplayEngine`.
  - knitr only: `pipeline.rs:7290,7313`, `capture_files.rs:401`,
    `quarto-trace/tests/roundtrip.rs` (serialization only).
  - Every `ReplayEngine` integration test uses a synthetic engine name and the markdown
    replay path, which Phase 3b keeps: `replay_engine.rs` (`replay-only-engine-4b`),
    `project_resources.rs` (`replay-real-pipeline-engine`), `render_to_html_captures.rs`
    and `pipeline.rs` (`markerlang`, `replay-only-engine`), `engine_execution.rs`
    (`mock-replay-engine`), `capture_splice_knitr_label.rs` (`someengine`),
    `stage/stages/capture_splice.rs` (`marimo`), and the `r`-named captures in
    `book_download.rs`, `pandoc_request_*.rs`, `typst_render_foundations.rs` and
    `quarto-preview` (`route_tests.rs`, `render_routes_support.rs`, `cache.rs`).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 1: the document-level presenter driver

`present_document` is a new driver, so it is built in small steps, each with its test
first. All tests use hand-built records and no kernel.

- [x] Test first: gap copying and adjacent cells (a document of prose and two cells; the
  output is gaps plus each cell\'s presentation). Then add the signature in the EXR-1
  presenter module,
  `present_document(record, document_text, units, scope, figure_sink)` returning a
  `PresentResult` with `markdown` and `files_written`, with the one pass that copies gaps
  and substitutes each unit\'s presentation by byte range. Units are the stage\'s list
  (design decisions above).
  Done 2026-11-01 in `presenter/document.rs`. The signature as built is
  `present_document(cells: &[RecordCell], document_text, units, scope, target: InlineTarget, fig)
  -> Result<PresentResult, PresentError>`: it takes an engine\'s record cells (the live
  path passes `EngineRecord.cells`, replay the one-engine record\'s `cells`) and the
  target format\'s mime preference for inline values (decision 27: not in the record).
  `PresentResult` also carries `notes` (an inline unit left in place for want of a text
  value), which the engine turns into its existing `Q-18-10` warning; `PresentError`
  is invalid cell options or a front matter value that cannot be written back. Units
  match cells by id, then by content hash, each cell at most once (decision 34). The
  value-side inline helpers (`classify_inline_result`, `InlineOutcome`,
  `escape_yaml_double`, `front_matter_key_at`) moved from `engine::jupyter::inline`
  to `presenter::inline` so the driver is target-neutral; the Jupyter module re-exports
  them.
- [x] Test first: a cell at the document start and one at the end; ceded fences left
  verbatim. Make them pass.
- [x] Test first: the newline rule (a cell after a paragraph ending in a single `\n`
  gets a blank line; `blank_line_before_cell_div_after_paragraph` is the integration twin).
  The driver owns the rule.
- [x] Test first: `begin_cell` is called for every executed unit including hidden ones
  (`figure_names_keyed_to_executed_cell_position_with_hidden_cells`, driver-level).
  Implement it.
- [x] Test first: an inline unit replaced in place (from EXR-3), and an inline unit does
  not consume a cell number. Implement it.
  Plus: a span with no usable text value is left in place and noted; a front matter
  value is escaped and a newline in it is an error; an id mismatch falls back to the
  content hash once per cell; a unit with no id matches by content hash.
- [x] Test first: a unit with no matching record cell is left verbatim. Implement it.
- [x] `PresentResult.files_written` replaces `fig.wrote_any` for supporting-files
  reporting (`text_execute.rs:373-379`); test first that a figure-writing record reports
  files and a text-only one does not.
  The test is in `document.rs` (Phase 1); the replacement landed with the flip (Phase 2).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 2: flip the live path

- [x] `execute_blocks_inner` executes units, enforces the `error:` policy, builds
  the record (EXR-5), and returns `present_document(...)` output as markdown.
  The string-building code (`text_execute.rs:284-371`) is deleted in the same
  commit.
  Done 2026-11-01. Two facts that differed from the plan text: (1) the loop builds
  **two** records: the exported one (EXR-5, over the stage\'s units, only when the stage
  wants it) and a presentation record over the run\'s own located units with ids filled
  from their scanned text, which is what `present_document` consumes; it exists so a
  unit an earlier engine\'s output introduced (no stage index, never in the exported
  record, EXR-5) is still presented, and so a run with no stage in front of it (unit
  tests, the hub provider) presents at all. (2) A span\'s fatal outcomes (raised, no
  value) are still decided at execution time (`span_fatal`); the "no usable text
  value" warning now comes from the driver\'s `PresentNote`, and the front matter
  newline error from `PresentError`, both mapped to the same `Q-18-10` diagnostics as
  before. `span_text` and the `replacements`/`splice_mixed` loop are gone from
  `text_execute.rs`; `router::splice_mixed` itself stays until Phase 4 (the shadow
  harness still uses it).
- [x] `jupyter_markdown_matches_goldens` green; `jupyter_integration.rs`,
  `engine_output_parity.rs`, `capture_splice_engines.rs` and
  `capture_splice_seam.rs` green and unmodified.
  Green and untouched (`git status`), with `jupyter_record_shadow.rs`,
  `jupyter_inline_expressions.rs`, `ipynb_stored_replay.rs`, `engine_record_stage.rs`
  and `engine_error_policy.rs` too. New kernel-free oracle,
  `stored_records_present_to_the_goldens`: a stub `jupyter` engine presents each
  case\'s stored `<case>.record.json` through the real engine stage (the stage\'s units,
  scope and format) and reproduces its golden byte for byte, figure files included. The
  wrapper-shape tests are re-pointed at `present_document` with their expectations
  unchanged.
- [x] `legacy_capture_payload_is_byte_identical_after_flip`: the preview capture
  `result.markdown` for each fixture equals the pre-flip golden.
  The preview capture\'s input differs from the engine stage\'s (`format: q2-preview`,
  a trailing newline), so it has its own goldens, `<case>.preview.md`, blessed from the
  pre-flip tree (commit `ab4fa2ca8` checked out for `text_execute.rs` and `inline.rs`
  during the bless) and compared after the flip. Kernel-gated.
- [x] Real-kernel run of the gated suites on a machine with ipykernel; record the
  command and the counts in this plan.
  2026-11-01, after the flip, with `JUPYTER_PATH=~/.virtualenvs/positron/share/jupyter
  PATH=~/.virtualenvs/positron/bin:$PATH`: `cargo nextest run -p quarto-core
  --no-fail-fast` ran 6964 tests, 6964 passed, 32 skipped (the kernel-gated shadow,
  goldens, inline-expression and capture-splice suites included; live baseline on the
  integration tip before this plan: 6944 passed, 32 skipped; the delta is this plan\'s
  20 new tests). The `#[ignore]`d kernel-lifecycle suite, `cargo nextest run -p
  quarto-core --run-ignored only -E 'test(jupyter_integration)'`: 6 run, 3 passed, 3
  failed, all three failing before the flip too (`test_kernel_execute_matplotlib`
  needs matplotlib, absent from the venv; the two `test_full_pipeline_*` tests panic
  with tokio\'s "Cannot start a runtime from within a runtime", a pre-existing defect
  of those `#[tokio::test]`s calling the blocking engine, outside this plan).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 3: rewire stored replay

- [x] Build a record from the notebook's code cells with EXR-4's ipynb reader
  (cell index from `owning_code_cell_index`, content-hash ids computed from the
  cell source), then present with the same driver. `stored_result_for_cell`
  (`stored.rs:229`) and the second splice loop (`stored.rs:93-202`) are deleted;
  `IpynbReplayEngine` becomes a thin adapter and keeps its name, its
  `intermediate_files`, and its non-registration in `EngineRegistry`.
  Done 2026-11-01 (`engine/ipynb_replay.rs`, the file EXR-1b moved `stored.rs` to).
  The record is built with `RecordBuilder` over the engine\'s located units with ids
  from their scanned text (`AuthorTexts::from_scanned` + `fill_ids`), one cell per
  fence that `owning_code_cell_index` maps into a notebook code cell, carrying the
  cell\'s stored `outputs` and `execution_count` as they are; "EXR-4\'s ipynb reader"
  (`read_record`) is not the tool, since a user notebook has no `quarto_` metadata.
  The legacy-executor cell selection (`runs_in_legacy_executor`) is unchanged. The
  option-YAML check with the located error text stays in the loop so the pinned
  `ERROR:` goldens hold. `legacy_cells`, `code_block_of` and `CodeBlock` in `units.rs`
  lost their last production caller and are now `#[cfg(test)]`.
- [x] `ipynb_stored_replay.rs` and `ipynb_content_processor.rs` green and
  unmodified, including `markdown_cell_literal_fence_stays_inert_while_real_cell_replays`
  and `stored_png_output_written_as_figure_file`.
  Green and untouched, with `presenter_goldens` (the 50-notebook EXR-1 corpus through
  the rewired replay), `wasm_replay`\'s native build and the `ipynb_replay` unit tests.
- [x] Add `ipynb_replay_and_live_share_one_presenter`: a notebook replayed and
  the same cells executed live (stubbed executor) produce identical markdown.
  In `text_execute.rs` `record_tests` (the fake kernel is crate-private): the
  converted notebook is replayed and executed live with the fake kernel answering
  with the stored outputs; the markdown is identical.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 3b: `ReplayEngine` and traces

- [x] Tests first (named):
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
- [x] `ReplayEngine` (or a thin Jupyter replay adapter it delegates to) reads the trace\'s
  `engine_capture.record` and calls `present_document`; entries without a record on a
  non-Jupyter engine keep returning the stored `ExecuteResult`.
  Done 2026-11-01 (`ReplayEngine::present_record`). The units are the stage\'s
  (`units_for_engine`), or a fresh scan with ids filled when the engine is driven
  directly; the scope and format come from the `ExecutionContext` the stage already
  passes (the "replay adapter inputs" risk below is settled: `ReplayEngine::execute`
  now reads its context, nothing is added to the trace). The stored result\'s side
  data is kept, its markdown and supporting files are recomputed, and the record
  travels on `ExecuteResult.record` byte for byte. The seven named tests are in
  `engine/replay.rs` (hand-built records through `RecordBuilder` and `coalesce`) except
  `replay_jupyter_trace_presents_same_markdown_as_live` and the real-run form of
  `replay_engine_returns_byte_identical_record`, which use the fake kernel in
  `text_execute.rs` `record_tests`. The pre-existing `replay.rs` unit tests now use a
  knitr capture (a Jupyter capture without a record is the re-record error). The
  browser preview does not use `ReplayEngine` (it splices captures through
  `CaptureSpliceStage`), so hub captures without a record are unaffected.
- Gate (after the tests and the adapter): `cargo clippy -p quarto-core --all-targets -- -D warnings`
  plus `cargo nextest run -p quarto-core`.
- [x] Update `claude-notes/instructions/replay-engine.md`: the trace carries the record,
  replay re-presents it, replay output follows the current presenter and target format,
  and Jupyter traces recorded before EXR-5 (no `engine_capture.record`) must be
  re-recorded.

### Phase 4: cleanup and verification

- [x] `format_outputs`, `render_cell` and `FigureWriter` are referenced only from
  the presenter module; `rg` check recorded in this plan. Remove the temporary
  re-exports used during EXR-1.
  Checked 2026-11-01 with `grep -rn` over `crates/`: `format_outputs` has no caller
  outside `presenter/`; `render_cell` is called outside it only by tests
  (`jupyter/presenter_goldens.rs`, `text_execute.rs` `record_tests`, the goldens
  wrapper-shape tests); the figure sinks (`FigureSink`, `FsFigureSink`,
  `RuntimeFigureSink`; there is no `FigureWriter` since EXR-1) are constructed by the
  three `present_document` callers (`text_execute.rs`, `ipynb_replay.rs`, `replay.rs`)
  and tests. The EXR-1 re-export `pub use crate::presenter::output::{...}` in
  `jupyter/execute.rs` is removed; `engine::jupyter::{CellResult, ...}` stays as the
  public path of the session API, now re-exported from the presenter directly. The
  shadow harness (`jupyter_record_shadow.rs`) drives `present_document` instead of its
  test-only driver, so `router::splice_mixed` has no caller outside `router.rs` and its
  tests; it is left for EXR-17 with the rest of the legacy splice.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`;
  `cargo nextest run -p quarto-core`; report the pass and skip counts against the
  live baseline taken at the start of Phase 2, and account for the delta (the
  expected delta is the removed duplicate replay tests plus the new golden and
  driver tests).
  2026-11-01, kernel on `PATH`: clippy clean; nextest 6973 passed, 32 skipped, against a
  live baseline on the integration tip (`f45c108ba`) of 6944 passed, 32 skipped. The
  delta is +29, all new tests, none removed (no duplicate replay test existed to
  remove): 18 in Phases 0 and 1 (the goldens file, the `present_document` tests, and
  the inline-helper tests that moved with their code, net of nothing), 2 in Phase 2
  (`stored_records_present_to_the_goldens`, the legacy payload test), 9 in Phases 3 and
  3b (`ipynb_replay_and_live_share_one_presenter`, the fake-kernel replay round trip,
  seven in `replay.rs`). Phase 4\'s edits only moved imports and the shadow harness.
- [x] Recommend, without running unprompted, the nextest run on direct dependents
  of `quarto-core`, and a manual `q2 preview` check that a Jupyter document still
  splices (the legacy path).
  Recommended in the handoff, not run: `cargo nextest run --no-fail-fast -p quarto
  -p quarto-preview -p quarto-hub-provider -p quarto-lsp-core -p quarto-publish
  -p quarto-test -p perf-harness`, and a manual `q2 preview` of a Jupyter document.
  `cargo check --all-targets` on `quarto`, `quarto-preview` and `quarto-hub-provider`
  is run before the merge.
- [x] Document the one command that regenerates the goldens (a note in
  `claude-notes/instructions/`, linked from the golden fixtures directory), and say in
  it that the goldens are otherwise never regenerated (epic decision 28).
  `claude-notes/instructions/jupyter-markdown-goldens.md`, linked from the fixtures README.
- [x] Render this plan with `q2 render --strict` before any push.
  Rendered with the two instruction files: only the 36 unrelated `Q-12-13` listing errors, none naming these files.

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
  Rehearsed 2026-11-01: `git revert f835ea24d` (the flip commit) on a detached head,
  then `jupyter_markdown_goldens` and `jupyter_record_shadow` with the kernel: 21 passed.
  The rehearsal ran at the flip commit itself. Reverting it on the final branch touches
  `text_execute.rs`, which Phase 3b later extended with a test, so expect a small
  conflict there; Phases 3 and 3b build on the driver, not on the flip.

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
