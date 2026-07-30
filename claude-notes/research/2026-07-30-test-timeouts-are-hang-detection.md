# Test timeouts are hang detection, not performance budgets

**Date:** 2026-07-30
**Trigger:** flaky timeout of the smoke-all WASM sweep on `main`
**Immediate fix:** `hub-client/vitest.wasm.config.ts` `testTimeout` 30s → 120s
**Deferred refactor:** bd-0yyy4ipi (per-fixture tests)

This note exists so that the *next* agent hitting a timeout flake — in any
dev shell, not just the one where this happened — finds the diagnosis and
the heuristics instead of re-deriving them. If you are reading this because
a test timed out in CI, start at "Heuristics" below.

## The incident

`src/services/smokeAll.wasm.test.ts > all smoke-all fixtures render
correctly` timed out at 30,534ms against the 30,000ms `testTimeout` in
`vitest.wasm.config.ts`, failing the ubuntu leg of the TS Test Suite on
`main` at commit `727b0502` (the sass-vendoring PR #438). The macOS leg of
the same run passed in 20.1s.

Observed durations across nearby runs:

| Run | Code | ubuntu | macOS |
|---|---|---|---|
| main `e86a9275` (7/30 00:41) | pre-#438 | 25.6s | 13.2s |
| main `e2e7946d` (7/30 12:31) | pre-#438 | 25.5s | 24.4s |
| PR #438 branch `ebc0c802` | with #438 | 26.5s ✓ | 16.6s ✓ |
| main `727b0502` (the failure) | with #438 | **30.5s ✗** | 20.1s ✓ |

Diagnosis: **not a broken test.** The batch was already running at ~85–90%
of its timeout on ubuntu runners; #438 added ~1s (bigger SCSS theme bundle
per render), and normal runner-speed variance did the rest. Note the macOS
column bouncing between 13s and 24s across *green* runs — shared-runner
variance of 2–3× is normal and irreducible. Any timeout with less than ~2×
headroom over the observed max will eventually flake.

Structural aggravator: the test renders *every* `smoke-all` fixture inside
a single `it()`, so its cost is O(fixture count) and grows as fixtures are
added — a fixed timeout over a growing workload has a built-in expiry date.

## Heuristics

Distilled from how mature ecosystems (Go, Bazel, cargo-nextest, pytest,
vitest/jest) handle this, balanced against the SRE-guide toil framing
(every false-positive CI failure costs a human interrupt *and* erodes trust
in red builds — "eh, probably that timeout thing again" is the truly
expensive failure mode).

1. **A timeout's job is hang detection, not performance enforcement — so
   it should be embarrassingly loose: 5–10× typical duration.** Go
   defaults to 10 *minutes* per test binary. Bazel makes you pick coarse
   size buckets (60s / 300s / 900s / 3600s) and *separately* warns when a
   test could fit a smaller bucket. cargo-nextest splits `slow-timeout`
   (warn) from `terminate-after` (kill). A tight timeout is an accidental
   performance SLO enforced by a flaky page. If you care about a test
   getting slower, that's a duration-trend signal, not a kill threshold.

2. **Never let a growing N share a fixed budget.** For batch/parameterized
   tests, either scale the timeout with the workload
   (`N × perItemBudget + setupBudget`), or split into per-item tests so
   each gets its own timeout, or shard across CI jobs (only when a sweep
   reaches many minutes).

3. **Retries are a shock absorber, not a fix, and need a paper trail.**
   Retry keeps humans unblocked, but un-telemetered retries convert loud
   flakes into silent rot. For timeout flakes specifically retries are the
   wrong tool anyway (you pay the full timeout twice); fix the threshold.

4. **Spend engineering effort on the second recurrence, not the first
   (rule of three).** First false positive → one-line fix (bump/scale the
   timeout). Same class of problem returning for a *different* reason →
   invest in mechanism (per-item tests, duration tracking, sharding). A
   generous timeout costs nothing until a real hang, where learning about
   it at 120s instead of 30s is irrelevant.

## What was done here

- `testTimeout` in `hub-client/vitest.wasm.config.ts` raised to 120s, with
  a comment marking it as hang-detection-only and pointing here.
- bd-0yyy4ipi filed (p3) for the per-fixture refactor: the test file's
  comment claims vitest can't register tests from async discovery, but
  vitest ESM test files support top-level `await`, so
  `const files = await discoverTestFiles(...)` + `it.each(files)` works
  (alternative: a checked-in generated manifest with a test asserting it
  matches the directory). That refactor also buys per-fixture reporting,
  replacing the hand-rolled `failures[]` accumulator. Per heuristic 4, do
  it when the sweep's size/growth actually becomes a problem again.

## If you hit a timeout flake elsewhere in this repo

1. Check headroom: pull recent durations for the test from green CI runs
   (`gh run view <id> --log | grep '<test name>'`). If the baseline is
   above ~50% of the timeout, it's a threshold problem, not a code problem.
2. Bump the timeout to 5–10× typical (or make it scale with N). Don't
   tune it "just above" observed max — that schedules the next incident.
3. If the test is a monolithic batch, consider whether this is the second
   recurrence — if so, split it (see bd-0yyy4ipi for the pattern).
4. Leave the timeout's *purpose* in a comment so nobody re-tightens it in
   a cleanup pass.
