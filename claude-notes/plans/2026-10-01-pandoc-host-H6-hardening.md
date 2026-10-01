# Plan: Hardening (pandoc-host H6)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D2, D4, Risks)
**Depends on:** H5. **Unblocks:** nothing (closes the download epic); H9's incremental spike reads its D2(b) result if one exists.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H5's demo STOP is ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H7 (its Start gate decides whether it can begin).

## Checklist

### Tasks
- [ ] **Cross-browser matrix in CI.** WebKit is commented out of `playwright.config.ts:74` and not installed in CI (`hub-client-e2e.yml:169`), and exnref on Playwright's Linux WebKit is unverified (the spike covered macOS WebKit 26.4 only), so start with a probe step. The pandoc specs run under `playwright.harness.config.ts` (a chromium-only project list), so add a webkit project there (`testMatch: pandoc-*`, one worker) rather than enabling the commented block in `playwright.config.ts`; Chromium as today; Firefox stays manual (CI Firefox is braid `bd-phu943t7`), with a documented list of verified browser versions.
- [ ] **Large-document and image-heavy memory test** against the budget H3 set (metric: wasm `memory.buffer.byteLength` in every browser, plus a CDP heap reading on Chromium only; `measureUserAgentSpecificMemory` needs cross-origin isolation); a recycle policy if needed.
- [ ] **Failure-taxonomy audit:** H2 builds the loader/worker classes with their tests; this audit confirms every class in the design has its diagnostic, UI state and test and adds the missing ones.
- [ ] **Time-boxed spike** (output: yes/no plus a latency number) on D2(b) (warm instance + Lua preamble), only if repeat latency is a problem or the PDF incremental spike (H9) needs it.

## Verification

CI matrix green on Chromium and WebKit (if the probe passes); the memory test within budget.

## Exit

The download feature is hardened. The PDF work (H7-H9) proceeds independently.

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
