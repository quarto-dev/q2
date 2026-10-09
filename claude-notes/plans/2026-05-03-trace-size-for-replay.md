---
title: 'Manage trace size for use as replay/regression-test fixtures (bd-5qnj)'
date: 2026-05-03
description: 'Shrinks trace files, which are dominated by repeated document AST snapshots, by dropping pretty-printing and compressing and deduplicating snapshots so traces stay small enough to check in as regression fixtures.'
---

**Date:** 2026-05-03
**Beads:** bd-5qnj
**Worktree:** `.worktrees/5qnj-trace-size` (branch `beads/5qnj-trace-size`, based on `main` \@ `2b954d75`)
**Status:** Design aligned with user 2026-05-03. Ready to implement on user go-ahead. See "Resolved design decisions" below.

## Triage verdict

**Ready to design.** The bulk distribution is clear, the relationship
to bd-45yw is concrete, and there's a tractable design space (drop
pretty-print → gzip → dedup AST snapshots → minimal replay artifact)
that maps onto well-defined intervention points. The remaining
choices are policy-level questions for the user, not gaps in the
investigation.

## Resolved design decisions (2026-05-03)

1. **Size budgets.** No hard targets. Provisional ceilings (≤ 100 KB
   compressed for CI fixtures, ≤ 1 MB compressed for user-attached
   reports) accepted as "directionally right" — the goal is to see
   where the obvious wins land before debating numbers. Re-evaluate
   after Phase 1+2 with measurements on real fixtures.
2. **Unified artifact.** One trace serves both diagnostic and replay
   roles. Phases 1+2 land first; bd-45yw's replay capture rides on
   top of the smaller format.
3. **Drop pretty-print on disk.** Yes. SPA formats client-side;
   `quarto trace show --pretty` covers ad-hoc `jq` users.
4. **Sequencing vs. bd-45yw.** Independent work streams. The
   investigation here is wire-format only; the in-memory representation
   on the reader side is allowed to be heavyweight (debugging context
   tolerates memory cost). The constraint we're optimizing for is
   **repo-at-rest growth** as regression-test fixtures accumulate, not
   runtime memory. A non-trivial merge with bd-45yw's branch is
   expected and acceptable.
5. **One v2 bump.** All improvements (dedup + replay capture + any
   diff-UI prerequisites) land together under `schema_version: 2`.
6. **Keep no-op `transform:` entries.** Don't elide. Dedup makes them
   nearly free, and the diff UI surface stays unchanged.

The empirical numbers (full report at
`claude-notes/plans/5qnj-trace-size-investigation/measurements.md`)
short summary:

- A 6.1 KB qmd produces a **16.3 MB** pretty-printed trace; 3.16 MB
  minified; 627 KB minified+gz.
- **94% of the data bytes are `DocumentAst` payloads.**
- **42 of those snapshots collapse to 6 distinct contents** —
  no-op transforms re-serialize the same AST 36 times.
- Pretty-print alone is **81% of file bytes**.

So the file is large for cheap-to-fix reasons. The interesting
question is which of the unification roles (diagnostic vs. replay)
each intervention is for.

## Issue context

> "Spun out of bd-45yw (replay engine). Once traces double as replay
> fixtures (and as user-attached bug-report artifacts), trace size
> becomes a real constraint — they will be checked into the repo as
> regression fixtures and posted by users in issues."

- Filed 2026-05-03 by cscheid, P2, type `task`, status `open`.
- Issue is one day old — no risk of stale assumptions.
- Investigation list called out: where the bulk lives, replay vs.
  diagnostics overlap, compression on disk, lazy reads, AST
  diffing/elision, size budgets.

The originating bd-45yw plan (on `beads/45yw-replay-engine`) makes the
unified-artifact question explicit:

> "Preferred shape: one trace serves both diagnostic and replay roles.
> The blocker is trace size … Tracked separately as **bd-5qnj** … the
> unified-artifact decision in Phase 1 depends on bd-5qnj's size
> investigation. If size can't be bounded, fall back to a dedicated
> single-purpose replay artifact."

## Dependency graph

```
bd-5qnj (this) ─ related ─> bd-45yw (replay engine, in_progress)
                              └── discovered-from ─> bd-o8pr (project resources, closed)
```

- **`related: bd-45yw`** is the only edge. bd-45yw is in_progress on
  its own worktree (`beads/45yw-replay-engine`), with phases drafted
  but not started; its Phase 1 explicitly depends on this issue's
  outcome.
- **No incoming `blocks` edges.** The `dep tree` shows bd-45yw's
  downstream graph (bd-o8pr → bd-t3ny / bd-k9i1 / bd-w5os / etc.),
  but those are bd-45yw's lineage, not consumers of bd-5qnj. No
  separate work is currently waiting on this issue.

The graph tells us this is a sizing/format prereq for bd-45yw's
Phase 1 decision. It is not blocking anything else open in the queue.

## What the code looks like today

All references in the issue are still live:

- `crates/quarto-trace/src/lib.rs` — `TraceDocument`, `TraceEntry`
  unchanged from the schema described in the doc comment (lines 11–30).
  `SCHEMA_VERSION = 1`. Adding fields is additive; entry-shape
  changes (delta-encoded entries) bump the version.
- `crates/quarto-trace/src/write.rs:43` — single call to
  `serde_json::to_writer_pretty`. This is the pretty-print source.
- `crates/quarto-core/src/stage/trace.rs::JsonTraceObserver` —
  serializes via `serialize_pipeline_data` (line 366). Each
  `PipelineData` variant produces a JSON object; the `DocumentAst` /
  `AtProfile` variants embed a full AST via
  `pampa::writers::json::write` (line 437). No deduplication, no
  delta encoding, no compression. Native-only (cfg-gated against
  `wasm32`).
- `crates/quarto-core/src/stage/stages/metadata_merge.rs:294` —
  `activate_trace_from_metadata` selects observer based on `trace`
  metadata key.

The "Trace size" decision doc (D8 of
`claude-notes/plans/2026-04-14-trace-viewer-design.md`, lines 174–185)
already anticipates the progression: schema_version is in place;
gzip is the cheap first move; JSON-Patch deltas under `schema_version: 2`
are pre-planned for the diff UI in Phase 4.6. **bd-5qnj is the
forcing function for actually making those moves**, because the
unification-with-replay use case turns "noticeable on disk" into
"part of the regression suite" and "attached to user issues."

## Empirical findings (from the investigation report)

Full numbers in
`claude-notes/plans/5qnj-trace-size-investigation/measurements.md`.
Short summary:

| intervention                                | size (big fixture) | reduction |
|---------------------------------------------|-------------------:|----------:|
| current: pretty-printed JSON on disk        |          16.27 MB  |        1× |
| drop pretty-print (minified JSON)           |           3.17 MB  |      5.1× |
| pretty + gzip                               |           0.93 MB  |       17× |
| minified + gzip                             |           0.63 MB  |       26× |
| minified + dedup ASTs (6 distinct of 42) + gz | (projected) <0.1 MB |     >150× |
| minimal replay artifact (no diagnostic AST) | (projected) <30 KB |     >500× |

Reproduces same shape on the medium fixture (4.5 KB → 15.6 MB pretty);
tiny doc shows the absolute floor (124 B → 620 KB pretty / 13 KB gz)
which sets a lower bound: any document, however small, currently
emits a fixed 47-entry pipeline whose per-entry overhead dominates.

## Proposed phases (draft)

Phase numbering is independent of bd-45yw — the two issues need to
sequence relative to each other, but neither is logically a phase of
the other.

### Phase 0 — Tests / measurement harness (TDD)

- [x] Round-trip through gzipped on-disk path
      (`test_roundtrip_through_gzipped_disk` in
      `crates/quarto-trace/tests/roundtrip.rs`).
- [x] On-disk format is compact (no pretty-print indentation)
      (`test_writer_emits_compact_json_on_disk`).
- [x] Legacy uncompressed `latest.json` still parses
      (`test_read_legacy_uncompressed_json`).
- [x] `list_traces` discovers both `latest.json.gz` and `latest.json`,
      preferring the gzipped one when both exist
      (`test_list_traces_finds_gzipped_and_uncompressed`,
      `test_list_traces_prefers_gz_when_both_present`).
- [x] Trace-viewer server returns gzipped traces transparently
      (`api_trace_returns_gzipped_trace` in
      `crates/quarto-trace-server/src/lib.rs`).
- [ ] Size-budget regression test against a small in-tree fixture —
      deferred to a follow-up under Phase 5 docs (need a stable, tiny
      qmd fixture in the test tree first; tracked as part of the
      `claude-notes/instructions/testing.md` note).
- [ ] Dedup correctness round-trip (Phase 2 only).

### Phase 1 — Cheap wins (schema-neutral)

- [x] Drop `to_writer_pretty` from the on-disk path; switch to
      `to_writer`. (Decision: no `--pretty` CLI flag added — `quarto
      trace show` already formats stdout via `to_string_pretty` from
      the parsed `TraceDocument`, and humans wanting raw `jq` access
      can pipe through `gunzip -c | jq`. The wire format being
      compact is invisible to all existing consumers.)
- [x] Gzip on disk: writer emits `latest.json.gz`; reader detects
      by extension; `list_traces` and the trace-viewer server both
      handle the new format and fall back to legacy uncompressed
      `latest.json` for backwards compatibility.
- Sequencing: Phase 1 is independent of bd-45yw and lands in
  parallel. Wire-format-only changes; in-memory representation
  unchanged. A non-trivial merge with the bd-45yw branch is
  expected (per "Resolved design decisions" #4).
- End-to-end results recorded in
  `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`
  ("Phase 1 verification" section).

### Phase 2 — DocumentAst dedup under `schema_version: 2`

- [x] Wire format: top-level `asts: { "<hash>": <AST> }` map plus
      `{ "$ref": "<hash>" }` sentinels inside entries\' `data`. Hash:
      SHA-256 truncated to 16 hex chars (64 bits).
- [x] Writer (`crates/quarto-trace/src/write.rs`): clones the doc
      before serializing, walks pipeline entries, dedups
      `data["ast"]` for wrapped DocumentAst/AtProfile entries and
      the whole `data` for bare `transform:*` AST entries. The
      caller's `TraceDocument` is never mutated.
- [x] Reader (`crates/quarto-trace/src/read.rs`): rehydrates
      `$ref` sentinels using the parsed `asts` map and clears the
      map before returning, so consumers see a v1-shaped
      `TraceDocument` regardless of on-disk format.
- [x] `SCHEMA_VERSION` bumped to 2. v1 traces (pre-bd-5qnj or
      hand-written) still parse via the rehydration no-op path.
- [x] No SPA changes needed: `quarto-trace-server` reads via
      `read_trace`, so the SPA receives the rehydrated (v1-shaped)
      JSON and is unaware of the wire-format dedup.
- End-to-end results recorded in
  `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`
  ("Phase 2 verification" section). Big fixture: 16.3 MB → 62 KB
  (≈265× total reduction); all three fixtures under both
  provisional budgets.
- bd-45yw's replay capture (Phase 3 of this plan) will ride on
  the v2 schema additively — no further schema bump required.

### Phase 3 — Replay capture in the unified artifact (joint with bd-45yw)

Decision: **unified.** One `latest.json.gz` carries both the diagnostic
pipeline trace and the replay payload (`engine_name`, recorded input,
`ExecuteResult`). Replay readers ignore everything outside the
engine-capture object; diagnostic readers ignore the capture.

- [x] Merged `main` (with bd-45yw's replay engine) into
      `beads/5qnj-trace-size`. Auto-merged cleanly: bd-45yw added an
      additive `engine_capture: Option<EngineCapture>` field to
      `TraceDocument`, orthogonal to my `asts` field. Schema bump to 2
      (mine) is the one merged version; bd-45yw didn't bump.
- [x] Unified-artifact end-to-end test:
      `test_unified_artifact_carries_dedup_and_engine_capture` in
      `crates/quarto-core/src/stage/stages/engine_execution.rs`.
      Drives three identical DocumentAst snapshots through the
      observer (Phase 2 dedup pass collapses them) plus a recording
      engine (`MockIncludesEngine`, bd-45yw machinery), writes to
      `latest.json.gz`, and asserts the on-disk JSON has both a
      populated `asts` map (1 entry from 3 snapshots) and a populated
      `engine_capture`. Reader-rehydration also verified — no `$ref`
      visible to consumers, `engine_capture.result` round-trips back
      to a typed `ExecuteResult`.
- [x] CLI end-to-end on the bd-5qnj fixtures (markdown engine):
      `q2 render big.qmd` produces a 62 KB `latest.json.gz` with
      `schema_version: 2`, 4 deduped ASTs in `asts`, and
      `engine_capture: null` (correct: markdown engine is a no-op
      passthrough per bd-45yw's design and doesn't emit a capture).
- [x] No additional schema bump needed — bd-45yw's
      `engine_capture` was additive, so v2 covers both.

The merge was nearly mechanical because bd-45yw and bd-5qnj
operated on orthogonal fields. The original plan anticipated a
bigger Phase 3 (extending the trace format with replay capture) —
but that extension was already done as part of bd-45yw, leaving
this phase as a merge + verification exercise.

**Outstanding follow-up** (worth its own beads issue, not this
plan): re-measure trace size on a fixture that exercises a real
engine (knitr/jupyter) producing significant `supporting_files`
content. The provisional budgets were measured on markdown-engine
fixtures where `engine_capture` is `None`; jupyter outputs as
base64 PNGs in `supporting_files` could push the total beyond the
100 KB CI-fixture budget.

### Phase 4 — Lazy load on read (deferred)

If the trace-viewer SPA's perceived load latency on large traces ever
becomes user-visible, switch the reader to streaming mode (skip
unused stage payloads at parse time, fetch on demand). Probably not
needed once Phase 1+2 land.

### Phase 5 — Docs

- [x] Internal note appended to `claude-notes/instructions/testing.md`
      under "Pipeline traces (`quarto-trace`)". Covers on-disk format,
      `quarto trace list/show/view` invocations, how to write tests
      against a fixture trace via `quarto_trace::read::read_trace`,
      and the provisional size budgets.
- [x] User-facing bug-report page at `docs/bug-reports.qmd`,
      modelled on quarto-web's top-level `bug-reports.qmd`. Covers
      what makes a useful bug report, how to enable `trace: true`,
      what's in (and not in) a trace, and how to inspect one with
      `quarto trace list/show/view`. Wired into the navbar; the
      docs-site IA redesign will likely move it, but the content is
      the load-bearing part.

## Open design questions for the user

All resolved 2026-05-03 — see "Resolved design decisions" near the top
of this document. No open questions remain blocking implementation.

## Risks / tradeoffs

- **Schema bump amortization.** v2 will require trace-viewer +
  `quarto trace show` reader updates. The cost scales with how many
  changes ride on the bump; question 5 controls that.
- **Dedup adds reader complexity.** v2 readers need to resolve
  `ast_ref` hashes against the top-level `asts` map. Manageable, but
  it means `jq` users need a stage like `jq '.asts[.pipeline[i].ast_ref]'`
  rather than `jq '.pipeline[i].data.ast'`. Acceptable IMO; worth
  flagging.
- **Gzipped traces aren't directly `jq`-able.** `zcat trace.json.gz |
  jq` is one extra step. We can soften with a `quarto trace cat` that
  decompresses to stdout. Not blocking.
- **Coordination with bd-45yw branch.** bd-45yw is in flight on its
  own worktree and may evolve the engine-capture shape independently;
  Phase 3 is where the merge happens. Wire-format-only scope here
  reduces but does not eliminate the merge surface — expect to
  reconcile `TraceEntry` / sibling-type choices when the branches
  rejoin.
- **The 16 MB → 0.6 MB result depends on the document.** Documents
  with heavy supporting_files content (jupyter outputs, plot
  PNGs as base64) will not shrink as much, because the `data`
  payload share grows. We should re-measure on a representative
  jupyter trace before committing budgets.
