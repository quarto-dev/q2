# Task P1 report — Bug A fix + Bug C engine root + PC4 RED→GREEN (bd-h4rhohhy)

**Status: DONE_WITH_CONCERNS.** Both engine defects fixed upstream on
`q2-close-busy-fix` (not pushed), wired into the q2 fixture, PC1/PC2/PC4
RED→GREEN proven. One concern: the PC4a *live harness* isolation is imperfect
on macOS (see §7).

- Upstream `~/src/quarto-julia-engine` `q2-close-busy-fix` @ **93bce7b** —
  "Recover from busy/failed oneShot worker close; redirect detached server stdio"
- q2 worktree `braid/bd-h4rhohhy-q2-preview-engine-capture` @ **4efa5be84** —
  "Wire Bug A/C engine fix into julia fixture; flip PC4a to recovery (bd-h4rhohhy)"

---

## 1. Decision gate — QNR force-close surface (work item 2)

**Answer: YES — QNR exposes a forceful close.** Evidence (file:line, upstream):
- `julia-engine.ts:774` — `ServerCommand` union includes
  `| { type: "forceclose"; content: { file: string } }`
- `julia-engine.ts:783` — response map `forceclose: { status: true }`
- `julia-engine.ts:1100-1106` — `closeWorker(file, force)` sends
  `type: force ? "forceclose" : "close"`; the CLI `close --force` help reads
  "This will terminate the worker if it is running."
- **Live confirmation:** the existing upstream smoke test
  `force-closing a running worker` is GREEN.

**Selected decision branch: YES = recovery** (per the plan's ratified rule).
Pre-run close falls back to `forceclose` on busy; **frozen PC4 assertion = the
fresh `record_capture` SUCCEEDS with a real capture.** The answer fits the YES
branch cleanly — no NEITHER-branch ambiguity, no controller escalation needed.
Countersign requested on this evidence.

---

## 2. PC1/PC2 — upstream deno TDD (work items 3, 4)

**Seam:** the close orchestration was extracted into a pure `src/worker-close.ts`
module (`preRunClose` / `postRunClose` over an injectable `CloseCommandWriter`),
so the busy-recovery logic is unit-testable with a mocked command writer —
exactly the frozen "QNR socket/`writeJuliaCommand`" boundary. `executeJulia` now
calls these helpers through a thin adapter over `writeJuliaCommand`.

Tests: `tests/unit/julia-engine/worker-close.test.ts` (6 tests). `run-tests.{sh,ps1}`
now discover `tests/unit/` alongside `smoke/`.

### RED (pre-fix extraction reproduces Bug A at the unit tier)

The module was first written as a verbatim extraction of the inline `:703-718` /
`:742-749` logic (no busy handling). Both busy tests failed with the exact QNR
message:

```
running 6 tests from ./tests/unit/julia-engine/worker-close.test.ts
PC1: a busy post-run close after a successful run is non-fatal ... FAILED
PC2: a busy pre-run close recovers via forceclose ... FAILED
...
error: Error: Julia server returned error after receiving "close" command:
  ...
  Tried to close file "/tmp/sleepy.qmd" but the corresponding worker is busy.
    at postRunClose (src/worker-close.ts:40:9)
    at preRunClose  (src/worker-close.ts:31:11)
FAILED | 4 passed | 2 failed
```

### GREEN (fix applied)

```
running 6 tests from ./tests/unit/julia-engine/worker-close.test.ts
PC1: a busy post-run close after a successful run is non-fatal (warns, resolves) ... ok
PC1: a clean post-run close does not warn ... ok
PC2: a busy pre-run close recovers via forceclose and does not throw ... ok
PC2: a non-busy pre-run close error is NOT swallowed (propagates, no forceclose) ... ok
PC2: a closed (not-open) file skips the close entirely ... ok
isWorkerBusyError matches the QNR busy message and nothing else ... ok
ok | 6 passed | 0 failed
```

- **PC1** (post-run, `:742-749`): a failed cleanup close after a *successful*
  run warns (`quarto.console.warning`) and returns the result — non-fatal. Mock
  fails ONLY the close (honors the vacuity note structurally: `postRunClose`
  never sends `run`, and `executeJulia` still awaits the run directly, so run
  errors cannot be swallowed).
- **PC2** (pre-run, `:703-718`): a busy `close` falls back to `forceclose`
  (frozen assertion = the sequence `isopen→close→forceclose` and no throw). A
  **non-busy** close error still propagates (binds the `isWorkerBusyError`
  scoping — reverting the guard reddens this test).

Full upstream suite (`tests/run-tests.sh`): **9 passed** (2 existing smoke
suites incl. `force-closing a running worker` + 6 new unit tests), 0 failed.

---

## 3. Bug C engine-side root fix (work item 5)

`start_quartonotebookrunner_detached.jl`:
`run(detach(cmd), wait=false)` → `run(pipeline(detach(cmd), stdout=devnull,
stderr=devnull), wait=false)`. The detached QNR server no longer inherits the
launcher's (hence the Deno engine-host's) stdout/stderr, so its early output
can't land on the JSON protocol channel. `quartonotebookrunner.jl` still logs to
its own `logfile` via its internal pipe → no diagnostics lost. Redirecting to
the logfile instead would race QNR's own `open(logfile,"w")` truncation, so
devnull is the conflict-free choice.

**No deno-mock regression test** — the launcher is a Julia subprocess and the
fd-inheritance behavior is an OS/Julia-runtime property, not mockable in deno.
Covered by: the PC-C framing probes (GREEN at P0) + PC4a live GREEN end-to-end
(the whole capture survives through the real launcher). Windows path (PowerShell
`Start-Process -WindowStyle Hidden`) does not inherit stdio → unaffected, left
as-is.

---

## 4. Rebundle into q2 (work item 6)

- Upstream rebundle: `quarto call build-ts-extension src/julia-engine.ts`
  (78 modules, 45323 B). Verified fix markers present (`forceclose`,
  "returning results anyway", 2× `devnull`).
- Fixture rebundle: copied `src/julia-engine.ts`, new `src/worker-close.ts`,
  and `start_quartonotebookrunner_detached.jl` into
  `crates/quarto-core/tests/fixtures/extensions/julia-engine/`; rebuilt via the
  compat-log §4 temp-symlink workaround (`q2 build-ts-extension
  _extensions/julia-engine`, symlink removed after).
- **Byte-identity property survives:** the q2 build and the Q1 `quarto call`
  build of the fixed source produce **identical** bundles —
  `82bff64cc5d060cb48983945060a6932`, 45323 B (was `d9d5120…`, 44512 B).
- Engine-host bundle NOT rebuilt (no engine-host TS changed).

---

## 5. PC4 freeze + RED→GREEN (work item 7)

`pc4a_abandoned_worker_close_busy` (julia_engine_e2e.rs) assertion flipped from
`expect_err("worker is busy")` to the **frozen YES-branch**: `record_capture`
#2 must **succeed**, and (non-vacuous) the returned julia capture's
`result.markdown` must contain `cell-output` (proves the recovered run actually
executed the cell). Docstring updated to record the selected branch.

### RED — flipped assertion vs the PRE-fix bundle (`d9d5120…`)

```
thread 'pc4a_abandoned_worker_close_busy' panicked at julia_engine_e2e.rs:1046:9:
PC4a: fresh record_capture against the ABANDONED busy worker must SUCCEED
post-fix (pre-run close recovers via forceclose); got Err:
Stage 'engine-execution' failed: Execution failed in julia: Julia server
returned error after receiving "close" command:
...
Tried to close file ".../sleepy.qmd" but the corresponding worker is busy.
Summary [21.4s] 1 test run: 0 passed, 1 failed
```

### GREEN — flipped assertion vs the FIXED bundle (`82bff64…`)

```
PASS [83.253s] (1/1) quarto-core::integration julia_engine_e2e::pc4a_abandoned_worker_close_busy
Summary [83.254s] 1 test run: 1 passed (1 slow), 393 skipped
```

(83s because the recovered #2 actually runs the cell, which sleeps 60s.)
Run under `QUARTO_PC4A_LIVE=1` + isolation env (real julia 1.11.7,
`JULIA_DEPOT_PATH`, `QUARTO_JULIA_PROJECT`). User server pid 9828 verified
ALIVE before and after both runs.

---

## 6. Docs (work item 8)

- **Compat log §15** (new): supersedes §4/§5's zero-change/byte-identity claim;
  documents Bug A (decision gate + PC1/PC2 fix), Bug C (devnull redirect), the
  full upstream diff summary, the **oneShot-reuse design question** for the
  upstream PR, testing, and the harness-isolation concern.
- **Migration guide**: headline "zero source changes" corrected with an UPDATE
  banner + a closing caveat — porting took zero changes, but q2's harder
  `preview` exercise later exposed two latent *engine* bugs (present in Q1 too)
  that required source changes. Framed as engine maintenance, not q2 adaptation.
- **Plan §P1** checkboxes reconciled (all three checked with outcome notes).

---

## 7. Concern — PC4a live-harness isolation is imperfect (macOS)

The harness isolates via a temp `HOME`, but the julia runtime/transport dir
resolves to `QUARTO_JULIA_PROJECT` (the shared real `~/Library/Caches/quarto/
julia`), so the transport file is **not** actually isolated. The GREEN run
spawned QNR servers on the *shared* transport, and the temp-HOME-reading cleanup
guard missed them (6 leaked). **Cleaned up manually:** killed my 6 servers by
PID (process groups), removed the stale transport entry they left (it pointed at
a server I'd killed; the file read empty pre-test). **User server pid 9828 was
never touched** (verified alive throughout); other agents' pre-existing pool
(bd-l9jhy5u0) and a teammate's concurrent `quarto-core` run were left alone.

This is a **harness** defect, not a product defect, and it does not undermine
the RED/GREEN evidence (the close/busy→forceclose recovery was exercised and
observed). But the harness needs a real runtime-dir override (or an explicit
skip) before it can run safely unattended. Recommend a follow-up (relates to
bd-l9jhy5u0). Documented in compat log §15.

---

## 8. Verification counts (work item 9)

| Suite | Result |
|-------|--------|
| Upstream deno (`tests/run-tests.sh`) | 9 passed, 0 failed (incl. 6 new PC1/PC2) |
| q2 `cargo nextest run -p quarto-core` | 2633 passed, 34 skipped, 0 failed (incl. live j1..j6 on rebundled fixture) |
| q2 `cargo nextest run -p quarto-preview` | 87 passed, 1 skipped, 0 failed |
| PC4a live (`QUARTO_PC4A_LIVE=1`) | RED (pre-fix) → GREEN (fixed) |

Constraints honored: neither repo pushed; path-scoped commits in each; upstream
edits only on `q2-close-busy-fix`; frozen seams strengthened (never weakened);
`feature/ts-engine-extensions`, marimo, and unrelated julia processes untouched.

### Report-scope correction (review Minor)

For full honesty: commit **4efa5be84** (§4 above) also carried the controller's
P0/P2 checkbox-reconciliation edits to the plan file that were sitting
uncommitted in the worktree — not only my own §P1 edits. Likewise the fix-wave
commit below carries the controller's PC4 **countersign** note and the P3
"PC6 + PC4a shared-transport isolation" item (added to the plan file by the
controller, uncommitted in the worktree). These are disclosed here rather than
silently bundled.

---

# Fix wave (review response — task-p1-review.md, 2026-07-02)

Addresses the one Important + one Minor.

## Important — forceclose-itself-failing now bound + documented

Added a **7th** PC2 unit test
(`tests/unit/julia-engine/worker-close.test.ts`): mocked writer with
`close → busy`, `forceclose → rejects`; asserts the forceclose error
**propagates with its real message** (`assertRejects(..., forcecloseError)`),
is not swallowed or retried, and the command sequence stops at
`["isopen","close","forceclose"]` (no run attempted). Added a one-line contract
comment at the forceclose call site in `worker-close.ts`:

> Last line of defense. If the forced close ITSELF fails, that is a genuine
> environment failure (control server unreachable, etc.) — let it propagate;
> do not swallow or retry.

No behavior change (binds existing behavior). **Fail-on-revert proven:** wrapping
the forceclose in a swallowing `try/catch` reddened ONLY this test
(`6 passed | 1 failed`); restored → `7 passed | 0 failed`. Full upstream suite:
**10 passed** (7 unit + 3 smoke steps), 0 failed.

**Bundle unchanged (recorded honestly):** the comment + test did **not** change
`julia-engine.js` — `deno bundle` strips comments and the test is not bundled.
Both the upstream and q2-fixture bundles stay `82bff64cc5d060cb48983945060a6932`
(45323 B). No fixture bundle rebundle was needed; only the fixture's
`src/worker-close.ts` was synced for source parity. Compat log §15 updated.

## Minor — see "Report-scope correction" above.

## Fix-wave commits
- upstream `q2-close-busy-fix` @ **697a462** — "Bind + document the
  forceclose-itself-fails contract (review follow-up)"
- q2 @ (see final message) — fixture `worker-close.ts` parity + docs/plan.
