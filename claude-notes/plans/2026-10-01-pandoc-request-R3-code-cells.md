# Plan: Code cells and cached results (pandoc-request R3)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D11)
**Depends on:** R2. **Unblocks:** the "N code cells not executed" status in host H5; R6 (the prefetch runs after the splice).
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R2 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R4 (its Start gate decides whether it can begin).

## Overview

The hub has no engines. A downloaded document renders code cells unexecuted unless the cached-execution (capture) system already holds results for them, including results peers contributed over the CRDT. This phase threads the capture inputs into `render_pandoc_request`, splices cached results, and counts the cells that stay unexecuted.

## Decisions

- Same capture inputs as the preview render; the splice is `insert_capture_splice_stage` (`pipeline.rs:~694-700`), already reachable through R2's `build_pandoc_prefix_stages(fmt, captures)`.
- The status is a count, not a policy bool.

## Checklist

### Tests first
- [ ] Fixture with and without a cached capture gives the right counts and the right output (cells with results show them; cells without render as source).
- [ ] A fixture with a captured figure in docx: `CaptureSpliceStage` materializes captured figure files into the runtime next to the document (`materialize_capture_files`, `engine/capture_files.rs:156`), so `resource_refs` finds and mounts them; the request carries them.

### Tasks
- [ ] **Capture input.** R2 already parses `capture_gz_json` (as the preview render does) and passes the captures through `pandoc_request::render::render_pandoc_request` into `build_pandoc_request_stages(captures)`, so the splice stage is inserted when captures are present; what R3 adds is the coverage that proves it (the two tests above; nothing exercises a non-empty capture on the pandoc path yet) and any fix they expose. The signature does not change in this phase. `stats.unexecuted_cells` is a constant 0 in R2's wrapper (`pandoc_request_envelope` in `wasm-quarto-hub-client/src/lib.rs`) until the count task below fills it.
- [ ] **Unexecuted-cell count.** `execution_skipped` (`pipeline.rs:190`, `stage/context.rs:239`) is a policy bool, not a count, so count the cells without a capture using the existing predicate `engine_cell_lang` (`engine/capture_splice.rs:86`, a brace-class `CodeBlock`), evaluated right after the splice and before `ast-transforms` can rewrite classes; carry the count through a new output-only `RenderContext` field bridged like `pandoc_request` (R1), into the response envelope's `stats.unexecuted_cells`.
- [ ] `materialize_capture_files` writes into the live VFS, so it is the one pre-snapshot write; R6 moves it onto the snapshot. Open: captures are recorded for the document's own format (usually html), so html-only raw blocks and svg figures can be spliced into a docx request; decide what to do with them when this phase runs.

## Verification

Crate-scoped clippy/nextest for `quarto-core` and the wasm crate; the wasm compile check; the hub-client wasm test.

## Exit

The response envelope's `stats.unexecuted_cells` carries the count; host H5 displays it.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
