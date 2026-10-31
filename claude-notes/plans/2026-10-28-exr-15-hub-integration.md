---
title: 'EXR-15: hub integration for engine execution records'
date: 2026-10-28
description: 'Moves the capture driver, re-execute handler, provide-hub, the preview capture cache and hub admin onto records, keeps masking and overlay semantics correct for unit hashes, folds in replay parity for filters and metadata, and follows through on capture size.'
status: draft  # not started
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

- [ ] First task, before the overlay tests: verify what `RenderOverlay`
  (`overlay_runtime.rs`) does to project-level versus document-level metadata. The
  answer decides which keys count as the overlay's execution-relevant provenance and
  whether the overlay-free recompute needs a project-level counterpart. Write the
  result into Background.
- [ ] Writer tests (gate on): each of the three writers stores a record document
  whose `meta.engines` equals the record's engines list and whose payload parses back
  to an equal record. The `Record` each writer receives is the one EXR-5's task "Record
  handoff to the capture writers" delivers next to the legacy captures; these tests build it
  with EXR-4's `RecordBuilder` and do not run an engine.
- [ ] Staleness tests: a prose-only edit leaves a recorded document `staleness: false`;
  editing a code cell, an execution-relevant `#|` line (for example `fig-width`), or an
  execution-relevant front matter key sets it to `true`; a legacy payload still uses the
  byte compare (a prose edit flips it, as today); a freshly recorded labeled knitr
  document is not stale (the symptom in bd-n1vmlalw; ignored with that reference until
  EXR-10 lands).
- [ ] Converse test (decision 34): an `echo` edit to a Jupyter cell's `#|` line leaves the
  record fresh (`staleness: false`) and the cell matched (EXR-14's lookup finds it by the
  execution hash), so the preview shows the recorded output with the new presentation.
- [ ] Per-engine document hash tests (decision 35): toggling `execute: echo` in a **knitr**
  document makes the record stale (the knitr declaration has empty exclusion lists), and
  the same toggle in a **Jupyter** document does not; toggling the `record` key changes
  neither. Both are checked by recomputing from the record's stored declarations with no
  engine registered.
- [ ] Include-edit test, scoped to what it shows: calling the staleness function after an
  edit inside an included file that holds code sets `staleness` to `true`, because the hash
  comes from the pre-engine units of the expanded document, not file bytes. This tests
  the function directly. It does not show that preview recomputes on such an edit: the
  watcher calls `recompute_staleness` only for the changed path
  (`crates/quarto-preview/src/lib.rs:484-510`), so an included-file or `_quarto.yml` edit
  never triggers the includer's recompute. That is a pre-existing limitation, not a
  regression of this plan, and the test name says "function".
- [ ] Overlay test: a record produced under `-M` has the same document-level execution hashes and
  unit hashes as one produced without it for the same source, and a viewer with no
  overlay sees it as fresh. This replaces the `captures[0].input_qmd` rewrite test for
  records (the legacy rewrite and its test stay for legacy payloads).
- [ ] Overlay provenance test: the record produced under `-M` carries the overlay's
  execution-relevant keys as informational provenance (post-merge difference on the
  allowlist), a non-execution key (for example a title) is absent from it, and the bytes
  do not depend on how the flag was spelled; provenance never changes staleness.
- [ ] Masking test: a document with a display block containing a cell and a span
  records no phantom units, and the unit list a viewer computes from the masked
  pre-engine AST equals the recorded one (shares EXR-14's conformance fixtures).
- [ ] Review-file test stays as is: `write_review_file` still shows the author's text.
- [ ] Cache tests: `record_capture_cached` stores and returns a record; a different
  `input_qmd` is a miss; concurrent writers still race safely on the rename.
- [ ] Admin tests (EXR-13 covers classification): `hub admin scan` over a store of mixed
  legacy and record docs reports both, ages by `meta.createdAt`, and collects only
  orphaned ones; `collect` re-checks kind and liveness immediately before deletion, as
  today.
- [ ] Parity tests (bd-cp3em scope): a record replayed in the preview pipeline carries
  the same `filters`, document `metadata`, Pandoc options, resource files and
  `needs_postprocess` as the live `q2 render` result for the same document, for a
  Jupyter fixture and (when EXR-9 lands) a marimo fixture. Each field gets its own named
  test so a regression names the field.
- [ ] Size test: a record with embedded figure bytes above `CAPTURE_DOC_WARN_BYTES`
  logs the existing warning, once per write.

## Phase B: writers and gate

- [ ] Switch `capture_driver`, `re_execute` and `provide-hub` to the shared record
  writer from EXR-13 (`write_capture_payload`), behind its gate, taking the `Record` from
  EXR-5's "Record handoff to the capture writers"; with the gate off they write legacy
  captures, unchanged.
- [ ] Replace `read_capture_from_doc` call sites with the payload reader (EXR-13) and
  make the staleness check payload-aware: legacy payloads keep the byte compare;
  records compare the per-engine document-level execution hashes (EXR-4, decision 35),
  recomputed from the record's stored `hash_definition` entries on the overlay-free,
  masked source, with no engine loaded.
- [ ] `provide-hub`: drop the `captures[0].input_qmd` overlay rewrite for records (the
  hash is overlay-free by construction); keep it for legacy payloads. Record the
  overlay's execution-relevant keys as provenance.
- [ ] `cache.rs`: store the record payload; keep the `sha256(input_qmd)` key (decision 5).
- [ ] Hub admin: confirm no code path assumes a payload type beyond the MIME and
  `meta.kind`; add the record counts to the scan report.

## Phase C: replay parity (bd-cp3em)

- [ ] Re-state the remaining scope of bd-cp3em after bd-5oyk1xce and bd-qbhp2cvv, as a
  comment on that strand, and then close it when this phase lands. (It is in plan
  scope, so it is recorded here, not opened as new work.)
- [ ] Fold notebook-level `filters`, `metadata`, Pandoc options and resource files from
  the record into the preview pipeline at the same seam as `fold_capture_includes_into_meta`;
  fold `needs_postprocess` and warnings likewise.
- [ ] Guests: a guest-style render with no engines and a record in the sidecar shows
  the same output as the host; a guest cannot trigger execution (existing boundary; add
  a regression test for the record case).
- [ ] Chapter downloads (bd-zykj3dmx): confirm the download path reads records through
  the EXR-13 reader; if the gap in that strand is still open, record its status here.

## Phase D: size follow-through (bd-sr73)

- [ ] Build a fixture with a real Jupyter plot (matplotlib PNG) and a small data frame;
  record it as legacy and as a record; measure the gzipped payload, the share of
  figure bytes, and the automerge document size in the hub store.
- [ ] Decide, with numbers, whether inline figures stay acceptable for the 10 MB warning
  and what the sidecar threshold would be; write the decision into the epic's open
  questions and into EXR-4's figure section. This is a measurement and a decision, not
  an implementation of sidecars.
- [ ] Orphan accumulation: measure how many bytes a typical edit-run-edit session leaves
  behind per document, since each execution writes a new document; record the number
  next to the figure decision (it motivates the single-cell upgrade path in the epic).

## Exit criteria

- Phase A tests green; `cargo clippy` and `cargo nextest run` on `quarto-core`,
  `quarto-preview`, `quarto-hub`, `quarto-hub-provider`; WASM build; hub-client tests.
- With the gate off, nothing written changes. With it on, a Jupyter document round trips
  provider to hub store to WASM preview with a correct splice and correct staleness.
- bd-cp3em has a final comment and is closed (or its leftover scope is explicitly
  moved to a named plan); bd-sr73 has the measurement recorded.

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
