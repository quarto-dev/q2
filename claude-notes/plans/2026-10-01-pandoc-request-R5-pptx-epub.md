# Plan: pptx and epub requests (pandoc-request R5)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D3, D1)
**Depends on:** R2; file-serial with R4 (both change `pandoc_write.rs`). **Unblocks:** the pptx and epub menu entries in host H5.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R4 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R6 (its Start gate decides whether it can begin).

## Overview

Extend the docx request to pptx and epub. The only real work is `epub_extra_args`, which extracts `FORMATS_DIR` to disk and passes temp paths to pandoc; in-memory it must become `files` entries.

## Checklist

### Tests first
- [ ] pptx and epub fixtures' requests equal the recorded native runs (R0 wrapper). Epub is expected to need a document `identifier` for reproducibility (R0 exploration).

### Tasks
- [ ] `epub_extra_args` (`pandoc_write.rs:152`, `std::fs` + `FORMATS_DIR`) made in-memory: the `--include-in-header` temp paths become `files` entries written under the temp root, beside the share tree; `epub-embed-font` and repeated `css` feed `resource_refs`; `epub-cover-image` and `epub-metadata` likewise (D3).
- [ ] pptx: confirm `reference-doc` handling and the lack of a post-step; slide-level and other forwarded args come from `forwarded_args`.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test.

## Exit

All four formats have requests; host H5 lists them from the format table.

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
