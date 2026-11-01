---
title: 'EXR-5: Jupyter emits execution records (shadow mode)'
date: 2026-10-28
description: 'Makes the Jupyter engine build an in-memory engine execution record next to its unchanged live markdown path, coalesces records at the end of the engine stage, and proves present(record) equals the live markdown on fixtures before anything depends on the record.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (EXR-5).
**Depends on:** [EXR-1](2026-10-28-exr-01-presenter-extraction.md) (the extracted
presenter), [EXR-3](2026-10-28-exr-03-jupyter-iee.md) (spans in the unit loop, and the
fake kernel these tests reuse; the `Unit` loop and the `kernel_for(unit)` seam are
[EXR-2](2026-10-28-exr-02-unit-scanner.md)\'s), [EXR-4](2026-10-28-exr-04-record-format.md)
(Phases 0 to 4: the record type, builder, ids and writer; Phases 5 and 6 are not needed). Soft: [JOL-2b](2026-10-28-jol-2b-ownership-gate.md)
(per-cell kernel metadata is recorded whatever kernel is chosen).
**Blocks:** [EXR-7](2026-10-28-exr-07-engine-wire.md) (it introduces
`ExecuteResult.record`, `want_record` and the coalescing hook that EXR-7 sets and
calls), [EXR-6](2026-10-28-exr-06-jupyter-flip.md) (the flip),
[EXR-13](2026-10-28-exr-13-hub-wire.md) (the hub needs something to carry). Per the dependency table in the epic, it also blocks EXR-15.
**Lane:** A (`workspace-4`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3-4.

**Merge gate (2026-10-31, Gordon).** Merge into the integration branch at the end of Phase 2
(the `record` field and `want_record`, unit ids and author texts in the stage, coalescing, the
"Record handoff to the capture writers" task, the file write) with `status: in-progress`, then
continue with Phases 3 and 4 on the same branch. EXR-7\'s mapping half, the writer half of
EXR-13, EXR-15, 9C and the Julia B2 need only Phase 2; EXR-6 needs all of this plan, because
Phase 3 is its baseline.

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

Done 2026-10-31. So the tests compile while they are red, Phase 0 also added the inert
surface they call, with no behavior behind it until Phases 1 and 2: `ExecuteResult.record`
(`serde(skip)`), `ExecutionContext.want_record` and `.author_texts`,
`PipelineObserver::wants_execution_record` (default `false`), the aux kinds
`ENGINE_CAPTURE_RECORD_KIND` and `ENGINE_RECORD_KIND`, the trace-only `EngineCapture.record`
(`quarto-trace`, with every struct literal in the workspace updated), and
`text_execute::execute_qmd_with(input, ctx, &HashDefinition)`, which the engine calls with
its `hash_definition()` (`jupyter_hash_definition()` is now a free function).

**0a. The unit-executor seam**

- [x] Test first: `execute_blocks_through_the_fake_kernel_match_render_cell_shape` (the
  kernel returns a stdout stream; the markdown equals the existing `render_cell` output).
  Green at once, because the seam already existed (next item).
- [x] The seam is EXR-3\'s fake kernel (`engine/jupyter/fake_kernel.rs`, 2026-10-31): canned
  results travel through the real daemon and session, so no injected closure was added and
  the loop under test is the production loop. The record tests live in `text_execute.rs`
  (`record_tests`), beside EXR-3\'s `span_tests`, whose context helpers they share. Script
  keys for a cell carry the newline before the closing fence (`cell("x = 1")`), since the
  scanner\'s cell code keeps it.

**0b. Builder call sites** (written against the fake kernel; red until Phase 1)

- [x] `record_has_one_cell_per_executed_unit_in_document_order`
- [x] `record_cell_ids_are_content_hashes_including_option_lines` (the
  `label-line-and-unicode` vector from EXR-4\'s
  `tests/fixtures/records/content-hash-vectors.json`)
- [x] `record_ids_come_from_the_stage_units` (the builder uses `ExecutionContext.units`
  as given, ids included, and does no scan of its own; the test supplies filled ids, the
  stage test in Phase 2 proves the stage fills them; epic decision 38)
- [x] `record_content_hash_uses_author_text_for_labeled_cell` (a `fig-` label plus
  `fig-cap`: the stored `quarto_content_hash` is of the author\'s text including those
  lines, supplied by the stage\'s mapping; epic decision 36)
- [x] `record_engine_entry_carries_full_hash_definition` (a custom `HashDefinition` passed
  to `execute_qmd_with`: the entry stores the whole declaration, and the builder has no
  engine-name branch; epic decision 35)
- [x] `record_keeps_hidden_cells` (`include: false`, `echo: false`, `output: false`)
- [x] `record_outputs_are_verbatim` (stream, `execute_result`, `display_data` with image
  plus text, error; `execute_result` versus `display_data` preserved)
- [x] `record_notes_kernel_per_cell` (kernel name and language on each cell)
- [x] `ceded_and_unowned_fences_are_not_record_cells`
- [x] `inline_units_are_recorded_as_one_line_cells` (from EXR-3; span ids are
  `{n}-{hash8}-inline` with the document-wide counter)
- [x] Added: `no_record_is_built_when_not_wanted` (engine level: `want_record: false`
  yields `record: None`; green at once) and `a_span_only_document_records_its_span`.

**0c. Gating** (stage level, in `tests/integration/engine_record_stage.rs`, driven by a
stub engine that builds its record from `ExecutionContext.units` and `author_texts` with
EXR-4\'s `RecordBuilder` exactly as the Jupyter engine will; the shared pipeline driver is
`tests/integration/engine_stage_driver.rs`, a public twin of `record_capture`\'s private
collector that takes any observer)

- [x] `no_record_is_built_when_observer_does_not_want_one` (the stub\'s builder counter
  proves zero builder calls for a default observer; green at once)
- [x] `legacy_capture_payload_is_byte_identical_with_and_without_record` (the `result`
  JSON of the `EngineCapture` aux event does not change; red until Phase 2)

**0d. Trace (D2)**

- [x] `trace_engine_capture_carries_per_engine_record_when_tracing` (a `JsonTraceObserver`
  run over a stub-engine fixture stores that engine\'s record, as a one-engine record
  notebook, in `engine_capture.record`; red until Phase 2). Added:
  `trace_pairs_each_engines_record_with_its_own_capture` (two engines, end to end).
- [x] `trace_record_pairs_by_run_index_not_last_pushed` (unit test in `stage/trace.rs`:
  the record of run 0 lands on run 0\'s entry even when the events are interleaved; a name
  mismatch and an unknown run index are dropped; red until Phase 2)
- [x] `trace_without_record_field_still_parses` (`crates/quarto-trace/tests/roundtrip.rs`;
  green at once, a guard)
- [x] `preview_and_hub_capture_bytes_unchanged_when_trace_field_exists` (same file; the
  recorders never set the field; green at once, a guard)

**0e. Side data and coalescing** (red until Phase 2)

- [x] `engine_stage_folds_result_side_data_into_notebook_metadata` for includes, filters,
  preserve and `needs_postprocess`. The echo and behave fixture engines the plan named are
  TS engines that return no record until EXR-7 carries it over the wire, so the stub engine
  of 0c stands in (it sets the side data on its `ExecuteResult` and leaves the entry input
  empty, which is what the stage must fold); EXR-7 can point the test at the fixtures.
- [x] `two_engines_coalesce_in_source_order_with_quarto_engine` and
  `coalescing_is_deterministic` (two runs, identical bytes, EXR-4 serializer).

**0f. Shadow harness skeleton**

- [x] `jupyter_record_shadow.rs` (integration test), one red test per fixture under
  `tests/fixtures/jupyter-record-shadow/` (README there): stdout; plain-text result; HTML;
  PNG plus text (figure file names asserted); error with `error: true`; `echo: false`;
  `output: false`; `include: false`; `warning: false` with stderr; several cells where an
  early cell is hidden and a later one plots; markdown-only document; a ceded `{ojs}`
  block. Every case is gated like `jupyter_inline_expressions.rs` (skip with a note when
  no Python kernelspec is found); the comparator is Phase 3\'s.
- [x] Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (only the new tests listed above may be red).

### Phase 1: record accumulation in the Jupyter engine

Done 2026-10-31.

- [x] Accumulate a record cell per executed unit inside `execute_blocks_inner`
  without touching the string building: the `CellResult` is converted to nbformat
  output JSON (`CellOutput::to_nbformat` and `CellResult::nbformat_outputs` in
  `presenter/output.rs`: the raw `MimeBundle`, stream text as the kernel sent it,
  `metadata` always an object; the inverse is `ipynb_replay`\'s `stored_result_for_cell`)
  and appended through EXR-4\'s `RecordBuilder` as a `UnitResult` (`push_unit_result`).
  The builder\'s inputs are the stage\'s unit list (`ExecutionContext.units`), the
  author-text mapping (`ExecutionContext.author_texts`) and the engine\'s
  `hash_definition()`, threaded from `execute_qmd_with`. A run unit maps to its stage
  index by the document-wide counter `locate_units` preserves (`index`, `span_index`);
  a unit an earlier engine\'s output introduced has no stage index and is not recorded,
  since `coalesce` could not place it. Spans: `evaluate_span` is now `run_span` (the raw
  result, recorded) plus `span_text` (the live replacement, unchanged). The entry
  carries `kernelspec: {name, language}` as provenance, `can_freeze: true`, the owned
  languages and the merged `ctx.metadata` as the document-hash input; side data is left
  for the stage to fold (Phase 2). A run that executes nothing returns an empty record
  when one is wanted; a run with `want_record` and no stage units fails with
  `JupyterError::RecordFailed` rather than record from a local scan.
- [x] `record: Option<EngineRecord>` on `ExecuteResult` with `#[serde(skip)]` and
  `ExecutionContext.want_record: bool` (builder `with_want_record`), both added in Phase 0.
- [x] Phase 0 unit tests green; the live markdown tests
  (`jupyter_integration.rs`, `ipynb_stored_replay.rs`, `engine_output_parity.rs`)
  unchanged and green.
- [x] Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 2: stage plumbing and coalescing

Done 2026-10-31. Stage-level tests live in `tests/integration/engine_record_stage.rs`
(stub engines through `engine_stage_driver.rs`); the snapshot tests in
`stage/stages/pre_engine_sugaring.rs`; the trace pairing test in `stage/trace.rs`; the
overlay tests in `record/overlay.rs` and `quarto-hub-provider/tests/integration/execute.rs`.

- [x] `PipelineObserver::wants_execution_record` (default `false`; the preview recorder
  `CaptureCollector` and `JsonTraceObserver` return `true`); `EngineExecutionStage` sets
  `want_record` from it next to `with_handled_languages`, or when the merged metadata has
  `execute: {record: true}` (tests `execute_record_true_builds_a_record_for_a_plain_observer`
  and `no_record_is_built_when_observer_does_not_want_one`).
- [x] Unit ids and author texts in the stage (tests `stage_fills_unit_ids_before_any_engine_runs`,
  `labeled_cell_id_uses_author_text`, `author_texts_has_one_entry_per_unit`): right after the
  one scan, `scan_and_fill_ids` builds EXR-4\'s `AuthorTexts` with
  `author_texts(&units, &ctx.sugared_blocks)`, calls `fill_ids`, and puts both on the context
  (`with_units`, `with_author_texts`). The ids are filled whether or not a record is wanted;
  the author texts are built with them in every case, since `fill_ids` reads them (the plan\'s
  "only when a record is wanted" was not possible), and they are attached to the context in
  every case too, a clone of one small vector.
  **The snapshot** is `StageContext.sugared_blocks`, filled by `pre_engine_sugaring` before
  `desugar_blocks` runs: `executable_block_texts` walks the same containers the pass descends
  into (block quotes, divs, lists, definition lists) and records every braced executable
  block\'s text, in document order, untouched blocks included; the walk after the pass gives
  the sugared texts, zipped block for block (tests `snapshot_lists_untouched_blocks_too`,
  `snapshot_has_the_authors_raw_lines_for_a_sugared_cell`,
  `snapshot_includes_cells_inside_containers`). The open checks are settled: a sugared cell
  inside a container keeps its author text
  (`sugared_cell_inside_a_container_keeps_its_author_text`), and a second engine iteration
  reuses the one snapshot and the one scan (`second_engine_reuses_the_one_snapshot`). A
  non-crossref label stays in both texts, so nothing special is needed for it.
- [x] Source-of-inputs tests: `code_edit_in_an_included_file_changes_the_hashes_and_prose_does_not`
  (an `{{< include >}}`\'d cell is a unit; its edit changes the unit and execution hashes, a
  prose edit changes neither) and `equivalent_metadata_spellings_hash_alike` (key order and
  quoting hash alike, a changed value does not). The stage feeds the hashes the merged
  `ctx.metadata`, lowered by the new `ExecutionContext::document_metadata_json`, which the
  Jupyter engine and the stub engines pass as `EngineEntryInput::document_metadata`.
- [x] Inside the engine loop, right after the capture event, `emit_engine_record` sends each
  engine\'s uncoalesced record (as a one-engine record notebook, so a reader needs one shape)
  under `ENGINE_CAPTURE_RECORD_KIND` with the same `run_index` and the engine name in the
  payload. `JsonTraceObserver` remembers which `engine_captures` slot each `run_index` filled
  and stores the record on that slot in the new trace-only `EngineCapture.record`
  (`crates/quarto-trace/src/lib.rs`, `serde(default, skip_serializing_if)`), checking the
  engine name; an unknown run index or a name mismatch is a warning and the record is dropped
  (`trace_record_pairs_by_run_index_not_last_pushed`,
  `trace_engine_capture_carries_per_engine_record_when_tracing`,
  `trace_pairs_each_engines_record_with_its_own_capture`). The preview and hub recorders
  ignore the kind.
- [x] After the loop, the stage coalesces with `coalesce(engine_records, unit_order)` over the
  one unit list, sets `quarto_source` (project-relative, forward slashes), and emits the
  record under `ENGINE_RECORD_KIND`. Side data is folded per engine by `fold_side_data`
  (includes, filters, preserve, `needs_postprocess`, `html_dependencies`, `pandoc`,
  `metadata`, `resource_files`, `warnings`) before the drains move it out of the result; no
  engine fills these itself (`engine_stage_folds_result_side_data_into_notebook_metadata`,
  `two_engines_coalesce_in_source_order_with_quarto_engine`, `coalescing_is_deterministic`).
- [x] **Record handoff to the capture writers.** `preview_record::CaptureRun { captures,
  record: Option<Record> }` and `record_capture_run(...)`; `record_capture` is a thin wrapper
  returning `.captures`, so its callers are unchanged. `CaptureCollector` collects the
  `ENGINE_RECORD_KIND` payload and returns `true` from `wants_execution_record`.
  `provide-hub` (`quarto-hub-provider/src/execute.rs`) now runs `record_capture_run` and
  `write_capture_doc` takes the `CaptureRun` (it stores the captures only until EXR-13 says
  how the hub document carries the record); the preview cache and the hub guest writer keep
  calling `record_capture` until EXR-13 and EXR-14 take the run. The `-M` question is
  settled: the stage cannot compute the overlay-free hash set in the one run, because the
  `-M` overlay is a runtime overlay (`RenderOverlay::apply`) that `MetadataMergeStage` folds
  over the document\'s metadata before the stage sees it, and the merge is not invertible (an
  overridden key loses its source value). The provider recomputes from the source project,
  with no extra pipeline run: `compute_input_qmd` is now a wrapper over
  `compute_source_inputs`, which returns the overlay-free merged metadata next to the bytes
  (`crates/quarto-core/src/engine/preview_record.rs`), and `overlay_free_run` rehashes the
  record with `record::rehash_record` (each entry\'s stored `hash_definition`, unit hashes
  untouched) and notes the overlay\'s execution-relevant keys in `quarto_overlay`
  (`record::overlay_difference`). Tests: `capture_run_carries_the_record`,
  `record_capture_wrapper_returns_the_same_captures`,
  `provide_hub_with_overlay_stores_overlay_free_record_hashes`,
  `legacy_capture_bytes_unchanged_when_record_collected`, plus
  `rehash_restores_the_overlay_free_hashes` and
  `overlay_difference_keeps_only_execution_relevant_keys`.
- [x] The file for a plain render (epic decision 33): with `execute: {record: true}`,
  `write_record_file` writes the coalesced record to
  `.quarto/execution-records/<doc path without extension>.ipynb` under the project
  directory (a `.quarto/` beside a standalone document) through the runtime, with
  `write_record`. Tests: `execute_record_true_writes_the_record_file` (the path, a parseable
  record, identical bytes on a second render) and `plain_render_writes_no_record_file` (an
  observer that wants the record does not write it; only the document\'s key does). The hub
  writers are untouched.
- [x] Phase 0 gating, trace and coalescing tests green.
- [x] Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (and `-p quarto-trace` for the new field, and
  `-p quarto-hub-provider` for the overlay path).

### Phase 3: the shadow equivalence test

Done 2026-10-31, in `tests/integration/jupyter_record_shadow.rs`, against the real kernel
(the Positron venv; every case skips with a message when no Python kernelspec is found).
The image fixtures display a one-pixel PNG through `IPython.display.Image`, so ipykernel is
the only requirement (matplotlib was not in the venv and is not needed).

- [x] `present(record)` equals the live markdown for every fixture, written figure file
  names and the `supporting_files` directory included. The comparator (test-only) drives
  the EXR-1 presenter the way the live loop does: the units scanned from the capture\'s
  `input_qmd`, matched to record cells by kind and document-wide counter (the engine\'s
  own rule); the same `execute:` scope, from `merge_execute_scope` (now `pub`, re-exported
  from `stage::stages`) over the input\'s front matter; `partition_cell_options`,
  `resolve_cell_options` and `CellVisibility::resolve` for visibility; `render_cell` over
  `CellResult::from_nbformat` (the stored-notebook replay\'s conversion, moved to
  `presenter/output.rs` so the record and `ipynb_replay` share it) with an `FsFigureSink`
  beside a second temp copy of the document; `render_inline` over the span\'s raw bundle;
  `splice_mixed`. Figure files are compared by relative name and bytes
  (`shadow_png_plus_text` asserts `figure-html/cell-1-output-1.png`,
  `shadow_hidden_then_plot` asserts `cell-2-output-1.png` after a hidden first cell).
- [x] The negative control (`shadow_negative_control_detects_a_perturbed_output`): a
  perturbed stream text and an emptied cell list both fail the comparator.
- [x] Serialized record sizes (`write_record`, pretty JSON), measured 2026-10-31 with
  `shadow_record_sizes_for_the_plan`: the figure fixture (`png-plus-text`, one
  one-pixel PNG plus its `text/plain`) is 2964 bytes for 1 cell; `twenty-cells` (twenty
  `print` cells) is 13464 bytes for 20 cells, about 670 bytes per cell of overhead
  (ids, hashes, the declaration). A real figure adds its base64 size on top. An input to
  bd-sr73 and the EXR-4 figure decision; no decision is made here.
- [x] Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (shadow suite run with the kernel: 57 tests across the
  shadow, stored-replay, replay-engine and stage files, all passed).

### Phase 4: verification

Done 2026-11-01.

- [x] `cargo clippy --all-targets -- -D warnings` and `cargo nextest run` on `quarto-core`,
  `quarto-trace` and `quarto-hub-provider` (the three crates the plan touched): clippy clean;
  7013 passed, 36 skipped (the kernel-gated shadow cases skip without a kernel; with the
  Positron venv on `PATH` they ran, 57 passed). The nextest run on the direct dependents of
  `quarto-core` (`cargo tree -i quarto-core`) is recommended in the handoff, not run here;
  `cargo check --all-targets` on `quarto-preview`, `quarto` and `quarto-hub-provider` passed.
- [x] Rendered with `q2 render --strict` (only the known unrelated Q-12-13 errors; none name
  this file).

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
