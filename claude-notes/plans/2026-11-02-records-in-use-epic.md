---
title: 'Epic: execution records in use (preview, hub, and records from other tools)'
date: 2026-11-02
description: 'Follow-up to the execution-records epic. A spike plan (RIU-0) first settles every open question; then the plans prove records through the real q2 preview and hub, make them a total substitute for the preview capture cache, flip the writer default, add record sources that do not execute (starting with the Positron cache), and end with the Positron command demo.'
status: draft  # written 2026-11-02; all plans drafted, provisional items in RIU-1 to RIU-6 are marked and wait on RIU-0
---

**Branch:** `feature/records-in-use`, cut from `feature/execution-records-epic`
(worktree `workspace-4`).
**Extends:** [the execution-records epic](2026-10-28-execution-records-epic.md). That epic
built the record, the presenter and the readers; this one makes them the thing preview and
hub actually use.
**Parallel to:** [the knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md)
(integration branch `feature/knitr-execution-records-epic`; the live work was on branch `knr-2-presenter` in `workspace-3` as of 2026-11-03). See
"Coordination with the knitr epic".
**Origin:** a session of 2026-11-02 that began as a Positron demo request and found that the
records are built and unit tested but have never been enabled or run through the real
`q2 preview`. The demo moved to the end of this epic, and a spike plan moved to the front.
**Terminology:** as in the parent epic. A **record source** is anything that can supply a
record for a document: an engine run, the preview cache, a file under
`.quarto/execution-records/`, a directory of compatible `.ipynb` files, or the Positron
cache.

## Goal

Make the ipynb record the only thing preview and hub need, and let it come from somewhere
other than an engine run:

```text
record source --> record --> capture doc / cache --> splice (preview, hub) --> page
(engine | cache | .quarto file | Positron cache | directory)
```

- Preview and hub work from records with the writer default on, and the legacy capture is no
  longer required for anything (it is deleted later, by EXR-17).
- A record is always available to a consumer that asks for one. `execute: {record: true}`
  controls only whether a copy is also written under `.quarto/execution-records/`.
- A user can point preview or provide-hub at records that were not made by q2, starting
  with Positron's inline-output cache, and no code runs.
- It ends with the Positron command demo that started this: three commands in Gordon's
  Positron fork that exercise both directions.

## What the research found (2026-11-02)

Four read-only surveys, then direct reads. Verified in the code on
`feature/execution-records-epic` unless a line says otherwise; the spikes in
[RIU-0](2026-11-02-riu-00-spikes.md) exist to check what was only read.

**Records and the real binary**
- **Records have not been run through the real binary.** EXR-14's end-to-end test is
  library-level (`render_qmd_to_preview_ast`, a stub engine) and its plan says it was "not
  run through the `q2 preview` binary or a browser". EXR-15 and EXR-18 use fake kernels and
  seeded hub docs. (S1)
- **No released version contains the record reader.** The record reader and the EXR-14 splice
  are on the epic branches only; the latest tag is v0.39.0 (2026-10-29). The runbook's rule is
  readers first, flip later, and the hub exposes no version a writer can read. For the hosted
  hub, "first" is measured by the deployed `q2_ref` (a hand-run deploy), not by a numbered
  release. (S3)
- **The writer gate is off.** `RECORD_WRITING_DEFAULT = false` (`capture_files.rs:259`);
  `QUARTO_ENGINE_RECORD_WRITE=1` turns it on. It is consulted by the preview writer
  (`capture_driver.rs:388`) and by provide-hub (`execute.rs:361`), not by `q2 render`.
  `gate_defaults_off_and_parses_overrides` (`capture_files.rs:385`) asserts the default and
  fails on a flip.
- **A mixed document loses output with the gate on.** `encode_capture_payload`
  (`capture_files.rs:298`) writes the record when it has any engine entry and then drops the
  legacy captures, so a document whose second engine builds no record loses that engine's
  output. EXR-15 noted this; the code is unchanged and no test covers it. Pure knitr documents
  now produce records (commit 663a2f7a8). (S1, B10)

**Records are not yet a substitute for the capture cache**
- `write_capture_doc` has no emptiness check; the guards are in its callers
  (`capture_driver.rs:181`, `re_execute.rs:307`, `quarto-hub-provider/src/execute.rs:338`).
  The cache writes only inside `if !run.captures.is_empty()` (`cache.rs:274`), writes
  `<key>.bin` first, and writes `<key>.record.bin` only when the record has engines. A missing
  `<key>.bin` reads as a miss, so a record-only entry would re-run the engine. Both files are
  gzipped JSON; the extension `bin` is generic. The record file holds the canonical record, which
  is an ipynb. (S2)
- The cache key is `sha256(compute_input_qmd)` (`cache.rs:55`), so any edit misses. A record's
  staleness is by execution hash. The hub index `CaptureRef` is path-keyed, but it holds a
  samod doc id in a store that lives in `data_dir`, which is ephemeral by default. (S1, S2)
- The preview cache is `<data_dir>/captures`; the CLI always passes `cache_dir: None`
  (`preview.rs:368`) and `data_dir` is a `TempDir` deleted on exit unless `--data-dir` is
  given. There is no eviction, because the directory goes away with the session. (S1, S2)
- `record_eager_captures` skips a path that already has a capture ref, so a persisted store
  does not re-record. (S1)

**Execution control**
- `preview.engine` is `manual` (the default: detect a code change, offer "Re-execute"), `auto`,
  or `off`. With `off` the eager driver returns immediately (`capture_driver.rs:65-69`) and
  `recompute_staleness` is a no-op, so no capture ref is created and a cached or imported
  record would not be shown unless a record source sits outside that guard. The re-execute POST
  is not gated. (S1, B8)

**Providers**
- provide-hub and preview share a shape but duplicate it: `write_capture_doc` exists in
  `capture_driver.rs:379-418` and `execute.rs` (~:528). Encoding (`encode_capture_payload`),
  the payload document and `set_capture` are shared. A single "publish a `CaptureRun`"
  function is the natural seam.
- provide-hub's consent gate is a single call (`execute.rs`, after materialize and the review
  file), so an import-only path can branch before it without touching the execution path. (S3)
- `.quarto/execution-records/` is write-only: `write_record_file`
  (`engine_execution.rs:1091`, sole caller `:864`) writes it for `record: true`, and nothing
  reads it. The call site propagates a failure with `?`, so an unwritable directory fails the
  render. (S1, B11)

**Reading side**
- Nothing in the WASM and hub-client read path needs to change for a record-only capture doc:
  `parse_capture_payload` dispatches on the payload's shape and never looks at the MIME, and
  hub admin classification already treats both MIME types as engine captures.

**Positron's cache and the record library**
- Positron's cache files carry `metadata.quarto_source`, the document's file URI; a directory
  scan on it finds a document's file. For a **dev build** the survey reads the cache
  directory as `~/Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs/`
  (not the `Positron` folder where released builds, and the two cache files seen on this
  machine, live); the `User/globalStorage` segment is unverified. (S5)
- Positron never notices an externally written file for an open document: the cache service
  has no events and no file watcher, and `loadCache` returns the in-memory entry. There is no
  push path today. (S5)
- A browser-layer command cannot spawn a process. Extensions use `child_process`, and workbench
  code can register internal commands an extension can call. A macOS GUI app may not inherit
  the login shell PATH, so `q2` may not resolve. (S6)
- **Design for a Positron source: build a `Record` by hand with no engines and empty execution
  hashes**, using `match_units` (which skips an empty execution hash and matches by content
  hash with the occurrence rule) and the splice's content-hash-only fallback for a unit with no
  engine. `record_is_stale` returns not-stale when there are no engines and `rehash_record`
  stamps a hash of an empty list, so neither may be called for it; per-unit staleness comes from
  the `SpliceReport`. `RecordBuilder` cannot produce an empty hash. The language gate has to
  run at conversion, because the owner is unknown inside the splice. (S4)
- `DisplayOnlyOutputs` must never gain a `From` or `Into` to a record cell
  (`imported_outputs_convert_to_nothing_a_reuse_decision_takes`); building a cell by hand from
  `outputs()` is allowed.
- The record module is target-neutral and `import_positron_cache` takes bytes, so the
  conversion should be usable in WASM (unverified, a `cargo check` on the WASM target). (S4)
- **Export for Positron** needs: stringified data values (arrays joined, JSON mimes as strings),
  an `outputs` array on every cell, `quarto_cache_version` 1, reduced metadata, and refusal of
  rendered `text/x-quarto-markdown` units, inline units and front-matter spans. Unverified:
  stream arrays, `image/png` display, Positron's duplicate pairing, and the shape of
  `quarto_source`. (S7)

## Settled decisions

Made with Gordon on 2026-11-02.

1. **Records substitute for the preview capture cache.** A record is built whenever a
   consumer asks (preview and hub already do: `want_record = record_requested ||
   ctx.observer.wants_execution_record()`), and the preview cache holds it. `record: true`
   decides only whether it is also written to `.quarto/execution-records/`. It never decides
   whether a record is available.
2. **Flip the writer default.** `RECORD_WRITING_DEFAULT` becomes on after RIU-0 and RIU-1.
   The flip is the cutover from legacy captures to records in the cache and the hub store. A
   plain `q2 render` keeps writing nothing under `.quarto/` unless `record: true` is set, so
   the litter and failed-write risks of a render default do not arise. The runbook's rule
   (do not flip a writer before the reader it writes to is deployed) applies. For a binary
   that means a release that already ships the reader; for the hosted hub it means a deployed
   `q2_ref` that contains the reader commits. RIU-1 states how it is met, and S3 decides
   whether it applies per surface.
3. **Reuse is existing behavior.** Preview and hub already reuse a recorded result (a cache
   hit skips the engine; a hub viewer splices a provided capture). This epic adds no new
   reuse concept and does not amend parent decisions 3 or 8 for preview and hub. What it adds
   is a second kind of record provider, one that does not execute.
4. **Record sources are an ordered list, per document, first wins, no merging.** Proposed
   option `--records cache,q2,positron,dir:<path>,engine` on `q2 preview` and
   `q2 provide-hub`, default `cache,q2,engine`. `q2` is the optional `.quarto` copy. One
   `CaptureRef` still points at one record. The name and the default list are settled; `q2` is in the default list.
5. **"Never execute" is `preview.engine: off`, not a new flag**, unless RIU-4 shows the key
   cannot do the job. Today `off` skips the driver entirely, so a record source would have to
   sit outside that guard; S1 and S2 settle what this means.
6. **A Positron record is display-only.** It carries a provenance marker, only content hashes,
   and the language gate; it feeds no freeze or reuse decision beyond display. The parent's
   decision 14 and EXR-16 decision 7 stand.
7. **The Positron demo is last and is a spike in the fork.** Spike branches stay in Gordon's
   fork `gordonwoodhull/positron`; nothing is sent to anyone.
8. **The record cache stays where the capture cache is.** `<data_dir>/captures/`, a temp
   directory deleted on exit unless `--data-dir` is given. No new location, gitignore or
   eviction. The record file is `<key>.ipynb`, plain JSON, because that is what it is;
   `.quarto/execution-records/` and the `q2` source remain the deliberate way to keep records
   across sessions.
9. **Spikes first.** RIU-0 settles every unknown in its register before RIU-1 to RIU-6 are
   finished, so the plans can be reviewed with the facts in place. Until RIU-0 reports, those
   six are drafts and anything provisional in them is marked.

## Decisions for review

These are Gordon's to make, not spikes:

- **Which q2 ref the hosted hub runs, and whether anyone runs `provide-hub` against it.** The
  rest of the release plan follows from facts now known (next section). Needed before the
  `provide-hub` default is flipped, not before RIU-0 to RIU-6.

### Hub deployment facts (read 2026-11-03 from `~/src/quarto-hub-deployment`)

- The hub binary (`quarto-hub`) and the hub-client are built together by one manual GitHub
  Action, "Build and deploy artifacts", from a `q2_ref` input (default `main`; a branch, tag or
  SHA), uploaded to S3, then deployed by hand with `quarto-hub-deploy` on the instance
  (`docs/operations.md`). So "the hub is on a reader" means the deployed `q2_ref` contains the
  reader commits. The numbered q2 release is irrelevant to the hub.
- Deploys are rarer than binary releases. The braid snapshot records deploys on 2026-09-12,
  09-14 (twice), 09-17 and 09-25 (`main-20260925-205359`); nothing later is recorded there,
  though the log may lag. The authoritative answer is `cat /etc/quarto-hub/version.json` on the
  instance, which I did not run.
- There is a rollback (`quarto-hub-rollback`, maintenance mode required) that restores an earlier
  `versions/<ref>-<stamp>` from S3. A rollback to a build without the reader is the exact case
  that strands record documents already written.
- The record reader is on the epic branches, not on `main`, so the hosted hub cannot have it
  unless someone deployed one of those branches.

## Plans

| ID | Plan | Sessions (guesses) | Notes |
|---|---|---|---|
| RIU-0 | [Spikes that settle the unknowns](2026-11-02-riu-00-spikes.md) | 6 to 7 | detailed; S0 to S7; includes the old "prove records in the real preview" run as S1 |
| RIU-1 | [Enablement and the writer flip](2026-11-02-riu-01-enablement-and-writer-flip.md) | 1 | drafted; provisional items wait on RIU-0 |
| RIU-2 | [A provider seam; records replace the capture cache](2026-11-02-riu-02-provider-seam-records-replace-cache.md) | 3 to 4 | drafted; the central piece |
| RIU-3 | [The Positron cache as a record source](2026-11-02-riu-03-positron-cache-as-record-source.md) | 2 | drafted |
| RIU-4 | [CLI surface for record sources](2026-11-02-riu-04-cli-surface-for-record-sources.md) | 1 | drafted; spelling is for review |
| RIU-5 | [Export for Positron](2026-11-02-riu-05-export-for-positron.md) | 1 | drafted |
| RIU-6 | [The Positron command demo](2026-11-02-riu-06-positron-command-demo.md) | 2 to 3 | drafted; spike in the fork |

Total roughly 16 to 21 sessions, with RIU-0 and RIU-2 the largest.

## Ordering

```text
RIU-0 (S0 ... S7)  -->  finish and review RIU-1 .. RIU-6
then:  RIU-1 --> (flip)
          \-----> RIU-2 --> RIU-3 --> RIU-4 --> RIU-5 --> RIU-6
```

RIU-1 and RIU-2 can overlap after RIU-0. RIU-2 must land before the flip ships in a release,
because the flip makes the cache hold records.

## Coordination with the knitr epic

- **KNR-5** owns widening the importer gate to knitr-owned `r` units and its
  `fallback: bool` argument. RIU-3 consumes it and adds no second gate.
- **Fallback units.** Until KNR-8 removes them, a knitr record may hold
  `text/x-quarto-markdown` units. RIU-5 must refuse them for Positron, and RIU-3's provenance
  has to survive them.
- **Records for R.** knitr fallback records are format-specific and a second render can
  overwrite the first. That affects `record: true` files, not the cache.
- **KNR-6** moves knitr consumers onto structured records. RIU-2's record-only cache entry is a
  prerequisite it can use; if the two collide on `capture_driver.rs`, the integration branches
  rebase, with no coupling in the plans.
- R through knitr is not in the first demo.

## Exit criteria

- RIU-0's findings note exists and every register row has an answer.
- With the writer default on, `q2 preview` and hub work from records alone, verified through
  the real binary and a browser (state plainly if any step could not be browser-tested).
- A Positron cache supplies a document's outputs to preview with no execution.
- The RIU-6 demo exists, or the notes say what blocked it.
- Plan checklists are reconciled with what landed.

## Risks

- **A latent defect in a path that has only run in tests.** S1 is the mitigation; budget for
  fixes, not just findings.
- **The flip is a one-way door for shared hub stores** (parent decision 4). RIU-1 states what a
  rollback leaves behind before the flip, not after.
- **Records in the cache do not outlive a session by default**, as legacy captures do not.
  A user who wants that passes `--data-dir`, or sets `record: true` and reads the copy back
  with the `q2` source.
- **Positron's cache format can change** under us. The importer already rejects a
  `quarto_cache_version` other than 1, and the conformance fixtures from EXR-16 stay the check.
- **Positron-side spikes may need Gordon at the window**, and the dev build may be slow.
- **Browser verification may not be possible in an agent session.** The plans say which steps
  need a human or a browser tool.

## References

- [RIU-0](2026-11-02-riu-00-spikes.md)
- [Execution-records epic](2026-10-28-execution-records-epic.md), plans EXR-13 to EXR-16
- [Knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md), KNR-5, KNR-6
- `claude-notes/research/2026-11-02-exr-16-s2b-s6-seed-and-demo-findings.md`,
  `claude-notes/research/2026-11-02-exr-16-s3-inline-exec-findings.md`
- `claude-notes/instructions/release-runbook.md` ("Engine-record writer gate")
