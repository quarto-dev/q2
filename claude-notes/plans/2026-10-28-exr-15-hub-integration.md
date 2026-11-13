---
title: 'EXR-15: hub integration for engine execution records'
date: 2026-10-28
description: 'Moves the capture driver, re-execute handler, provide-hub, the preview capture cache and hub admin onto records, keeps masking and overlay semantics correct for unit hashes, folds in replay parity for filters and metadata, and follows through on capture size.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** [EXR-13b](2026-10-28-exr-13-hub-wire.md) (writer helper and gate; the reader half, 13a, is implied),
[EXR-14](2026-10-28-exr-14-preview-splice.md) (read side), [EXR-5](2026-10-28-exr-05-jupyter-records-shadow.md)
(Phase 2: the record handoff to the capture writers), [EXR-4](2026-10-28-exr-04-record-format.md).
Soft: [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md) (knitr records appear on the hub
once it lands).
**Blocks:** [EXR-17](2026-10-28-exr-17-legacy-removal.md)
**Lane:** D (`workspace-7`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3 (writers and staleness: 1.5; masking and overlay: 0.5; parity and size: 1)

> **For implementers:** TDD, task by task. Record writing stays behind the EXR-13
> gate until release N+1.

## Goal

Make every place that records, stores, ages or serves a capture treat a record as the
payload, with the behaviors that were correct for legacy captures kept correct:

- the preview capture driver, the `/api/preview/re-execute` handler and
  `q2 provide-hub` write records (gated);
- staleness of a record is decided from its document-level execution hashes (one per
  engine, decision 35; each covers that engine's document-level inputs and its ordered
  unit hashes, so a code edit flips it), so unit hashes and staleness agree on what the
  document is (legacy payloads keep the byte compare);
- `provide-hub -M` (metadata overlay) and nested-cell masking keep unit hashes
  identical to what a viewer recomputes, and the overlay's execution-relevant keys are
  recorded as informational provenance;
- `hub admin` inventories, ages and collects records;
- guests, who have no engines, show the host's record;
- replay parity for everything a capture carried but the splice dropped (bd-cp3em);
- capture size is measured on a real engine and the limit decision is recorded (bd-sr73).

## Background (verified in code on 2026-10-28, workspace-1 at `2ee0bd966`)

**Writers.** Three `write_capture_doc` functions (see EXR-13): `capture_driver.rs:331`,
`re_execute.rs:336`, `quarto-hub-provider/src/execute.rs:493`.

**Provider and the overlay.**

- `Provider` in `crates/quarto-hub-provider/src/execute.rs` runs a fresh
  `record_capture` every time (`:330`, decision 4: code may have side effects).
- When `-M` supplies a metadata overlay (`self.overlay.is_some()`, `:340`), it
  recomputes `compute_input_qmd` on the overlay-free project and overwrites
  `captures[0].input_qmd` with those bytes (`:348` to `:352`, "F8-D9 / 8.3c"): the
  staleness key must describe the source, not the provider's execution settings, or
  every capture would look stale to a viewer with no overlay.
- `write_review_file` (`:441` to `:462`) unmasks what `compute_input_qmd` masked; a
  test (`:556`) pins that the review file shows the author's text, not the mask.

**Staleness today.** `capture_driver.rs` compares, byte for byte, the freshly computed
`compute_input_qmd` (masked, whole document) against the recorded capture's
`input_qmd` (`:248` to `:283`). Any prose edit flips `staleness`. `compute_input_qmd`
(`preview_record.rs:209` to `:250`) runs the pre-engine pipeline (includes expanded,
project and document metadata merged) and then masks, so the record's hash must be
computed from the units of that same output, never from raw file bytes (verified
2026-10-29). bd-n1vmlalw records a
second symptom: a freshly recorded labeled knitr document looks stale immediately
because the label re-injection leaks into the recorded input.

**What `RenderOverlay` does (verified 2026-11-01, `overlay_runtime.rs`, `metadata_layers.rs`).**
`-M` fills `RenderOverlay::metadata`, which `OverlayRuntime::runtime_metadata()` serves; the
metadata merge adds it as the **last** layer (`LayerKind::Runtime`, `metadata_layers.rs:161`),
over project, directory and document metadata, so an overlay key can override a *document-level*
key (`params`, `execute`, `engine`), which is why the hashes need recomputing. The overlay
changes no file (the provider passes `-M` only, so `files` is empty) and no project-level
file read: the overlay-free recompute needs only the plain runtime, with no project-level
counterpart (`compute_source_inputs` on `NativeRuntime`, as `execute.rs` does). The
provenance keys are the post-merge difference restricted to `engine`, `execute-dir`, `params`,
`execute` and each engine's `document_keys` (`record::overlay_difference`, EXR-5).

**Already in place when this plan started** (EXR-5, EXR-13b): `overlay_free_run` and
`quarto_overlay` in the provider, the gated shared writers, the record counts in the admin
scan, and `CapturePayload` dispatch in `read_capture_from_doc`. This plan added
`record::record_is_stale`, the record arm of `recompute_staleness`, `SourceInputs::sugared_blocks`
and the record file in the preview cache.

**The preview cache.** `crates/quarto-preview/src/cache.rs`: per-session files
`<cache_dir>/<sha256(input_qmd)>.bin`, the same gzipped `EngineCapture` payload as the
hub doc (`compute_key` `:48`, `record_capture_cached` `:150`). Cross-session reuse is
a recorded follow-up (bd-povv). It is a fourth capture mechanism, and a
whole-document one, keyed on the same whole-input bytes.

**Admin.** `hub admin scan`, `classify`, `collect` as described in EXR-13. The
collection allowlist admits `DocKind::EngineCapture` only
(`scan.rs:374`, `collect.rs:341`).

**What the splice reads.** `CaptureSpliceStage` reads `result.markdown` and
`result.includes` only (verified by grep). `ExecuteResult` (`engine/context.rs:291`)
also carries `supporting_files`, `filters`, `needs_postprocess` and more. Embedded files
are materialized from `EngineCapture.files`, so the file part of bd-cp3em is already
fixed; the includes part was fixed by bd-5oyk1xce. Filters, document metadata, Pandoc
options, resource files, `needs_postprocess` and warnings are still dropped. bd-cp3em is
therefore partly stale and its remaining scope should be re-stated when this plan
starts.

**Not re-verified in this session** (reported by the handoff's hub-consumers research):
the guest behaviors (strands bd-jddbirj3, bd-0lc6tqyq, bd-a5gp5y38, bd-7zygxe1e), the
chapter-download gap (bd-zykj3dmx), and the exact set of admin subcommand names beyond
scan, classify and collect.

## Design decisions (from the epic, do not re-litigate)

1. The hub payload is the record, gzipped, in the EXR-13 envelope; one `CaptureRef`
   per path; immutable blobs; orphans collected by `hub admin`.
2. Only the host executes. Guests and the browser read the host's record; the
   host-only re-run boundary stays.
3. Unit hashes are computed on masked source and are independent of any overlay.
4. `-M` semantics are kept: what is stored describes the source, not the provider's
   execution settings. The document-level execution hashes are overlay-free, computed on the
   overlay-free source project as F8-D9 does for `input_qmd` today
   (`execute.rs:340` to `:352`), so a viewer with no overlay sees the record as fresh.
   The record also stores the overlay's execution-relevant keys as informational
   provenance, never used for invalidation: the post-merge difference restricted to the
   keys that count under the hash declarations of the engines (epic decision 35; not the raw `-M`
   strings), so records stay deterministic whatever the flag spelling. EXR-4 defines the
   field. The known cost: a record produced under a materially different overlay (for
   example `params`) looks fresh.
5. **Staleness uses the hash for records; the cache key stays bytes.** Record staleness
   compares the per-engine document-level execution hashes (epic decision 35:
   `quarto_execution_hash` is a hash over the ordered per-engine values, each computed with
   that engine's stored `hash_definition`, and each includes that engine's ordered unit
   hashes, so editing code or an execution-relevant option moves it for Jupyter and knitr
   as well as for document-granularity engines). The comparison recomputes them from the
   **stored** declarations, with no engine loaded, so the same check works in WASM and on
   the hub; the units come from the shared scan (EXR-2) of the masked, overlay-free
   pre-engine document, each unit hashed under the engine whose recorded owned-language
   set (stored in that engine's `quarto_engines` entry by EXR-4) contains its language.
   The unit hash holds no kernel, so no engine is needed for that either. Which `execute:` keys count is therefore per engine: knitr declares empty
   exclusion lists (its `echo`, `warning` and other presentation keys are baked into its
   rendered markdown, so they are in its hash), Jupyter excludes its closed presentation
   list. Legacy payloads keep the byte compare. The
   preview capture cache keeps its `sha256(input_qmd)` key until freeze: switching it
   to the hash would make a cache hit skip execution, which is reuse and belongs with
   freeze. Staleness is advisory and, in preview, drives a re-execution that a user
   edit triggers; nothing in this epic skips execution on a hit. The intended,
   user-visible consequence: a prose-only edit no longer flips staleness or triggers an
   auto re-execute for records (it also fixes the second symptom of bd-n1vmlalw;
   EXR-14's lenient id-based splice already makes prose edits harmless for display).
6. New code in this plan does not import from `engine/capture_splice.rs` and does not
   call the legacy-only functions outside the `Legacy` arm (EXR-13 decision 8); the
   seam test there covers this plan's files.
7. Figures stay inline in the record for now; sidecar documents are deferred until the
   bd-sr73 measurement says otherwise.
8. **Which wins, hub payload or on-disk record, is moot in this epic.** The payload type
   a reader gets is already defined: `CapturePayload { Empty, Legacy, Record }`, dispatched
   on the `nbformat` key (EXR-13 decision 3). Nothing in this epic reads the on-disk record
   file to present or to judge staleness: the preview and the hub read only the capture
   payload, and the on-disk record has only EXR-4's own reader (round-trip tests and trace
   replay) and the docs. A "which wins when both exist" rule only has a job once a consumer
   reads both, which is the freeze work, so it needs no decision here.

## Out of scope

- Reusing a record to skip execution (freeze, not in this epic), including switching the
  preview cache key from `sha256(input_qmd)` to the execution hash.
- Structured automerge storage, figure sidecar documents.
- UI for staleness.
- Cross-session cache reuse (bd-povv) beyond swapping the payload.

## Phase A: tests first

- [x] First task, before the overlay tests: verify what `RenderOverlay`
  (`overlay_runtime.rs`) does to project-level versus document-level metadata. The
  answer decides which keys count as the overlay's execution-relevant provenance and
  whether the overlay-free recompute needs a project-level counterpart. Write the
  result into Background.
  Done: see Background (the overlay is the last metadata layer; no project-level counterpart needed).
- [x] Writer tests (gate on): each of the three writers stores a record document
  whose `meta.engines` equals the record's engines list and whose payload parses back
  to an equal record. The `Record` each writer receives is the one EXR-5's task "Record
  handoff to the capture writers" delivers next to the legacy captures; these tests build it
  with EXR-4's `RecordBuilder` and do not run an engine.
  Done for the two writer implementations, `record_doc_meta_and_payload_match_the_record` in `quarto-preview/src/capture_driver.rs` and `quarto-hub-provider/src/execute.rs`; the re-execute handler shares the preview writer since EXR-13b, so its copy is gone.
- [x] Staleness tests: a prose-only edit leaves a recorded document `staleness: false`;
  editing a code cell, an execution-relevant `#|` line (for example `fig-width`), or an
  execution-relevant front matter key sets it to `true`; a legacy payload still uses the
  byte compare (a prose edit flips it, as today); a freshly recorded labeled knitr
  document is not stale (the symptom in bd-n1vmlalw; ignored with that reference until
  EXR-10 lands).
  Done: `record::staleness` unit tests, `capture_driver` `record_*` tests, and `record_preview_splice::a_freshly_recorded_labeled_knitr_document_is_not_stale` (EXR-10 has landed, so it is not ignored). The legacy byte compare keeps `recompute_staleness_also_flips_for_prose_only_edits_v1_limitation`.
- [x] Converse test (decision 34): an `echo` edit to a Jupyter cell's `#|` line leaves the
  record fresh (`staleness: false`) and the cell matched (EXR-14's lookup finds it by the
  execution hash), so the preview shows the recorded output with the new presentation.
  Done at the function level, `jupyter_echo_option_edit_is_fresh`; the matched-cell half is EXR-14\'s (`match_units` keys on the execution hash).
- [x] Per-engine document hash tests (decision 35): toggling `execute: echo` in a **knitr**
  document makes the record stale (the knitr declaration has empty exclusion lists), and
  the same toggle in a **Jupyter** document does not; toggling the `record` key changes
  neither. Both are checked by recomputing from the record's stored declarations with no
  engine registered.
  Done: `execute_echo_toggle_is_stale_for_knitr_and_fresh_for_jupyter` and `toggling_the_record_key_changes_neither_engine`; `record_is_stale` reads only the record\'s stored declarations.
- [x] Include-edit test, scoped to what it shows: calling the staleness function after an
  edit inside an included file that holds code sets `staleness` to `true`, because the hash
  comes from the pre-engine units of the expanded document, not file bytes. This tests
  the function directly. It does not show that preview recomputes on such an edit: the
  watcher calls `recompute_staleness` only for the changed path
  (`crates/quarto-preview/src/lib.rs:484-510`), so an included-file or `_quarto.yml` edit
  never triggers the includer's recompute. That is a pre-existing limitation, not a
  regression of this plan, and the test name says "function".
  Done: `record_function_sees_a_code_edit_inside_an_include` (named for the function, not the watcher).
- [x] Overlay test: a record produced under `-M` has the same document-level execution hashes and
  unit hashes as one produced without it for the same source, and a viewer with no
  overlay sees it as fresh. This replaces the `captures[0].input_qmd` rewrite test for
  records (the legacy rewrite and its test stay for legacy payloads).
  Done: `provide_hub_with_overlay_stores_overlay_free_record_hashes` now also asserts `record_is_stale` is false for an overlay-free viewer.
- [x] Overlay provenance test: the record produced under `-M` carries the overlay's
  execution-relevant keys as informational provenance (post-merge difference on the
  allowlist), a non-execution key (for example a title) is absent from it, and the bytes
  do not depend on how the flag was spelled; provenance never changes staleness. Also (moved
  from EXR-4): `quarto_overlay` is absent from a record produced with no overlay, and changing
  its contents changes no hash (unit, document or `quarto_execution_hash`).
  Done: `overlay_provenance_is_independent_of_flag_spelling` (flag order, YAML flow versus JSON, a title absent); `quarto_overlay` absent with no overlay is `coalesce.rs` and unchanged hashes under changed provenance is `freeze_readiness::mutating_provenance_changes_no_matching_staleness_or_reuse_key`.
- [x] Masking test: a document with a display block containing a cell and a span
  records no phantom units, and the unit list a viewer computes from the masked
  pre-engine AST equals the recorded one (shares EXR-14's conformance fixtures).
  Done: `a_recorded_document_is_fresh_with_and_without_the_mask`, over the same `CONFORMANCE` corpus as EXR-14.
- [x] Review-file test stays as is: `write_review_file` still shows the author's text.
  Unchanged and green (`write_review_file_shows_the_authors_r_not_the_mask` in the provider).
- [x] Cache tests: `record_capture_cached` stores and returns a record; a different
  `input_qmd` is a miss; concurrent writers still race safely on the rename.
  Done: the record is cached in `<key>.record.bin` beside `<key>.bin`; tests `cached_wrapper_stores_and_returns_the_record`, `a_different_input_misses_for_the_record_too`, `concurrent_record_writers_leave_a_readable_entry`. Temp files are now unique per writer, so two writers can no longer interleave bytes in one temp file.
- [x] Admin tests (EXR-13 covers classification): `hub admin scan` over a store of mixed
  legacy and record docs reports both, ages by `meta.createdAt`, and collects only
  orphaned ones; `collect` re-checks kind and liveness immediately before deletion, as
  today.
  Covered by EXR-13\'s `admin_record_documents` integration tests and `scan.rs::records_and_captures_scan_alike_and_count_separately`.
- [x] Parity tests (bd-cp3em scope): a record replayed in the preview pipeline carries
  the same `filters`, document `metadata`, Pandoc options, resource files and
  `needs_postprocess` as the live `q2 render` result for the same document, for a
  Jupyter fixture and (when EXR-9 lands) a marimo fixture. Each field gets its own named
  test so a regression names the field.
  Scoped out by Gordon's decision (2026-11-01): live render consumes none of these fields, so there is no live result to compare against. The warnings test exists (`record_splice` tests); `html_dependencies` parity moves to bd-j0tsv4xw.
- [x] Size test: a record with embedded figure bytes above `CAPTURE_DOC_WARN_BYTES`
  logs the existing warning, once per write.
  Covered structurally: `gzip_helpers_share_one_gzip_and_warning_body` pins one gzip body with one warning call, and `size_warning_fires_only_above_threshold` pins the threshold; each write gzips once.

## Phase B: writers and gate

- [x] Switch `capture_driver`, `re_execute` and `provide-hub` to the shared record
  writer from EXR-13 (`write_capture_payload`), behind its gate, taking the `Record` from
  EXR-5's "Record handoff to the capture writers"; with the gate off they write legacy
  captures, unchanged.
  Done by EXR-13b (`encode_capture_payload`, `create_engine_payload_document`, the one gate). Found here: an engine that builds no record leaves an *engineless* coalesced record, which with the gate on would replace the legacy captures with an empty document; `encode_capture_payload` and the preview cache now treat it as absent (`gate_on_with_an_engineless_record_falls_back_to_legacy`). A mixed document (one engine with records, one without) still loses the second engine\'s output with the gate on; the records-in-use epic reproduced this on 2026-11-05 (spike S5, an extension engine that builds no record) and decides what the writer does about it, in place of "the gate stays off until every shipped engine builds records" (its decisions of 2026-11-06). The engineless rule above is extended, not reversed: a record with cells and no engines, which only an import produces, counts as describing something (RIU-2).
- [x] Replace `read_capture_from_doc` call sites with the payload reader (EXR-13) and
  make the staleness check payload-aware: legacy payloads keep the byte compare;
  records compare the per-engine document-level execution hashes (EXR-4, decision 35),
  recomputed from the record's stored `hash_definition` entries on the overlay-free,
  masked source, with no engine loaded.
  Done: `read_capture_from_doc` returns `CapturePayload` (EXR-13b); the staleness arm now compares document hashes for records (`recompute_staleness`).
- [x] `provide-hub`: drop the `captures[0].input_qmd` overlay rewrite for records (the
  hash is overlay-free by construction); keep it for legacy payloads. Record the
  overlay's execution-relevant keys as provenance.
  Satisfied: `overlay_free_run` (EXR-5) rewrites the first capture\'s `input_qmd` for the legacy fallback and recomputes the record\'s hashes and provenance; a record doc stores no `input_qmd`.
- [x] `cache.rs`: store the record payload; keep the `sha256(input_qmd)` key (decision 5).
  Done; the key is still `sha256(input_qmd)` (decision 5).
- [x] Hub admin: confirm no code path assumes a payload type beyond the MIME and
  `meta.kind`; add the record counts to the scan report.
  Done by EXR-13 (`classify.rs`, `capture_inventory` in the scan report, `admin_record_documents`).

## Phase C: replay parity (bd-cp3em)

**Finding (2026-11-01, needs Gordon\'s decision before the unticked items below).** The plan\'s premise
that the live `q2 render` consumes a result\'s `filters`, `metadata`, Pandoc options, resource files and
`needs_postprocess` is false in the current code: `EngineExecutionStage` applies only `markdown`,
`includes`, `supporting_files`, `html_dependencies` and `warnings` (`engine_execution.rs:762-800`); the
other five are copied onto the record (`fold_side_data`) and the trace and read by nothing. So there is
no live behavior for the preview to match, and folding them would add behavior `q2 render` lacks.
Of what the live stage does apply, the record splice now covers `markdown` (units), `includes`,
supporting files and, as of this phase, `warnings`. `html_dependencies` is the real remaining gap, but
`HtmlDependency` holds host file paths and the record stores no bytes, so a replay on another machine
(guest, WASM) cannot store them; closing it needs the record to embed the files, an EXR-4 format
change. Both questions are for the epic owner.

**Decision (Gordon, 2026-11-01).** Skip the five unconsumed fields: the preview matches live render,
which ignores them. The `html_dependencies` gap is split out of EXR-15 as bd-j0tsv4xw (EXR-4 format
change); EXR-15 lands without it.

- [x] Re-state the remaining scope of bd-cp3em after bd-5oyk1xce and bd-qbhp2cvv, as a
  comment on that strand, and then close it when this phase lands. (It is in plan
  scope, so it is recorded here, not opened as new work.)
  Restated as a comment on bd-cp3em (see the Finding above). The strand stays open: its html_dependencies half waits on the decision.
- [x] Fold notebook-level `filters`, `metadata`, Pandoc options and resource files from
  the record into the preview pipeline at the same seam as `fold_capture_includes_into_meta`;
  fold `needs_postprocess` and warnings likewise.
  Done as scoped by Gordon's decision: `warnings` are raised as diagnostics (`fold_warnings`, `recorded_engine_warnings_are_raised_as_diagnostics`). `filters`, `metadata`, Pandoc options, resource files and `needs_postprocess` are deliberately not folded (consumed by nothing live). `html_dependencies` (needs the record to carry bytes) is split out as bd-j0tsv4xw.
- [x] Guests: a guest-style render with no engines and a record in the sidecar shows
  the same output as the host; a guest cannot trigger execution (existing boundary; add
  a regression test for the record case).
- [x] Chapter downloads (bd-zykj3dmx): confirm the download path reads records through
  the EXR-13 reader; if the gap in that strand is still open, record its status here.
  Guest: `record_preview_splice::a_guest_shows_the_hosts_recorded_output_and_never_executes` renders with `ExecutionPolicy::None` and a counting engine in the registry: output equals the host\'s, and the engine runs zero times for fresh and stale cells alike.
  Chapter downloads: the TS side landed in `f285bb650` (chapter-alone request passes `captureGzJson`; `downloadController.test.ts`), and `parse_capture_from` in `wasm-quarto-hub-client` goes through `parse_capture_payload`, the EXR-13 reader, so a record reaches the splice as `CapturePayload::Record`. Verified below the browser: `pandoc_request_captures::a_chapter_request_with_a_record_shows_the_recorded_output` and `..._leaves_an_edited_cell_as_source` feed a gzipped record through `parse_capture_payload` into a chapter-alone `render_pandoc_request`; `downloadController.test.ts` (47) and `useDownloadAs.integration.test.tsx` (8) cover the TS side. Browser: `hub-client/e2e/chapter-capture-download.spec.ts` (Chromium) seeds a record doc and its `captures` sidecar entry on the hub, then downloads an ordinary page (docx) and a book chapter ("This chapter only", epub); both contain the recorded output, and a no-capture control shows the source. With the controller's `captureGzJson` pass-through removed the two recorded-output tests fail and the control passes, so the spec guards the fix.

## Phase D: size follow-through (bd-sr73)

- [x] Build a fixture with a real Jupyter plot (matplotlib PNG) and a small data frame;
  record it as legacy and as a record; measure the gzipped payload, the share of
  figure bytes, and the automerge document size in the hub store.
- [x] Decide, with numbers, whether inline figures stay acceptable for the 10 MB warning
  and what the sidecar threshold would be; write the decision into the epic's open
  questions and into EXR-4's figure section. This is a measurement and a decision, not
  an implementation of sidecars.
- [x] Orphan accumulation: measure how many bytes a typical edit-run-edit session leaves
  behind per document, since each execution writes a new document; record the number
  next to the figure decision (it motivates the single-cell upgrade path in the epic).

**Result (2026-11-01):** measured with a throwaway `uv` environment (ipykernel, matplotlib, pandas)
through `record_capture_run`, `encode_capture_payload`, `create_engine_payload_document` and a
filesystem-backed samod repo (a temporary ignored test, not committed). Legacy capture 24,417
bytes gzipped, record 23,099, stored document 24,842 and 23,522 bytes; 10 runs of an edited
document left 248,406 bytes (legacy) and 235,206 (record) in the store, exactly linear. Decision:
inline figures stay acceptable (10 MB is about 430 figures), the sidecar threshold would be
1 MB gzipped per record, and the orphan cost is about 23.5 KB per execution per document. The
decision is in the epic's open question 5 and in EXR-4's figure section.

## Exit criteria

- Phase A tests green; `cargo clippy` and `cargo nextest run` on `quarto-core`,
  `quarto-preview`, `quarto-hub`, `quarto-hub-provider`; WASM build; hub-client tests.
- With the gate off, nothing written changes. With it on, a Jupyter document round trips
  provider to hub store to WASM preview with a correct splice and correct staleness.
- bd-cp3em has a final comment and is closed (or its leftover scope is explicitly
  moved to a named plan); bd-sr73 has the measurement recorded.

**Verification at completion (2026-11-01):** `nextest` on `quarto-hub`, `quarto-hub-provider`,
`quarto-preview`, `quarto-publish`, `quarto-lsp-core`, `quarto-test`, `perf-harness` and `quarto`:
1942 passed, 7 skipped; `nextest -p quarto-core` 7099 passed, 33 skipped (before the last two
record tests, which passed alone); `npm run test:wasm` after a fresh `build:wasm`: 440 passed,
28 skipped; the Chromium e2e passes. The guest test was mutation-checked: with
`ExecutionPolicy::All` it fails ("a stale cell is not re-run on a guest"). bd-cp3em is closed;
its `html_dependencies` leftover is bd-j0tsv4xw. Not run: `cargo xtask verify`, Firefox e2e.

## Risks and open questions

- **Hash completeness.** Staleness by hash is only as sound as the hash. A document-level
  input missing from an engine's declaration shows a stale record as fresh and suppresses a
  re-execute. The default (nothing declared) over-invalidates instead. Each engine's
  declaration is reviewed with Gordon for this reason.
- **Includes are not watched.** An edit to an included file or `_quarto.yml` does not
  recompute the includer's staleness in the preview watcher
  (`crates/quarto-preview/src/lib.rs:484-510`). Pre-existing; this plan does not widen
  or fix it, and claims nothing about preview behavior for such edits.
- **Overlay.** An overlay can legitimately change what runs (for example `params`). The
  overlay-free hash means a record produced under a materially different overlay looks
  fresh; the provenance field makes the difference discoverable by a later UI or tool,
  but nothing acts on it here.
- **Prose edits stop re-executing.** Intended (decision 5) but user-visible; mention it
  in the preview docs (EXR-18).
- **Guest and download behaviors** are reported by earlier research and not re-verified
  (the guest strands bd-jddbirj3, bd-0lc6tqyq, bd-a5gp5y38 and bd-7zygxe1e, and the
  chapter-download gap bd-zykj3dmx; see Background). Phase C's guest and chapter-download
  tasks start by checking them.

## References

- `crates/quarto-preview/src/{capture_driver,re_execute,cache}.rs`
- `crates/quarto-hub-provider/src/execute.rs`, `crates/quarto-hub/src/admin/`
- [Capture-doc metadata and hub admin tools](2026-07-24-capture-meta-and-hub-admin-tools.md)
- [Preview engine-capture delivery](2026-07-02-preview-capture-delivery.md)
- Strands: bd-cp3em, bd-sr73, bd-povv, bd-n1vmlalw, bd-zykj3dmx, bd-lucp (closed)
