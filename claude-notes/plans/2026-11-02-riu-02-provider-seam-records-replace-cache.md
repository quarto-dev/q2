---
title: 'RIU-2: a provider seam; records replace the capture cache'
date: 2026-11-02
description: 'Factor one publish-a-capture-run function out of the preview and provide-hub copies, make a record-only entry valid everywhere the preview path assumes a legacy capture, settle where the cache lives and how an edit finds its record, and read the .quarto copy as a record source.'
status: draft  # written 2026-11-02; design points marked provisional wait on RIU-0 S1 and S2
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** [RIU-0](2026-11-02-riu-00-spikes.md) S1 and S2. RIU-1 can overlap.
**Blocks:** RIU-3 (sources plug into the seam), and the release of RIU-1\'s flip.
**Estimated sessions:** 3 to 4. The central piece.

> **For implementers:** tests first. Spike S2 prototypes a record-only entry and the pointer;
> its branch is scratch, but its list of code that assumes a legacy capture is the work list
> here.

## Overview

Preview and hub work from a `CaptureRun` that carries legacy captures and, when the gate is on
and the engine built one, a record. Several places treat "no legacy captures" as "nothing to
show", so a record alone is not enough. This plan makes the record sufficient, puts the two
copies of the publish code behind one function whose input can come from any record source,
and settles how an edit finds its record within a session.

Settled (epic decisions 1, 3, 4): records substitute for the capture cache whether or not
`record: true` is set; reuse is existing behavior and this plan adds no reuse concept; sources
are an ordered list, first wins, no merging, one `CaptureRef` points at one record.

## Phase A: tests first

- [ ] A preview test where the engine produces a record and no legacy captures: the cache
      writes, a second request hits, the engine does not run again. Today the cache write is
      guarded by `if !run.captures.is_empty()` (`cache.rs:274`) and a missing `<key>.bin`
      reads as a miss.
- [ ] Update the tests that assert empty-captures behavior, because "no captures" is no longer
      the same as "no output": `cached_wrapper_returns_empty_for_prose_only_doc`,
      `prose_only_doc_records_no_capture`, `md_doc_with_engine_spec_records_no_capture`,
      `recompute_staleness_noop_when_no_capture`, `prose_only_doc_leaves_sidecar_empty`.
      Keep the intent of each (a prose-only document records nothing) and change the
      predicate to "no captures and no record units".
- [ ] A staleness test: the Legacy arm of staleness must handle a ref whose payload is a
      record only.
- [ ] A test that a prose edit reuses the record (execution hash unchanged) in standalone
      preview. **Provisional (S1, S2, U4, U11):** this needs the pointer or the `CaptureRef`
      below, because the cache key is `sha256(compute_input_qmd)` (`cache.rs:55`) and any edit
      misses.
- [ ] A test for the `q2` source: a file at `.quarto/execution-records/<doc>.ipynb` supplies
      the record when the cache has none.

## Phase B: the seam

- [ ] One function that publishes a `CaptureRun` (encode via `encode_capture_payload`, write
      the payload document, `set_capture`), replacing the two copies of `write_capture_doc`
      (`quarto-preview/src/capture_driver.rs:379-418` and `quarto-hub-provider/src/execute.rs`
      near `:528`). Decide the home crate by what both already depend on; if neither is the
      right home, the shared function goes beside `encode_capture_payload` in `quarto-core`.
- [ ] The input to that function is a source-neutral value (a `CaptureRun`, or a record plus
      provenance), so RIU-3 adds a source without touching the publish code.
- [ ] Preserve the three caller guards\' intent (`capture_driver.rs:181`, `re_execute.rs:307`,
      `execute.rs:338`) with the new predicate from Phase A.

## Phase C: record-only entries

- [ ] The cache write guard (`cache.rs:274`): write when there are captures or a record with
      units. Write the record file `<key>.ipynb` without requiring `<key>.bin`.
- [ ] Hit or miss decided from either file.
- [ ] The Legacy staleness arm and `recompute_staleness` handle a record-only ref.
- [ ] Nothing in the WASM or hub-client read path changes (`parse_capture_payload` dispatches
      on payload shape); confirm with the S1 hub scenario and say so in the plan result.

## Phase D: the record file, and how an edit finds its record

- [ ] **Settled (epic decision 8):** the location does not change. Records are cached in
      `<data_dir>/captures/`, a temp directory deleted on exit unless `--data-dir` is given
      (`preview.rs:248-254`; the CLI passes `cache_dir: None`, `:368`). No gitignore, no
      eviction, no standalone-file case.
- [ ] Rename the record file from `<key>.record.bin` to `<key>.ipynb`
      (`record_file_path`, `cache.rs:109`) and write it as plain canonical record JSON, not
      gzipped. It is an ipynb, so Jupyter can open it and the `dir:` source can read a cache
      directory unchanged. The gzip in `write_cached_record` exists because the hub stores
      gzipped bytes; the hub publish path keeps gzipping, the cache does not need to. The
      legacy `<key>.bin` is untouched (EXR-17 deletes it). Add a test that a legacy entry
      written before the rename still hits, and that the reader rejects a gzipped `.ipynb`
      with a clear miss, not a panic. Update the `cache.rs` module comment.
- [ ] **Provisional (S2, U11):** the pointer from a document path to its latest record, if
      S2 shows it is needed, within one session. The prototype is
      `<cache>/by-path/<hash(rel_path)>`. If the hub `CaptureRef` (path-keyed, holds a samod
      document id in the store under the same `data_dir`) already gives preview what it needs,
      prefer it and add nothing.

## Phase E: the `q2` source and `preview.engine: off`

- [ ] Read `.quarto/execution-records/<doc>.ipynb` as a record source, through the source
      trait or function the seam defines.
- [ ] **Provisional (S1 B8, U5):** with `preview.engine: off` the eager driver returns
      immediately (`capture_driver.rs:65-69`) and `recompute_staleness` is a no-op, so no
      capture ref is created and a cached record would not show. If S1 confirms this, a
      record-source lookup runs outside that guard: with `off` the sources are consulted but
      the engine is never run. The re-execute POST is not gated today; leave it, but note it.
- [ ] Stamp each record with its source (`engine`, `cache`, `q2`, later `positron`, `dir`) and
      carry it to the viewer in the same place existing capture metadata travels. Decide the
      field by reading how the viewer receives capture metadata; do not widen the WASM
      surface beyond one string.

## Phase F: verify

- [ ] `cargo clippy -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto-core -p quarto-preview -p quarto-hub-provider`
- [ ] Because this touches `quarto-core`, which the WASM client depends on, recommend
      `cargo xtask verify` to Gordon; he decides.
- [ ] Real binary, with the preview rebuild chain from `AGENTS.md`: `q2 preview` on a Python
      document, edit prose, confirm no re-execution (log), restart with a persisted store and
      confirm reuse. Browser verification as RIU-0 S0 allows; state plainly what was not seen.

## Exit criteria

- With the default on, preview and hub run with no legacy capture written or read.
- A prose edit does not re-execute, in standalone preview, within a session. Reuse across
  restarts is not a goal; `--data-dir` or `record: true` with the `q2` source provides it.
- Every code location S2 listed has been changed or has a stated reason it need not be.
- This is also the step toward EXR-17\'s trigger (deleting the legacy path); note in EXR-17\'s
  plan what remains.

## Coordination

KNR-6 moves knitr consumers onto structured records and uses the record-only entry from this
plan. If both touch `capture_driver.rs`, the integration branches rebase; there is no
coupling in the plans (epic, "Coordination with the knitr epic").

## Risks

- **The seam grows.** Keep it to publish; sources and CLI belong to RIU-3 and RIU-4.
- **A rename of the record cache file** strands entries in a persisted `--data-dir`. They
  miss and re-run once; nothing breaks. Say so in the plan result.
