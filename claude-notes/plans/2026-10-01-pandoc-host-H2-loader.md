---
title: 'Plan: Loader and worker lifecycle (pandoc-host H2)'
date: 2026-10-01
---

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D2, D4, D6, D8.5, Failure taxonomy)
**Depends on:** H1. **Unblocks:** H3.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H1 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H4 (its Start gate decides whether it can begin).

## Overview

The main-thread loader that fetches, caches, decompresses, verifies and compiles `pandoc.wasm`, keeps the resident `WebAssembly.Module`, and drives one short-lived worker per render with abort, timeout and idle drop, plus the failure taxonomy and the Playwright smoke tests.

## Decisions

- The `Module` is compiled and held on the main thread and posted to a fresh worker per render, so terminating a worker never discards the compile (D2).
- One download in flight at a time; a new click supersedes (D8.5).
- Wall timeout 120 s; the idle policy drops the resident `Module` after 5 minutes (recompiled from the cache on the next click). Both are decided.

## Checklist

### Tests first
- [x] A corrupted or wrong-SHA cache entry is refetched and never compiled; the SHA compared is of the *decompressed* bytes against the request's `expected_pandoc_wasm_sha256` (a fixture whose `.gz` hashes to the expected value but whose wasm does not is rejected); an old-SHA entry is evicted.
- [x] The three loader input cases: gzip, raw wasm, neither (some servers add `Content-Encoding: gzip` to `*.gz`).
- [x] `caches.open` throwing `SecurityError` proceeds uncached; `QuotaExceededError` likewise. A cancel during a Cache API write leaves no unverified entry in use and the next load succeeds.
- [x] A fake `Worker` sees `terminate()` once on abort and on timeout; two quick clicks leave one live worker and reject the first as superseded.
- [x] A late result message from a terminated worker triggers no download; idle drop, with fake timers, recompiles from the cache on the next click.
- [x] About 20 consecutive renders keep memory flat within a bound.

### Tasks
- [x] **Main-thread loader**, in a hub-client service module (it needs `document.baseURI`, `caches` and `crypto.subtle`; the DOM-free host package stays DOM-free). Each of `DecompressionStream`, `Cache` and `crypto.subtle` is feature-detected with its own message and test.
  - Fetch `pandoc.wasm.gz` with progress and cancel. Sniff the magic bytes (`1f 8b` gzip versus `\0asm`, accept both) because some servers add `Content-Encoding: gzip` to `*.gz` and the browser then decodes it transparently (defensive: some static hosts and CDNs do this).
  - Cache API and `DecompressionStream` per D6; SHA check on the decompressed wasm. `crypto.subtle` exists only in secure contexts, so on a plain-http non-localhost origin the feature is unavailable with a clear message, as `caches` is.
  - Compile once and keep the `Module`; exnref feature detection with a message naming minimum browser versions (D4).
  - The main thread passes an absolute URL (`new URL(p, document.baseURI)`) since the app uses `base: './'`.
- [x] **Worker lifecycle.** One short-lived worker per render with the `Module` posted in; abort = terminate; 120 s wall timeout; an idle policy that drops the resident `Module` after 5 minutes. Peak memory (16 MB gz + 59 MB decompressed + hashing + compile, beside the Rust wasm and Automerge) is measured in H3 and stated as a budget. Call `navigator.storage.persist()` or show a clear message for "evicted + offline"; offline works only after first online use. A supersede or cancel aborts the render only: a wasm load already in flight is shared by the next click unless the user cancelled it, and the idle timer runs only while no load or render is active.
- [x] **Failure taxonomy** (design): each class is one diagnostic plus one UI state with a test, using the fault-injection hook.
- [x] **Worker naming.** The worker is `pandoc.worker.ts` deliberately: Vite's `pandoc.worker-<hash>.js` matches `globIgnores: ['**/*.worker-*.js']` and the `ondemand-assets` route (`vite.config.ts:147,180`; shared `maxEntries: 12` with Monaco's workers), keeping it out of the precache.
- [x] **Playwright smoke** (~6 independent tests): no pandoc request before first use; exnref-absent friendly error (inject `WebAssembly.validate` false); abort mid-fetch; cache hit on the second run; fault-injected OOM/crash/hang; absolute URL under a subpath deploy. Specs are `pandoc-*.harness.spec.ts` under `playwright.harness.config.ts`, driven by a `VITE_E2E`-gated hook in `src/test-hooks.ts` (the existing test-hook mechanism; compiled out of release builds), and the subpath test rewrites the prefix with `page.route`. `hub-client-e2e.yml` builds with `npm run build` and installs chromium only (`:169`); Vite copies `public/` at build time, so its download step goes before "Build TypeScript packages" (otherwise the loader finds no asset and every smoke assertion fails). The wall timeout is a parameter the test hook can shorten (the hang test would otherwise take 120 s), and requests and the share tree come from the R0 recordings passed through the hook, since R2's exports do not exist yet. *(As built: the hook builds a tiny markdown-to-plain job in the page (`src/pandoc/smokeJob.ts`) instead of passing an R0 recording across the Playwright boundary: these tests exercise the loader and worker, not a format, and oom is provoked with `+RTS -M256k`. The real-wasm vitest still uses the recordings for oom.)*

## Verification

Real-wasm vitest through the loader (compressed fixture to compiled `Module`, including the corrupted-cache and sniffing cases) and the Playwright smoke in CI.

## Exit

A request can be run from a page with abort, timeout and caching working. The dev harness and the parity net come in H3.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h2-loader` in `.worktrees/workspace-7` (cut from `feature/pandoc-wasm` at 5fd626d4e).
- Last commit; tasks ticked: all Tests-first, Tasks and Close-out boxes ticked in the work commit.
- Phase-boundary gates (2026-10-01): workspace nextest 15523 passed, 202 skipped, against H1's recorded 15504/202: +19, none from H2 (no Rust touched); the delta comes from R1's commits after that measurement (0271e86dd added `pandoc_request/args.rs` normalizer code and tests), not individually counted. hub-client unit 1317, wasm 205, pandoc-host 33 all green; Playwright pandoc smoke 6/6.
- State and gotchas:
  - Code: `hub-client/src/pandoc/{pandocLoader,pandocRunner,pandoc.worker,pandocService,smokeJob}.ts`; tests `pandocLoader.test.ts`, `pandocRunner.test.ts` (fakes), `pandocLoader.wasm.test.ts` (real wasm, `worker_threads` via `src/test-utils/nodeWorker.ts`), `e2e/pandoc-loader.harness.spec.ts` (6 Chromium tests, pass with `--retries=0`). Docs: `dev-docs/pandoc-wasm-host.md`.
  - **A bare `for (;;);` is deleted by esbuild's minifier**, so H1's hang fault did nothing in a production bundle (the Playwright hang test caught it). `execute.ts` now spins on `performance.now()`; the package `dist/` was rebuilt. Any new "hang" must survive minification.
  - `@quarto/pandoc-host` resolves to `ts-packages/pandoc-host/dist` in a vite *build* (source only under vitest aliases): after editing the package, `npm run build -w ts-packages/pandoc-host` before building hub-client, as CI does.
  - Added host diagnostic codes (`pandoc-timeout`, `wasm-unsupported`, `worker-blocked`, `compile-blocked`, `download-failed`, `checksum-mismatch`, `offline`) to `HostDiagnosticCode`. `uiStateFor(outcome)` maps each failure class to one UI state for H5.
  - The Playwright page hook `window.__quartoTest.pandoc` builds the request in the page (see the note on the Playwright task). Wait for `window.__quartoTestReady` to be *assigned* (`waitForFunction`) before awaiting it: `goto`\'s load event can precede main.tsx.
  - `-M256k` makes a trivial document exit 251; `-M1m` does not.
  - vitest `toEqual` on two 16 MB typed arrays exhausts the heap; use `Buffer.equals`.
  - The Cache API is optional (proceeds uncached with a notice); `DecompressionStream`, `crypto.subtle` and exnref are hard errors with their own messages. Peak memory budget is still H3's job; measured here: ~8 MB RSS growth over 20 renders (Node).
  - The runner transfers the *request's* byte buffers to the worker (caller loses them); the share tree is cloned, since the caller reuses it.
  - Not done here (belongs to later phases): Rust classification of exit codes and warnings (`Q-20-3`, `Q-11-1`) and their UI; H4 wires the fetch script into build:all / test:e2e / test:harness / build-local-prod.sh.
- Next step: H4 (its Start gate decides).
