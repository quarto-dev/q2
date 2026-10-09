---
title: 'Plan: Hardening (pandoc-host H6)'
date: 2026-10-01
---

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D2, D4, Risks)
**Depends on:** H5. **Unblocks:** nothing (closes the download epic); H9's incremental spike reads its D2(b) result if one exists; the D2(b) result led to H10a and H10b.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H5's demo STOP is ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H7 (its Start gate decides whether it can begin).

## Checklist

### Tasks
- [x] **Cross-browser matrix in CI.** WebKit is commented out of `playwright.config.ts:74` and not installed in CI (`hub-client-e2e.yml:169`), and exnref on Playwright's Linux WebKit is unverified (the spike covered macOS WebKit 26.4 only), so start with a probe step. The pandoc specs run under `playwright.harness.config.ts` (a chromium-only project list), so add a webkit project there (`testMatch: pandoc-*`, one worker) rather than enabling the commented block in `playwright.config.ts`; Chromium as today; Firefox stays manual (CI Firefox is braid `bd-phu943t7`), with a documented list of verified browser versions. (Done: `webkit` project in `playwright.harness.config.ts`, `pandoc-browser-probe.harness.spec.ts`, probe-gated WebKit steps in `hub-client-e2e.yml`, the chromium step pinned with `--project=chromium`, `test:harness:webkit`, the browser table in `dev-docs/pandoc-wasm-host.md`. All 38 pandoc specs pass in macOS WebKit 26.4. **Linux WebKit is unverified**: Docker was not running here and nothing is pushed, so the first CI run is the probe; the steps skip with a warning if it fails, and the guards come out once it passes.)
- [x] **Large-document and image-heavy memory test** against the budget H3 set (evidence §12: ~0.6-1.4 GB transient for the first load, then ~50 MB per render plus 11-17x the image payload; H3 left the 300 MB `total_bytes` limit as a human decision) (metric: wasm `memory.buffer.byteLength` in every browser, plus a CDP heap reading on Chromium only; `measureUserAgentSpecificMemory` needs cross-origin isolation); a recycle policy if needed. (Done: `e2e/pandoc-memory.harness.spec.ts`, Chromium and WebKit, in CI; the sweep and the decision numbers are evidence section 13. **No recycle policy was needed or possible:** the worker is already one per render on the fresh path, and the Rust wasm that keeps ~4.4-4.7x the payload cannot be recycled. `limits.total_bytes` is still the 300 MB it was: that choice is Gordon's, with the numbers in section 13 and the Handoff log.)
- [x] **Failure-taxonomy audit:** H2 builds the loader/worker classes with their tests; this audit confirms every class in the design has its diagnostic, UI state and test and adds the missing ones. (Done: two gaps closed, `WebAssembly` absent (`no-wasm`, was a download failure) and a main-thread memory failure (`out-of-memory`, was `crashed`), each with tests; a test that every failure state has copy. Evidence section 13.)
- [x] **Time-boxed spike** (output: yes/no plus a latency number) on D2(b) (warm instance + Lua preamble), only if repeat latency is a problem or the PDF incremental spike (H9) needs it. (Not run, by the condition: repeat latency is not a problem, and H9 can use an explicit latency budget instead. Measured with the module resident and a fresh instance per render: **165 ms in Chromium (instantiate 22 + run 138), 299 ms in WebKit**, callouts.qmd, evidence section 12; the first run after a load ~285 ms. Written into H9's `incr_compile` task as that budget. D2(b) was measured afterwards in the H9 spike and built for the PDF preview as H10a and H10b.)

## Verification

CI matrix green on Chromium and WebKit (if the probe passes); the memory test within budget.

## Exit

The download feature is hardened. The PDF work (H7-H10b) proceeds independently.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS). (Verification: the pandoc harness specs pass on Chromium and macOS WebKit 26.4 locally (38 of 38; the 2 opt-in measure tests skip), and so do the new probe and memory specs (memory within budget). **The Linux WebKit CI job has not run** (nothing is pushed); its probe-gated steps decide on the first CI run. Gates: hub-client `test:ci` unit 1362 -> 1366 (+4: no-wasm loader x2, runner no-wasm row, controller OOM test), integration 154 -> 155 (+1: copy-per-failure-state), wasm 274 -> 274; `ts-packages/pandoc-host` 33 pass; eslint clean on the changed files. No Rust touched, so no workspace nextest.)
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

- **Open for Gordon, until answered** (deliberately not a checkbox, so it does not hold the lane on H6; not a gate for H7): (1) `limits.total_bytes` (300 MB): lower it or accept desktop-only for large-image documents; numbers in evidence section 13 (browser process grows ~15x the payload: 100 MB is +1.5 GB, 150 MB +2.0 GB, 200 MB +2.9 GB, 300 MB +4.7 GB; the main thread's Rust wasm then keeps ~4.5x the payload until reload). H6 recommends **100 MB**. (2) Real Safari by hand, plus the first Linux-WebKit CI result (then remove the `continue-on-error`/`if:` guards in `hub-client-e2e.yml`).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h6-hardening` in `.worktrees/workspace-7`, cut from `feature/pandoc-wasm` at `d35cab082`.
- Tasks ticked: all four (matrix, memory test, taxonomy audit, D2(b) spike not run by its condition). Open: the Close-out's "Open for Gordon" box only.
- State and gotchas:
  - WebKit: `webkit` project in `playwright.harness.config.ts` (`testMatch: **/pandoc-*.harness.spec.ts`; Playwright has no per-project workers, so `--workers=1` is on the command line: `npm run test:harness:webkit`). `npm run test:harness` and the CI harness step are pinned to `--project=chromium`. CI installs `chromium webkit`, runs `pandoc-browser-probe.harness.spec.ts` first (`continue-on-error`), and runs the suite only if it passed, else a `::warning::`. Linux WebKit is unverified until CI runs; Docker was not running locally. All 38 pandoc specs pass on macOS WebKit 26.4, including the cache-hit test.
  - Memory: `e2e/pandoc-memory.harness.spec.ts` (CI, both browsers; CDP `Runtime.getHeapUsage` after GC on Chromium: `Performance.getMetrics`\' JSHeapUsedSize does not count ArrayBuffers). `e2e/helpers/seedImages.ts` is the shared image generator; `window.__quartoTest.pandoc.rustWasmMemoryBytes()` is new. The measure spec sweep now has 20/100/144/192/288 MB rows and `rustWasmMb`. Its WebKit RSS growth column is unreliable (inflated baseline); trust Chromium's.
  - Findings that change other phases\' assumptions: worker memory is already recycled per render on the fresh path (not on the H10 warm preview path, where linear memory never shrinks and a worker is dropped above 512 MB or when idle); the Rust wasm never shrinks (~4.5x payload after a render) and cannot be recycled without reloading; reducing copies in request building is a Rust/R-lane change if wanted.
  - Taxonomy: `no-wasm` load code (WebAssembly absent) and `looksLikeOom` now exported from `@quarto/pandoc-host` (rebuild it before hub-client tests that import dist: `npm run build -w ts-packages/pandoc-host`).
  - H7 shares `ts-test-suite.yml` etc. with H1-H3 but H6 touched only `hub-client-e2e.yml` and hub-client files, so no conflict expected.
- Next step: H7 (check its Start gate), and the post-STOP menu wire-ups as R3-R5/R7 land.
