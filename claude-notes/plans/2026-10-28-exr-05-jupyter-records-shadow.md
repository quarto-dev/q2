---
title: 'EXR-5: Jupyter emits execution records (shadow mode)'
date: 2026-10-28
description: 'Makes the Jupyter engine build an in-memory engine execution record next to its unchanged live markdown path, coalesces records at the end of the engine stage, and proves present(record) equals the live markdown on fixtures before anything depends on the record.'
status: draft
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (EXR-5).
**Depends on:** [EXR-1](2026-10-28-exr-01-presenter-extraction.md) (the extracted
presenter), [EXR-3](2026-10-28-exr-03-jupyter-iee.md) (spans in the unit loop, and the
fake kernel these tests reuse; the `Unit` loop and the `kernel_for(unit)` seam are
[EXR-2](2026-10-28-exr-02-unit-scanner.md)\'s), [EXR-4](2026-10-28-exr-04-record-format.md)
(the record type and builder). Soft: [JOL-2b](2026-10-28-jol-2b-ownership-gate.md)
(per-cell kernel metadata is recorded whatever kernel is chosen).
**Blocks:** [EXR-7](2026-10-28-exr-07-engine-wire.md) (it introduces
`ExecuteResult.record`, `want_record` and the coalescing hook that EXR-7 sets and
calls), [EXR-6](2026-10-28-exr-06-jupyter-flip.md) (the flip),
[EXR-13](2026-10-28-exr-13-hub-wire.md) (the hub needs something to carry). Per the dependency table in the epic, it also blocks EXR-15.
**Lane:** A (`workspace-4`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3-4.

## Goal

Additive step 3 of the migration (epic decision 2): the Jupyter engine returns an
engine execution record as a side product of every run, the engine stage
coalesces the records of all engines into one document-level record, and a
shadow test proves that presenting that record reproduces, byte for byte, the
markdown the unchanged live path produced. Nothing in production reads the
record yet, and the live path does not change.

## Background

Verified 2026-10-28 on `feature/execution-records-epic` (main `2ee0bd966`).

- **The live loop throws the execution result away.** `execute_blocks_inner`
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs:262-381`) starts one
  session (271-273), then per block partitions `#|` options and code (311),
  executes (331-334), applies the `error:` abort policy (339-350), and splices
  `render_cell(...)` output into a `String` (359-365). The
  `KernelExecuteResult` (`status`, `outputs`, `execution_count`;
  `jupyter/execute.rs:26-33`) is dropped after `render_cell`. Raw outputs are
  already in the shape nbformat uses: the `CellOutput` enum (stream,
  display data, execute result, error) with a `MimeBundle` of JSON values
  (`execute.rs:52-75`).
- **Presentation inputs that are not outputs.** Echoed source is the
  option-stripped `cell.code` (`text_execute.rs:359-365`), and visibility comes
  from `CellVisibility::resolve` over the cell's `#|` map merged over
  `ctx.execute_scope` (323-327, 425-468). Those are resolved at present time
  from the document, so `echo` toggling needs no re-run (a Jupyter-only
  property, epic decision 6).
- **Figure numbering is keyed to executed-cell position.** `fig.begin_cell()`
  runs for every executed cell, visible or not (354-358), and
  `FigureWriter::write_image` names files `cell-N-output-M.ext` under
  `<stem>_files/figure-html/` (623-711). A record must keep hidden executed
  cells, in order, or the presenter cannot reproduce the names.
- **`ExecuteResult` is serialized whole into the legacy capture.**
  `serde_json::to_value(&result)` at
  `crates/quarto-core/src/stage/stages/engine_execution.rs:638`. New fields that
  must not enlarge legacy captures need `#[serde(skip)]` or an explicit take.
  New optional fields elsewhere use `#[serde(default)]`
  (`engine/context.rs:338-381`).
- **Observer-gated collection has a precedent.**
  `PipelineObserver::wants_engine_capture_files` defaults to `false`
  (`stage/observer.rs:230`), is `true` only for the preview recorder
  (`engine/preview_record.rs:103`), and gates the file I/O at
  `engine_execution.rs:646`. Plain `q2 render` must keep paying nothing.
- **The stage loop** (`engine_execution.rs:504`) masks, serializes, builds the
  context (`handled_languages` at 552, `multi_engine` at 587), calls
  `engine.execute` (610), unmasks (623), emits the capture aux event (638-675),
  then reparses and reconciles (831). Each engine receives the previous
  engine's reconciled output, so coalescing has to happen after the loop with
  source positions taken from the original document.
- **No kernel-free test seam in the live loop today.** `ipynb_stored_replay.rs`
  gets kernel-free coverage only because replay reads stored outputs
  (`stored.rs:229`); `execute_blocks_inner` calls the global daemon directly.
- The record types are EXR-4\'s: `EngineRecord`, `RecordBuilder`, `UnitResult`,
  `AuthorTexts` and `coalesce(engine_records, unit_order)` (EXR-4 Phase 1 and Phase 3).
  This plan calls them and defines none of them.

## Design decisions (from the epic; do not re-litigate)

- Additive: the live path and its output are untouched; the record is a side
  product, built only when asked for.
- Ids use Positron\'s format and content hash (epic decisions 13 and 39: the label
  segment and the index can differ, matching uses the hash); the record carries
  `quarto_engine`, per-cell kernel metadata, and the execution-input hash. All of
  that is EXR-4\'s schema; this plan only fills it in.
- **What the builder is given** (epic decisions 35, 36 and 38). It consumes the stage\'s
  single unit list from `ExecutionContext.units`, whose ids this plan\'s Phase 2 fills by
  calling EXR-4\'s `fill_ids` right after the stage scan (span counter document-wide); it
  does no scan of its own. It receives the stage\'s author-text mapping,
  `ExecutionContext.author_texts` (EXR-4\'s `AuthorTexts`: the author text and its content
  hash for every unit), because crossref sugaring strips the label and caption lines from
  the text the engine sees, so the builder cannot hash the engine-visible text. And it
  takes the engine\'s declared hash rule from `ExecutionEngine::hash_definition()`, stores
  that full `HashDefinition` in the record\'s `quarto_engines[].hash_definition` entry, and
  never matches on an engine name.
- Side data folds into notebook-level metadata: `supporting_files`, `includes`,
  `filters`, `preserve`, `needs_postprocess`, `html_dependencies`, `pandoc`,
  `resource_files`. Provenance (engine version, kernelspec) is recorded but never
  used for matching.
- Plain `q2 render` writes no record. Records are built when the observer wants
  them (preview, hub) or when the merged metadata has `execute: {record: true}`
  (epic decision 24; `-M 'execute:{record:true}'` is the command-line form).
- The legacy `EngineCapture` payload sent to the preview and the hub must not change
  (`ExecuteResult.record` stays `#[serde(skip)]`). Never run the legacy splice and an
  id-based path over the same record.
- **The trace carries the record through an explicit trace-only field** (2026-10-29
  review, D2: `ReplayEngine` stays and, after the flip, replays records; EXR-6). The
  field is not obtained by removing `serde(skip)` from `ExecuteResult.record`: that
  would enlarge every legacy capture. Instead the engine stage emits each engine's own
  (uncoalesced) record as an aux event next to that engine's capture event, and only
  the trace observer stores it, on that engine's `engine_capture` entry, in a new
  optional field named `engine_capture.record` (`serde(default,
  skip_serializing_if = "Option::is_none")`, so old traces still parse and the preview and
  hub recorders, which never set it, write the same bytes as before). `EngineCapture` has
  no `deny_unknown_fields` (`crates/quarto-trace/src/lib.rs:187-220`, verified), so old and
  new readers interoperate. The per-engine record is what `ReplayEngine` needs, because it
  is registered under one engine\'s name. The trace observer is `JsonTraceObserver`
  (`crates/quarto-core/src/stage/trace.rs:49`); it returns `true` from
  `wants_execution_record`. Its `on_auxiliary_data` (`:234-240`) already routes
  `ENGINE_CAPTURE_KIND` and pushes to `state.doc.engine_captures`, and
  `PipelineObserver::on_auxiliary_data` (`observer.rs:210`) already carries aux events, so
  the only new trait method is `wants_execution_record`. Pairing: the record event is emitted
  right after that engine\'s capture event with the same `run_index` as the aux `index`
  and the engine name in the payload; the observer remembers which `engine_captures` slot
  each `run_index` filled and attaches the record to that slot, checking the engine name.
  An unknown `run_index` or a name mismatch is a warning and the record is dropped; it is
  never attached to the "last pushed" entry (fragile: a capture whose serialization failed
  is skipped). Trace size grows (base64 figures); accepted.

## Out of scope

- Presenting from the record in production (EXR-6), including `ReplayEngine` consuming
  the trace field (EXR-6).
- The hub document format, MIME, readers and the writers\' use of the record (EXR-13);
  preview consumption (EXR-14). The handoff type those writers receive is defined here
  (the Phase 2 task "Record handoff to the capture writers").
- Julia, knitr and marimo records (EXR-8, EXR-11, EXR-9).
- The `kernel_for` policy itself (JOL-2b; JOL-3 is deferred); this plan records whichever
  kernel the seam returns.

## Phased checklist (tests first)

### Phase 0: failing tests and fixtures

Small tasks, tests first. The unit-executor seam is introduced here, in 0a (EXR-2\'s
`plan_units` precedes it, and this seam later refactors the same loop).

**0a. The unit-executor seam**

- [ ] Test first: `execute_blocks_with_injected_executor_matches_daemon_shape` (a stub
  returns a stdout stream; the markdown equals the existing `render_cell` output). Red.
- [ ] Introduce the seam in `text_execute.rs` (a trait or closure taking a code string and
  returning `KernelExecuteResult`), defaulting to the daemon, so tests can inject canned
  results. The test goes green; the live markdown tests are unchanged.

**0b. Builder call sites** (written against the injected executor; red until Phase 1)

- [ ] `record_has_one_cell_per_executed_unit_in_document_order`
- [ ] `record_cell_ids_are_content_hashes_including_option_lines` (fixture from EXR-4\'s
  Positron conformance set)
- [ ] `record_ids_come_from_the_stage_units` (the builder uses `ExecutionContext.units`
  as given, ids included, and does no scan of its own; the test supplies filled ids, the
  stage test in Phase 2 proves the stage fills them; epic decision 38)
- [ ] `record_content_hash_uses_author_text_for_labeled_cell` (a `fig-` label plus
  `fig-cap`: the stored `quarto_content_hash` is of the author\'s text including those
  lines, supplied by the stage\'s mapping; epic decision 36)
- [ ] `record_engine_entry_carries_full_hash_definition` (a stub engine that declares a
  custom `HashDefinition`: the entry stores the whole declaration, and the builder has no
  engine-name branch; epic decision 35)
- [ ] `record_keeps_hidden_cells` (`include: false`, `echo: false`, `output: false`)
- [ ] `record_outputs_are_verbatim` (stream, `execute_result`, `display_data` with image
  plus text, error; `execute_result` versus `display_data` preserved)
- [ ] `record_notes_kernel_per_cell` (kernel name and language on each cell)
- [ ] `ceded_and_unowned_fences_are_not_record_cells`
- [ ] `inline_units_are_recorded_as_one_line_cells` (from EXR-3; span ids are
  `{n}-{hash8}-inline` with the document-wide counter)

**0c. Gating**

- [ ] `no_record_is_built_when_observer_does_not_want_one` (a counting hook proves zero
  builder calls for a default observer)
- [ ] `legacy_capture_payload_is_byte_identical_with_and_without_record` (the `result`
  JSON of the `EngineCapture` aux event does not change)

**0d. Trace (D2)**

- [ ] `trace_engine_capture_carries_per_engine_record_when_tracing` (a `trace: true`
  render of a Jupyter fixture with a stubbed executor stores that engine\'s record in
  `engine_capture.record`)
- [ ] `trace_record_pairs_by_run_index_not_last_pushed` (two engines; the record of
  run 0 lands on run 0\'s entry even when the events are interleaved; a name mismatch is
  dropped with a warning)
- [ ] `trace_without_record_field_still_parses` (a committed pre-change trace
  deserializes)
- [ ] `preview_and_hub_capture_bytes_unchanged_when_trace_field_exists` (the recorders
  never set the field)

**0e. Side data and coalescing**

- [ ] `engine_stage_folds_result_side_data_into_notebook_metadata` using the echo and
  behave fixture engines (`crates/quarto-core/tests/integration/echo_engine_e2e.rs`,
  `behave_engine_e2e.rs`) for includes, filters, preserve, `needs_postprocess`.
- [ ] `two_engines_coalesce_in_source_order_with_quarto_engine` and
  `coalescing_is_deterministic` (two runs, identical bytes, EXR-4 serializer).

**0f. Shadow harness skeleton**

- [ ] `jupyter_record_shadow.rs` (integration test), initially red, with fixtures: stdout;
  plain-text result; HTML; PNG plus text (figure file names asserted); error with
  `error: true`; `echo: false`; `output: false`; `include: false`; `warning: false` with
  stderr; several cells where an early cell is hidden and a later one plots;
  markdown-only document; a ceded `{ojs}` block. Real-kernel variants are gated like
  `jupyter_integration.rs` (skip with a note when ipykernel is missing).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (only the new tests listed above may be red).

### Phase 1: record accumulation in the Jupyter engine

- [ ] Accumulate a record cell per executed unit inside `execute_blocks_inner`
  without touching the string building: the `KernelExecuteResult` is converted to
  nbformat output JSON (lossless `MimeBundle`; multiline text kept as the kernel
  sent it) and appended through EXR-4\'s `RecordBuilder` as a `UnitResult`. The builder\'s
  inputs are the stage\'s unit list (`ExecutionContext.units`), the author-text mapping
  (`ExecutionContext.author_texts`, a field beside `units`, set by the stage) and the
  engine\'s `hash_definition()`.
- [ ] Add `record: Option<EngineRecord>` to `ExecuteResult` with `#[serde(skip)]`
  and `ExecutionContext.want_record: bool` (builder `with_want_record`).
- [ ] Phase 0 unit tests green; the live markdown tests
  (`jupyter_integration.rs`, `ipynb_stored_replay.rs`, `engine_output_parity.rs`)
  unchanged and green.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 2: stage plumbing and coalescing

- [ ] Add `PipelineObserver::wants_execution_record` (default `false`; the preview
  recorder and the trace observer return `true`), and set `want_record` from it in
  `EngineExecutionStage` next to `with_handled_languages` (about line 582). The stage
  also sets it when the merged metadata has `execute: {record: true}`; test that a
  plain render with the key builds a record and one without it does not.
- [ ] Unit ids and author texts in the stage (tests first:
  `stage_fills_unit_ids_before_any_engine_runs`, `labeled_cell_id_uses_author_text`,
  `author_texts_has_one_entry_per_unit`): right after the stage\'s one scan (EXR-2 Phase 4
  puts the list, ids still empty, on `ExecutionContext.units`), obtain each unit\'s author
  text from the original text range (or the pre-sugaring AST; the mechanism is EXR-4\'s
  first task), build EXR-4\'s `AuthorTexts`, call EXR-4\'s `fill_ids(units, &author_texts)`,
  and put both on the context for the builder (epic decision 36). The ids are filled
  whenever the stage scans, whether or not a record is wanted, because the wire (EXR-7)
  carries them; the author texts are built only when a record is wanted.
- [ ] Inside the engine loop, next to the capture event at line 638, emit each
  engine\'s uncoalesced record as its own aux event (a per-engine kind constant beside
  `ENGINE_CAPTURE_KIND`), after that engine\'s capture event and with the same
  `run_index`. The trace observer stores it on that engine\'s `engine_capture` entry in the
  new optional trace-only field `engine_capture.record` (add `record: Option<...>` to
  `EngineCapture` in `crates/quarto-trace/src/lib.rs`, `serde(default,
  skip_serializing_if = "Option::is_none")`), pairing by `run_index` as described above;
  the preview and hub recorders ignore the event.
- [ ] After the engine loop, coalesce the per-engine records with EXR-4\'s
  `coalesce(engine_records, unit_order)`, where `unit_order` is the stage\'s single unit
  list (no source-position sort, no renumbering), fold side data, and emit the result with
  a new aux kind `ENGINE_RECORD_KIND` (`"EngineRecord"`) next to `ENGINE_CAPTURE_KIND`
  (line 52). Observers that do not know a kind ignore it (see the comment at 636).
- [ ] **Record handoff to the capture writers** (tests first; EXR-13 and EXR-15 cite this
  task by name). Today nothing consumes `ENGINE_RECORD_KIND`: `CaptureCollector` returns
  early for every kind but `ENGINE_CAPTURE_KIND` (`engine/preview_record.rs:70-73`), and
  `record_capture` returns `Vec<EngineCapture>` (`:139`). Add the crate-visible type
  `CaptureRun { captures: Vec<EngineCapture>, record: Option<Record> }` and a function
  `record_capture_run(...) -> Result<CaptureRun, PipelineError>`; make `record_capture` a
  thin wrapper that returns `.captures`, so its many test callers
  (`capture_splice_seam.rs:139`, `engine_output_parity.rs:66` and others) do not change.
  `CaptureCollector` also collects the `ENGINE_RECORD_KIND` payload (one coalesced record
  per run) into `CaptureRun.record`; the preview recorder returns `true` from
  `wants_execution_record`, so a record is built. The capture writers take a
  `CaptureRun`: the preview writer, the hub guest writer and `provide-hub`
  (`quarto-hub-provider/src/execute.rs:332`; the exact writer list is EXR-13\'s, which cites
  this type). The `-M` path (`execute.rs:346-352`) overwrites `captures[0].input_qmd` with
  overlay-free bytes; the record equivalent is that the record\'s unit hashes, document
  hashes and `quarto_execution_hash` are the overlay-free ones, with the overlay\'s
  execution-relevant difference in `quarto_overlay` (EXR-4). Decide first whether the
  stage can compute both hash sets in the one run or whether the provider recomputes them
  from the `source_project` it already discovers at `execute.rs:346` (UNVERIFIED which is
  cheaper). Tests: `capture_run_carries_the_record`,
  `record_capture_wrapper_returns_the_same_captures`,
  `provide_hub_with_overlay_stores_overlay_free_record_hashes`,
  `legacy_capture_bytes_unchanged_when_record_collected`.
- [ ] Write the file for a plain render (epic decision 33). When the merged metadata has
  `execute: {record: true}`, write the coalesced record to
  `.quarto/execution-records/<doc path without extension>.ipynb` (epic decision 3;
  project-relative, a `.quarto/` beside a standalone document) using EXR-4\'s
  deterministic serializer. Tests: a plain render with the key writes the file at that
  path; one without it writes nothing; rendering twice produces identical bytes; the
  hub writers of EXR-13 are untouched.
- [ ] Phase 0 gating, trace and coalescing tests green.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (and `-p quarto-trace` for the new field).

### Phase 3: the shadow equivalence test

- [ ] `present(record)` (EXR-1 presenter, driven by the same units and the same
  `execute_scope` the engine used) equals the live markdown for every fixture,
  including written figure file names and the `supporting_files` directory.
  Test-only: the comparator lives in the integration test, not in production.
- [ ] A negative control: perturb one output in the record and assert the
  comparator fails, so the harness cannot pass vacuously.
- [ ] Record the serialized record size for the figure fixture and for a
  twenty-cell document in this plan (an input to bd-sr73 and to the EXR-4
  figure decision; no decision is made here).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 4: verification

- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core`; recommend (do not run unprompted) the
  nextest run on direct dependents (`cargo tree -i quarto-core`).
- [ ] Render this plan with `q2 render --strict` before any push.

## Exit criteria

- With an observer that wants records, a Jupyter render yields a coalesced
  record whose presentation equals the live markdown on every shadow fixture.
- Default renders build no record and allocate nothing extra.
- Legacy capture bytes are unchanged (named test), and a `trace: true` render stores the
  per-engine record on the trace's `engine_capture` entry while old traces still parse.
- EXR-6 can start: it may assume a verified `present(record) == live` baseline and a
  trace that carries the record.

## Risks and open questions

- **Presenter inputs outside the record.** `execute_scope`, the document source
  and the output format are read from the pipeline at present time. EXR-6 must
  supply them identically for replay. Decided (epic decision 27): the record does
  not store them, so it stays format-neutral; guests in WASM have the metadata.
- **Hidden-cell bookkeeping.** If the builder skips a hidden cell, figure names
  shift. The `record_keeps_hidden_cells` test guards this.
- **Memory.** Base64 images are held twice while a record is built (kernel
  result plus record). Acceptable behind the observer gate; measure in Phase 3.
- **`text/markdown` is not handled by the Rust presenter** (see the comment at
  `text_execute.rs:725-727`), so shadow equality does not exercise it. The Julia
  parity suite in EXR-8 is where that gap is measured.
- **Per-cell kernel (and a reopened JOL-3).** The builder takes the kernel from the
  seam and records it per cell, so a multi-kernel document, if JOL-3 is ever reopened
  (it is deferred), records each cell\'s kernel with no change here.
- **Trace size.** Tracing now stores base64 figures. Accepted (2026-10-29 review); the
  trace is opt-in (`trace: true`).

## References

- `crates/quarto-core/src/engine/jupyter/text_execute.rs`, `stored.rs`, `execute.rs`
- `crates/quarto-core/src/stage/stages/engine_execution.rs`,
  `stage/observer.rs`, `engine/preview_record.rs`, `engine/context.rs`,
  `stage/trace.rs` (`JsonTraceObserver`), `crates/quarto-trace/src/lib.rs`
  (`EngineCapture`)
- `crates/quarto-core/tests/integration/engine_output_parity.rs` (shape-parity
  precedent), `ipynb_stored_replay.rs` (kernel-free replay precedent)
- [The epic](2026-10-28-execution-records-epic.md);
  [Plan 7c](2026-07-08-plan7c-ipynb-content-processor.md) (stored replay, complete)
