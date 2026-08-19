# behave F4 crash-relaunch e2e: async launch-marker race (bd-qlnkdw9u)

## Overview

`behave_engine_e2e::f4_crash_yields_process_crashed_then_transparently_relaunches`
failed on PR #560's CI (ubuntu-latest, run 32279995673, 2026-08-19) — an
unrelated, lockfile-only PR. The assertion at
`crates/quarto-core/tests/integration/behave_engine_e2e.rs:681` expected
`count_containing("BEHAVE_LAUNCH_MARKER:1") == 2` and got 1:

```
captured engine_host messages: ["engine-host connected over loopback TCP",
 "engine-host spawned", "BEHAVE_LAUNCH_MARKER:1",
 "BEHAVE_CRASH_MARKER: intentional crash for Phase 4b-F crash-path e2e",
 "engine-host connected over loopback TCP", "engine-host spawned"]
```

**This is a test-side race, not a product bug.** The "crash" in the CI log is
the fixture's own intentional `BEHAVE_CRASH` → `Deno.exit(1)`, and the crash
recovery demonstrably worked: witness 1 ("engine-host spawned" seen twice,
line 668) passed, and execute-2 had already returned `Ok` before the failing
assertion ran. What was missing is process #2's `BEHAVE_LAUNCH_MARKER:1`
stderr line — and only in the *capture*, not in reality.

### Root cause

The two witnesses have different delivery paths:

- `"engine-host spawned"` is a `tracing` event emitted **synchronously on the
  calling thread** inside `ensure_started_inner` (`ts_process.rs:978`), so it
  is always captured before `execute()` returns.
- `BEHAVE_LAUNCH_MARKER:1` is written by the fixture's `console.error`, flows
  through the child's **stderr pipe**, and is forwarded into `tracing` by the
  **background stderr-forwarding thread** (`ts_process.rs::stderr_loop`,
  ~:1582). This path is fully decoupled from the request/response transport:
  execute-2 completing guarantees nothing about when (or whether yet) the
  marker line has been forwarded. On a loaded ubuntu runner the forwarder
  simply hadn't run when the bare `count_containing()` executed.

### Precedent

Exactly this race hit F3 on 2026-08-18 (loaded ubuntu runner, passed on
macos-latest, same commit) and was fixed in `ae01254ce` by introducing
`LaunchMarkerCapture::wait_for_count_containing` (bounded poll + trailing
settle that preserves the `== expected` upper bound). That commit applied the
helper to F3's `:2` relaunch witness only; F4's witness-2 — and the other
async-marker equality checks in F3/F4 that rest on fixed 200 ms sleeps —
stayed bare. CI has now reproduced the documented failure mode at one of
those remaining sites.

## The fix

Convert every assertion in `behave_engine_e2e.rs` that counts an
**asynchronously delivered** stderr marker from bare
`count_containing()` (guarded only by a fixed 200 ms sleep) to
`wait_for_count_containing(needle, expected, 10s)`:

- F4 line ~681 (the CI failure): `BEHAVE_LAUNCH_MARKER:1` == 2 after
  execute-2 — the mandatory fix.
- F4 lines ~637/~644: crash marker == 1 and `BEHAVE_LAUNCH_MARKER:1` == 1
  before execute-2 (same class; replaces the fixed 200 ms sleep).
- F3 line ~468: `BEHAVE_LAUNCH_MARKER:1` == 1 before execute-2 (same class;
  replaces the fixed 200 ms sleep). The `:2 == 0` absence check at ~475 keeps
  its protection via the preceding wait's trailing settle.

Assertions on `"engine-host spawned"` stay bare — that event is synchronous
and cannot race. The helper's trailing settle preserves each `== expected`
upper bound (a spurious extra marker/relaunch still fails), so no binding is
weakened.

## TDD note (fail-first evidence)

The race depends on the stderr-forwarder losing a scheduling race, which is
not deterministically reproducible on an idle dev machine. To honor the
fail-first discipline anyway, verification uses a **temporary, local-only
delay injection** in `stderr_loop` (sleep before forwarding lines containing
`BEHAVE_LAUNCH_MARKER`; never committed):

1. With the delay injected and only the *pre*-execute-2 checks hardened,
   F4 must fail at the witness-2 assertion with the exact CI signature
   (spawned == 2 captured, marker count 1).
2. With the full fix applied (delay still injected), F4 must pass.
3. Delay removed; normal runs + a repetition loop confirm stability.

## Work items

- [x] File strand bd-qlnkdw9u; link this plan
- [x] Worktree `.worktrees/bd-qlnkdw9u-behave-f4-crash-relaunch`
- [x] Write this plan document
- [x] Harden F4 pre-execute-2 async checks (~:637, ~:644) with `wait_for_count_containing`
- [x] Fail-first: injected stderr delay (750 ms on marker lines) reproduces the
      CI failure exactly — panic at :681, `left: 1, right: 2`, identical
      captured-messages list, hardened pre-checks and the synchronous
      spawned == 2 witness all passing
- [x] Fix: witness-2 (~:681) uses `wait_for_count_containing(_, 2, 10s)`;
      full behave suite 6/6 green with the delay still injected
- [x] Harden F3 pre-execute-2 check (~:468) the same way; remove the fixed sleeps
- [x] Remove the temporary delay injection (`git diff` clean on `src/`)
- [x] Run the behave e2e suite normally (6/6) + F4 repetition loop (30/30)
- [x] Full `cargo nextest run --workspace` (12907/12907 passed, 198 skipped)
      + review checklist (fmt clean, clippy clean, no HashMap/TODO in diff)
- [x] Commit; braid comment with results

## Results (2026-08-19)

Fail-first evidence: with a local-only 750 ms delay injected into
`stderr_loop` for `BEHAVE_LAUNCH_MARKER` lines and only the pre-execute-2
checks hardened, F4 failed at the witness-2 assertion with the exact CI
signature — panic at `behave_engine_e2e.rs:681`, `left: 1, right: 2`, and a
captured-messages list identical to the CI run's. With the witness-2 fix
applied and the delay still injected, the full behave suite passed 6/6
(proving the fix tolerates a forwarder stall far larger than the one CI
hit). Delay reverted; `git diff` on `crates/quarto-core/src/` is empty —
this change touches only the test file. Normal runs: behave suite 6/6, F4
30/30 in a repetition loop, full workspace suite green. All fixed
200 ms sleeps in the file are gone; every async-marker equality check now
rides the bounded-poll helper, while the synchronous "engine-host spawned"
assertions stay bare (they cannot race).
