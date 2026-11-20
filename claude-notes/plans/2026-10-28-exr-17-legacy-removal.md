---
title: 'EXR-17: legacy capture removal (trigger-gated)'
date: 2026-10-28
description: 'Defines the exact trigger for deleting the structural capture splice and the legacy capture reader, what is deleted and what is kept (ReplayEngine and the trace stay), and the tests that prove nothing regressed; nothing here starts until the trigger is met, and the trigger carries no date.'
status: draft  # blocked on the trigger below; do not start early
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** [EXR-6](2026-10-28-exr-06-jupyter-flip.md), [EXR-8](2026-10-28-exr-08-julia.md),
[EXR-9](2026-10-28-exr-09-marimo.md), [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md),
[EXR-12](2026-10-28-exr-12-knitr-k2-inline.md), [EXR-13](2026-10-28-exr-13-hub-wire.md) (both halves, 13a and 13b),
[EXR-14](2026-10-28-exr-14-preview-splice.md), [EXR-15](2026-10-28-exr-15-hub-integration.md)
**Blocks:** nothing
**Lane:** D (`workspace-7`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2, plus whatever time the owner lets pass before the trigger holds

> **For implementers:** this plan is deliberately small and must not be started until
> the trigger holds. Its job is to make deletion safe, not fast. The earlier plans keep the
> legacy code isolated (epic decision 10) so that this plan is mostly deleting what the
> compiler points at.

## Goal

Retire the legacy mechanisms the epic replaced, once nothing depends on them, so that
one record mechanism remains and the failure class of running two mechanisms on one
capture (bd-mu9i0bct, bd-42202ipf) cannot recur.

## Background (verified in code on 2026-10-28, workspace-1 at `2ee0bd966`)

What exists that the record replaces, with where it lives:

- **The structural splice.** `crates/quarto-core/src/engine/capture_splice.rs`
  (1088 lines), declared by `pub mod capture_splice;` (`engine/mod.rs:57`). Its legacy
  items, all deleted with the module:
  - public: `derive_cell_outputs` (`:233`), `CellOutputMap` (`:189`), `apply_capture_splice`
    (`:448`), `apply_capture_splices` (`:469`), and `splice_cells` (`:370`, which
    `tests/integration/capture_splice_engines.rs:26` imports);
  - private helpers that only the splice uses: `is_echo_of` (`:116`), `is_cell_wrapper`
    (`:134`), `is_engine_output_block` (`:162`) and `structural_eq_block_local` (`:352`).

  The splice is keyed `(structural_hash, occurrence)`. The module also exports
  `engine_cell_lang` (`:86`), which is **not** legacy. Its users today are
  `nested_cell_mask.rs` (`:259`, plus three in-file tests at `:660`, `:869` and `:1105`),
  `stage/stages/lmd_desugar.rs` (`:29`), `stage/stages/unexecuted_cell_count.rs` (`:13`),
  `lmd/emit.rs` (`:48` and `:236`) and the resolver (`resolution.rs:136`), plus the two
  integration tests `capture_splice_seam.rs:36` and `capture_splice_engines.rs:26`.
  [EXR-2](2026-10-28-exr-02-unit-scanner.md) Phase 1 moves it (and any other non-legacy
  helper) to a module that stays and repoints every one of those callers, so by the time
  this plan starts the module holds only legacy code. Two comments name the old path and
  go stale with the deletion: `crates/pampa/src/writers/qmd.rs:538` and
  `crates/pampa/tests/integration/test_code_block_writer_roundtrip.rs:8`; Phase B repoints
  them to the new home of `engine_cell_lang`.
- **`CaptureSpliceStage`** (`stage/stages/capture_splice.rs`), whose legacy arm (parse
  A1 and B1, `label_reinject::strip` at `:271`) goes; its record arm (EXR-14), files
  materialization and includes folding stay.
- **`label_reinject::strip`** (`crates/quarto-core/src/crossref/label_reinject.rs:61`),
  the compat path for stored captures whose input contains a re-injected label. `inject`
  (`:40`) stays: it is used by the engine stage (`engine_execution.rs:527`) until EXR-10
  moves re-injection to the engine boundary.
- **The legacy reader arm.** `parse_capture_gz` (`pandoc_request/captures.rs:18`) and
  the `Legacy` arm of `parse_capture_payload` (EXR-13), plus the array path in
  `read_capture_from_doc`. By EXR-13\'s rule this arm is the only caller of the legacy
  functions, and it already has the degrade-to-source behavior behind a flag.
- **The per-engine `EngineCapture` schema** (`crates/quarto-trace/src/lib.rs:187`):
  `engine_name`, `input_qmd`, opaque `result`, `files`; emitted by the engine stage
  (`engine_execution.rs`, `ENGINE_CAPTURE_KIND` near `:636` to `:672`). It is **kept**: the
  trace file\'s `engine_capture` block and `ReplayEngine` consume it (next bullet).
- **`ReplayEngine`** (`crates/quarto-core/src/engine/replay.rs`) and the trace file\'s
  `engine_capture` block (`claude-notes/instructions/replay-engine.md`): the bd-45yw
  regression-testing tool, also used by committed tests (`capture_splice_engines.rs`,
  `capture_splice_seam.rs`, `ipynb_stored_replay.rs` and others). Epic decision 9 keeps it:
  trace recording stores the record through an explicit trace-only field (EXR-5), replay
  returns it and the presenter runs again (EXR-6). Removing it is out of scope here.
  `claude-notes/designs/engine-resolution.md` (section 6) said it should likely be replaced
  by a capture-driven replay path; that sentence is amended in Phase B.
- **The preview capture cache** (`crates/quarto-preview/src/cache.rs`), whose payload
  became records in EXR-15 and which stays keyed by `sha256(input_qmd)`; it is not legacy
  any more.
- **The stored-ipynb replay engine** (`engine/jupyter/stored.rs`, Plan 7c), which EXR-1
  and EXR-6 generalize onto the presenter. `IpynbReplayEngine` stays as a thin adapter that
  keeps its name (EXR-6, "keeps its name"), used by `engine/jupyter/mod.rs`, `stored.rs` and
  `engine_execution.rs`, so it is **not** an EXR-17 deletion; its leftovers are deleted in
  EXR-6, not here.
- **`parse_code_blocks` and `runs_in_legacy_executor` are not capture code and stay.**
  `parse_code_blocks` (`engine/jupyter/text_execute.rs:124`, called from `stored.rs:97`) is
  the Jupyter executor's cell finder; EXR-2 Phase 4 replaces it with the shared scan, so
  this plan has nothing to delete there. `runs_in_legacy_executor` (EXR-2) does not exist
  yet; it is the quarantined function that reproduces the executor's cell rule, and epic
  decision 40 (the scanner reports what the legacy executor skips) says it is "deleted with
  the executor rule". The executor stays in this epic, so nothing here deletes it. Its
  retirement trigger is the three divergences it preserves being decided: list-nested cells
  (bd-9j6z311f), attribute-tail cells such as `{python .foo}` (bd-rh4cce9e) and capitalized
  `{Python}` (bd-7gvl455a). Each decision either removes that clause from the function or
  makes it a permanent rule, and the function is deleted when no clause is left. That work
  belongs to those strands, not to this plan.

**ENGINE_CAPTURE_KIND (verified 2026-10-31).** The kind constant is defined in
`stage/stages/engine_execution.rs:52` and emitted at `:672`. Its readers: `stage/trace.rs:240`
(the trace observer, **kept**) and `engine/preview_record.rs:71` (`CaptureCollector`, the
the source for the preview and hub writers, **deleted** with the legacy writers), and it is exercised
by the test at `engine_execution.rs:2953`. So the boundary is two readers of one emission,
not one module edge; the emission itself stays for the trace. The legacy MIME
(`CAPTURE_MIME_TYPE`) is also handled in `crates/quarto-preview/src/capture_driver.rs:44`,
`crates/quarto-core/src/engine/capture_files.rs` and `crates/quarto-hub/src/resource.rs:162`
(`classify.rs` keeps recognizing it, see Phase B). The `parse_capture_gz` callers
(`pandoc_request/render.rs:313`, `project/book/request.rs:82` and the WASM
`parse_capture_from`, `wasm-quarto-hub-client/src/lib.rs:1586`) are listed with their
replacement in [EXR-13](2026-10-28-exr-13-hub-wire.md) Background and Phase B.

## The trigger

All of the following, each checkable. There is **no release count and no date**: the
owner decides when the trigger holds and when to act on it.

1. EXR-6, EXR-8, EXR-9, EXR-11, EXR-12, EXR-14 and EXR-15 have landed, **or** the epic has an
   explicit dated entry declaring that engine "not migrating" and why (for example a
   third-party TS engine that never returns records). A non-migrating engine keeps the
   legacy arm alive for that engine only; deletion then happens per engine. Since the
   records-in-use epic (2026-11-06) a document that uses an engine without records is stored
   whole in the legacy format and a diagnostic names the engine, so that list is the
   inventory of engines still keeping the arm alive; extension engines that never build
   records are the expected members.
2. The record-writing gate (EXR-13) has defaulted to on for every writer: the local preview
   (records-in-use RIU-1) and `provide-hub` (its flip, after the hosted hub reads records).
   (Readers having shipped before writers, per writer and reader pair, is EXR-13\'s rule and
   is already behind this point.) Documents already stored in the legacy format, in hubs,
   kept data directories and old cache entries, stay readable until step 3 allows otherwise.
3. A `hub admin scan` against the project\'s own deployed hub store shows no referenced
   legacy captures that matter (or the owner has decided they may degrade, see below).
4. No committed test, fixture or trace file needs the legacy shape, other than what the
   kept `ReplayEngine` and trace need: the inventory in Phase A is empty or each remaining
   item has an owner and a migration. A committed trace fixture or `ReplayEngine` test that
   still needs the legacy shape is such an item.

## Design decisions

1. **No in-place rewrite of old documents**, here or ever (epic decision 4).
2. **Old captures degrade, they do not break** (epic decision 10). After deletion, a
   legacy payload is read as "no capture": code cells show as source, as for a document
   that was never executed, with one logged notice naming the document. There is no
   converter (a legacy capture holds one markdown blob, not per-unit outputs). The owner
   may leave the legacy arm in place for as long as wanted; removal is not scheduled.
3. **Delete per mechanism, in separate commits**, each with its own passing test run, so a
   problem bisects to one removal.
4. **Never run both mechanisms on one record** stays an invariant after deletion (it
   becomes trivially true).
5. **`ReplayEngine`, `--replay` and the trace\'s `engine_capture` block stay**, along with
   the `EngineCapture` type in `quarto-trace`. Only the preview and hub uses of the legacy
   capture are deleted.

## Out of scope

- Removing `ReplayEngine` or the trace format (decided: they stay).
- Deleting `IpynbReplayEngine`, `parse_code_blocks` or `runs_in_legacy_executor`: the
  adapter stays (EXR-6), the first two are replaced inside EXR-2, and the third retires with
  the divergence strands named in Background.
- Anything about freeze.
- Rewriting committed fixtures beyond what the inventory shows is needed.

## Phase A: inventory and tests first

- [ ] Inventory every committed use of the legacy shape and write it into this plan:
  `EngineCapture` in `crates/**` tests and non-test code, trace fixtures with an
  `engine_capture` block, `ts-packages/**` and `hub-client` tests with
  `application/x-engine-capture+gzip` (for example `client.test.ts`,
  `useActiveCaptureBytes.integration.test.tsx`), and docs that describe the format.
  Legacy-only tests carry the `legacy_capture_` name prefix or module (EXR-13, EXR-14,
  EXR-15), so this is one search; list any that do not and rename them first.
- [ ] Re-check the `ENGINE_CAPTURE_KIND` readers listed in Background (`trace.rs:240` kept,
  `preview_record.rs:71` deleted) against the tree as it stands when this plan starts, and
  list any reader added since. Test: after the deletions in Phase B the trace still records
  and `--replay` still replays a fixture.
- [ ] Verify that `engine_cell_lang` and any other non-legacy helper already left
  `capture_splice.rs` (EXR-2) and that nothing outside the `Legacy` arm imports the module
  (the grep-style test from EXR-14). If either fails, fix it in EXR-2 or EXR-13 terms
  before any deletion; do not move helpers here.
- [ ] Characterization tests for degrade-to-no-capture: a legacy payload through the
  payload reader with the degrade flag on produces an `Empty`-equivalent result plus a
  notice, and a document renders with cells as source.
- [ ] A "no legacy left" lint-style test: a grep-based unit test (as the repo does for
  other invariants) asserting that the deleted symbols are not reintroduced: every item
  named in Phase B, the module path `engine::capture_splice` and `parse_capture_gz`.
- [ ] Confirm the full set of record-path tests (EXR-13, 14, 15) is green on `main`
  before starting, and record the workspace test baseline (run count and skips) to
  compare against afterward.

## Phase B: deletions (one commit each)

- [ ] Turn the degrade flag (EXR-13) on by default, so the `Legacy` arm shows source plus
  the notice, and run the characterization tests; then delete the arm\'s presenting code.
- [ ] Delete the structural splice and the legacy arm of `CaptureSpliceStage`, together
  with their tests: `derive_cell_outputs`, `CellOutputMap`, `apply_capture_splice(s)`,
  `splice_cells`, the private helpers `is_echo_of`, `is_cell_wrapper`,
  `is_engine_output_block` and `structural_eq_block_local`, the whole file
  `engine/capture_splice.rs` and its `pub mod capture_splice;` line (`engine/mod.rs:57`);
  and the integration tests that import them (`capture_splice_seam.rs`,
  `capture_splice_engines.rs`; keep the cases that exercise the record path by moving them
  to the record-path test files first). Then repoint the two stale path comments
  (`pampa/src/writers/qmd.rs:538`, `test_code_block_writer_roundtrip.rs:8`).
- [ ] Delete `label_reinject::strip` and its call at `capture_splice.rs:271`. Keep
  `inject` unless EXR-10 has moved it.
- [ ] Delete `parse_capture_gz` and the legacy arm of the payload reader; the legacy MIME
  stays recognized by `classify.rs` so old orphans remain collectible by `hub admin`.
- [ ] Delete the preview and hub uses of the legacy capture (the writers and readers that
  produce or consume `EngineCapture` arrays for the capture doc). Keep the `EngineCapture`
  type, the engine stage\'s emission to the trace, `ReplayEngine` and `--replay`.
- [ ] Update `claude-notes/instructions/replay-engine.md` (replay now returns records for
  Jupyter; old Jupyter traces need re-recording), the capture-meta plan\'s pointers, and
  `engine-resolution.md` section 6 to describe what remains.

## Phase C: proof of no regression

- [ ] Workspace run (`cargo nextest run --workspace`, which Gordon decides whether to
  run): report the pass and skip counts against the live baseline taken in Phase A and
  account for every difference (deleted tests listed by name).
- [ ] Hub-client, preview-runtime and WASM tests; `npm run build:wasm`.
- [ ] A manual end-to-end on the owner\'s hub: an old legacy capture degrades with the
  notice; a new record splices; `hub admin scan` still collects old orphans.
- [ ] `cargo xtask verify` clean.

## Exit criteria

- None of the deleted symbols remain; `rg` for each returns nothing outside history.
- The workspace baseline delta is exactly the deleted tests.
- Old hub documents degrade to source with a notice instead of erroring.
- `ReplayEngine` and trace-based replay still pass their tests.

## Status after RIU-2 (2026-11-06)

RIU-2 (records replace the capture cache) made a record alone sufficient for preview and the hub
provider, but it deliberately kept the legacy form alive. What remains for this plan, found while
implementing it:

- The legacy arm of `encode_capture_payload` and the refusal predicate `record_has_no_legacy_form`
  (`crates/quarto-core/src/engine/capture_files.rs`) exist only because the writer gate can be off.
  With the gate gone, `encode_for_publish` loses its `PublishError::RecordWriterOff` case.
- `store_run` in `crates/quarto-preview/src/cache.rs` still writes the `.bin` whenever captures
  exist, and `lookup_cached_run` still accepts a `.bin`-only hit. Both are legacy readers and
  writers to delete.
- `CaptureRun::captures` and `CaptureRun::has_content()` keep a legacy field next to the record;
  the guards moved to `has_content()` so deleting the field is a one-line change per guard.
- `write_capture_doc` in `crates/quarto-hub-provider/src/execute.rs` and `write_capture_doc_gated`
  in `crates/quarto-preview/src/capture_driver.rs` both go through one pair, `encode_for_publish`
  and `publish_payload_doc`; this plan only has to change that pair.

## Risks and open questions

- **Degrade versus keep a converter** is decided (degrade; no converter, because the
  lossless conversion does not exist). The only way to serve old captures longer is to
  leave the legacy arm in place, which the owner may do indefinitely.
- **The `ENGINE_CAPTURE_KIND` boundary** (see Not verified) may make the "preview and hub
  uses" deletion less clean than one module; Phase A\'s trace of it decides the scope.
- **Deployed stores** are unknown to us; step 3 of the trigger depends on the hub owners.
- **A third-party engine** that returns only markdown cannot produce per-unit records; it
  needs either a coarse single-unit record (an EXR-7 question) or the legacy arm.

## References

- `crates/quarto-core/src/engine/{capture_splice,replay}.rs`, `crates/quarto-trace/src/lib.rs`
- `claude-notes/instructions/replay-engine.md`, `claude-notes/designs/engine-resolution.md`
- [Multi-engine execution](2026-05-27-multi-engine-execution.md) (per-engine captures and the freeze statement)
- bd-mu9i0bct, bd-42202ipf (the two-mechanisms bug class)
