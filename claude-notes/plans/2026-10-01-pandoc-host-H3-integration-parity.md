# Plan: Integration and parity net (pandoc-host H3)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D12, Failure taxonomy)
**Depends on:** H2; request R2 (`render_pandoc_request`, share-tree export). **Unblocks:** H5.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H2 and R2 are fully ticked (H4 comes first in the lane so R2 can land meanwhile; if R2 is still open after H4, wait). If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H5 (its Start gate decides whether it can begin).

## Overview

Connect the host to the real Rust request and prove output equals native: a dev-only harness, the docx parity net in Node and in the browser, a CI job guarding it, and browser measurements that settle the limits H1 and H2 proposed.

## Checklist

### Tasks
- [ ] **Dev-only harness** (hidden route or console function) running the real `render_pandoc_request` → worker → blob for docx, so the epic is demoable here. The share tree now comes from the Rust export; the disk-fixture mounting from H1 remains for tests.
- [ ] **Parity net.** Render the P7 docx golden fixtures through the Rust request → wasm pandoc, in Node and in the browser. The comparator is Rust-only (`quarto-output-extract`, called from `xtask capture_pandoc_goldens.rs:226`), so the Node run writes its outputs to disk (the browser run saves via Playwright's `download` event) and R0's extractor CLI compares them; the fixture list (`quarto_output_extract::FIXTURES`) is exported to a JSON file that the TS side reads. Open the outputs in Word once by hand (the extractor never checks that a file opens cleanly).
- [ ] **CI net:** a new workflow or job (the existing ones have no `paths:` filters) that runs the fixture set through wasm whenever `resources/pandoc-filters`, `pandoc_write.rs`, the pampa JSON writer, or `resources/pandoc-wasm.json` change. It needs the download and a wasm build (as slow as the existing job). Decide fork behavior (no secrets are needed).
- [ ] **Measure** first-download latency, per-render latency in the browser, Lua startup, and peak memory with image-heavy docs, against the Node baseline of ~0.3-0.5 s per fresh-instance render (~250-300 ms of it Lua init, ~50 MB linear memory). Include the cost of mounting ~250 share-tree files into a fresh instance (D2a). Check the decided limits and the 120 s timeout against the browser numbers (a miss goes to a human, not a silent change), set the decompression-ratio guard, and state the memory budget. Also run the typst, pptx and epub recordings through the harness once R4/R5 exist.

## Verification

The parity job is green on the docx fixtures; the harness downloads a docx that opens in Word.

## Exit

Docx works end to end in a developer build. H4 and H5 turn it into a product feature.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Manual: the harness's downloaded docx opens in Word.
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
