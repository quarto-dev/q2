# Task P3 report — error-path coverage (PC3, PC8), julia-harness isolation, full verification

**Status:** DONE. This session completed verification only — the substantive TDD and
isolation work below was done by a previous session (killed mid-verification by a
session pause) and is reported here from `git show`, not re-derived. Where the report
says "predecessor's evidence," that RED/GREEN transcript was not independently
reproduced by this session.

## 1. What the predecessor's commits contain

### c16200f92 — `test(preview-capture): PC3/PC8 error-path coverage (bd-h4rhohhy)`

```
crates/quarto-preview/src/capture_driver.rs | 127 ++++++++++++++++++++++++++++
crates/quarto-preview/src/re_execute.rs     | 121 ++++++++++++++++++++++++++
2 files changed, 248 insertions(+)
```

**PC3** (`capture_driver.rs`, `pc3_failing_engine_does_not_block_next_doc_capture`):
a `FailingTestEngine` (declares `test-failing`, `execute()` always returns
`ExecutionError::Other(...)`) registered alongside the existing
`PassthroughTestEngine`. Two docs, `a-failing.qmd` (engine `test-failing`) and
`b-echo.qmd` (engine `test-passthrough`) — the `a-`/`b-` prefix matters because
`qmd_files` is sorted (`discovery.rs:154`), guaranteeing the failing doc runs first,
so a loop-return-on-Err regression can't hide behind ordering. Assertions: doc A's
failure emits `Q-PREVIEW-CAP-1` to the test diagnostic sink, AND doc B's capture is
still recorded (`record_eager_captures`' continue-on-error contract,
`capture_driver.rs:116-140`). No product-code change — both behaviors already
existed; this closes an untested gap.

Fail-on-revert proven both ways (per commit message, not re-run by this session):
(a) commenting out the `sink.emit(...)` call reddens the diagnostic-count assertion;
(b) replacing the loop's `Err(e) => { .. }` arm with an early `return Err(..)`
reddens doc B's capture assertion (loop stops at the first failure).

**PC8** (`re_execute.rs`, `pc8_re_execute_failure_sets_error_state_and_emits_diagnostic`):
a `FailingReExecuteEngine` sharing the doc's declared engine name
(`test-passthrough`) but always failing, installed via a **wholesale registry
override** applied only at re-execute time (the seed run used the real
`PassthroughTestEngine` and is unaffected). Uses a **separate cache dir** from the
seed run — required because `record_capture_cached` keys purely on
`sha256(input_qmd)` (`cache.rs:150-163`), and content is unchanged between seed and
re-execute, so reusing the seed's cache dir would replay the cached success and
never invoke the failing engine. Assertions: `perform_re_execute`'s failure branch
(`re_execute.rs:253-279`) writes sidecar `CaptureState::Error` + `last_error`, and
emits `Q-PREVIEW-RE-1`. No product-code change.

Fail-on-revert (per commit message): commenting out the
`ctx_for_task.index().set_capture(&rel_path_for_task, &errored)` write leaves the
sidecar stuck at whatever state `claim_and_spawn` set pre-run, reddening the
`CaptureState::Error`/`last_error` assertions.

### 4785c9d2c — `test(preview-capture): isolate QUARTO_JULIA_PROJECT in live-julia harnesses (bd-h4rhohhy P3)`

```
.../plans/2026-07-02-preview-capture-delivery.md   |  54 ++++++++-
.../tests/integration/julia_engine_e2e.rs          | 129 ++++++++++++++++++++-
.../e2e/engine-capture-splice-julia.spec.ts        |  77 +++++++++---
3 files changed, 236 insertions(+), 24 deletions(-)
```

PC4a and PC6 already isolated the julia transport/server via a temp `HOME`. This
commit adds a **second isolation layer**: `isolate_julia_project()` (Rust,
`julia_engine_e2e.rs:124-138`) / `isolateJuliaProject()` (TS,
`engine-capture-splice-julia.spec.ts`) copy the ambient `QUARTO_JULIA_PROJECT`'s
`Project.toml` + `Manifest.toml` into a per-test temp dir and re-point
`QUARTO_JULIA_PROJECT` at the copy, so the detached server's `--project=` flag
never names the shared real directory (`JULIA_DEPOT_PATH` stays shared — no
package re-instantiation). Both harnesses gained a
`SharedTransportSentinel`/`captureSharedTransportMtime()` assertion that the
shared `~/Library/Caches/quarto/julia/julia_transport.txt` existence+mtime is
unchanged across the run.

**Live-verified twice by the predecessor** (evidence from the commit message,
not re-run by this session): `pc4a_abandoned_worker_close_busy` (78.8s,
`QUARTO_PC4A_LIVE=1 ... --run-ignored all`) and PC6
(`QUARTO_PC6_LIVE=1 npx playwright test engine-capture-splice-julia`, 8.6s) —
both PASSED; shared transport file mtime/existence unchanged both times;
`IsolatedJuliaServerGuard` reaped every process it spawned (no new leaked pids).
Found but explicitly did NOT touch ~28 pre-existing leaked julia processes on the
shared transport (pre-existing `bd-l9jhy5u0` leak, out of scope, "never touch
processes you didn't start" constraint honored).

Decision recorded: `QUARTO_PC6_LIVE` stays **opt-in** — isolation is no longer the
reason (proven safe); the remaining reason is environmental/speed (real
julia+deno dependency, multi-second server boot), mirroring PC4a's `#[ignore]`
gate. Documented in the spec's file header and (now, after this session's fix
below) the plan's seam table.

## 2. Dangling edit disposition (this session)

One uncommitted edit was found on disk in
`crates/quarto-core/tests/integration/julia_engine_e2e.rs`: a clippy-shape
rewrite of PC4a's markdown extraction, `.map(|c| ...).unwrap_or_else(|| panic!(...))`
→ `.map_or_else(|| panic!(...), |c| ...)`. Verified semantically identical (same
panic-on-missing-capture, same markdown extraction on hit) and confirmed it
compiles clean (`cargo check -p quarto-core --tests`). Committed as
`f0728dd63` — `style(quarto-core): use map_or_else in PC4a markdown extraction
(bd-h4rhohhy)`.

## 3. Verification ladder (this session)

All logs in `/tmp/bd-h4rhohhy-p3-logs/` (not committed — scratch).

### Leg A — `cargo nextest run -p quarto-preview -p quarto-core`

**Result: 2723 tests run: 2721 passed, 2 failed, 35 skipped.**

Log: `/tmp/bd-h4rhohhy-p3-logs/leg-a-nextest.log`.

Two failures, both in `julia_engine_e2e.rs`, **neither part of this branch's
PC3/PC8/PC6 work** (pre-existing tests from the original julia-validation plan):

- `julia_engine_e2e::j1_minimal_julia_render`
- `julia_engine_e2e::j2_document_level_echo_false_hides_source_keeps_output`

Both failed with `Julia server returned error after receiving "isopen" command:
Incorrect HMAC digest` — a shared-julia-transport handshake failure. These
tests use `setup_julia_project()` (NOT the isolated
`IsolatedJuliaServerGuard`/`isolate_julia_project()` path — that's PC4a-only),
so they connect to the AMBIENT `~/Library/Caches/quarto/julia/julia_transport.txt`.
At the time of the run the machine also had **~49 leaked julia server
processes** from unrelated test activity (the pre-existing `bd-l9jhy5u0`
worker-leak bug), which was the initial suspect.

**Confirmed transient, not a regression**: re-ran each failing test in isolation
(single test, no concurrent julia contention):

```
cargo nextest run -p quarto-core --test integration -- julia_engine_e2e::j1_minimal_julia_render
  → PASS (6.1s)
cargo nextest run -p quarto-core --test integration -- julia_engine_e2e::j2_document_level_echo_false_hides_source_keeps_output
  → PASS (5.9s)
```

Both pass cleanly in isolation. **The full diagnosis came during Leg C** (see
below): the orphan pool was only an amplifier — the real root cause is that the
`julia_engine_e2e` tests race EACH OTHER on the single ambient transport file.
Fixed by nextest serialization (commit `156b290ec`); see Leg C.

### Leg B — `cargo build --bin q2` + q2-preview-spa `npm run test:e2e`

`cargo build --bin q2`: succeeded (log: `leg-b-build.log`).

`npm run test:e2e` (log: `leg-b-e2e.log`): **37 passed, 1 skipped, 1 failed**
(39 total across the `chromium` + `firefox-ws-queue` projects).

- Skipped: `[chromium] engine-capture-splice-julia.spec.ts` PC6 — expected,
  opt-in (`QUARTO_PC6_LIVE` unset).
- Passed: PC5 (`engine-capture-splice.spec.ts`) — `PC5: recorded echo capture
  splices into the pane without reload`.
- Failed: `[firefox-ws-queue] firefox-ws-queue.spec.ts` —
  `browserType.launch: Executable doesn't exist at
  .../ms-playwright/firefox-1522/firefox/Nightly.app/...` — Firefox not
  installed on this machine. This is the **known pre-existing failure** the
  brief called out; it is the ONLY e2e failure, matching the expected shape.

### Leg C — full `cargo xtask verify` (three runs; the whole story)

**Run 1** (log: `leg-c-xtask-verify.log`, dirty machine — ~49 orphaned QNR
processes present): FAILED at the Rust-tests step.
`Summary [78.4s] 8624/10589 tests run: 8623 passed, 1 failed, 198 skipped`
(nextest fail-fast cancelled the rest). Failure:
`julia_engine_e2e::j1_minimal_julia_render`, `Incorrect HMAC digest` at
`isopen`. Initial hypothesis: contention from the orphaned QNR pool
(bd-l9jhy5u0).

**Orphan-pool cleanup** (user-approved, done by the coordinator between runs):
~50 orphaned QuartoNotebookRunner processes were reaped via category-pattern
pkill; the user's own julia server (pid 9828) was preserved. Machine clean.

**Run 2** (log: `leg-c-xtask-verify-2.log`, clean machine): FAILED AGAIN.
`Summary [74.6s] 8758/10589 tests run: 8756 passed, 2 failed, 198 skipped`.
Failures: `julia_engine_e2e::j3_exeflags_and_env_through_julia_block` and
`julia_engine_e2e::j4_error_handling_does_not_wedge_host` — same
`Incorrect HMAC digest` signature, but DIFFERENT victims than run 1.

**Diagnosis — intra-suite race, structural, pre-existing.** Rotating victims
on a clean machine rule out the orphan pool as the root cause. The
`julia_engine_e2e` tests had NO nextest serialization; each uses
`setup_julia_project()`, which isolates the project dir but NOT `HOME`, so
every concurrently-running j-test's `daemon: false` render boots its own QNR
server against the SINGLE ambient
`~/Library/Caches/quarto/julia/julia_transport.txt` — concurrent startups
overwrite each other's transport entry (port/pid/HMAC key), and a client that
reads the wrong entry fails the socket handshake with `Incorrect HMAC digest`.
Which test loses the race depends on scheduling — hence rotating victims.
This is a **pre-existing test-infra property, not caused by this branch's
diff**: the j-tests, `setup_julia_project()`, and the shared-transport reuse
design all predate P3 (they belong to the julia-validation plan), and this
branch's P3 commits touch only new PC3/PC8 tests, the PC4a/PC6 isolation
helpers (which are NOT used by j1-j6), and a comment-shape edit. We did NOT
re-verify on the merge-base — a merge-base run would race the same way only
probabilistically (the failures are scheduling-dependent, so a green
merge-base run would prove nothing and a red one would only confirm what the
structural argument already establishes: none of the racing components
changed on this branch).

**Fix** (commit `156b290ec`, config-only, precedent bd-u3ze): added a
`julia-shared-transport = { max-threads = 1 }` test-group in
`.config/nextest.toml` with an override filtering
`package(quarto-core) & binary(integration) & test(/^julia_engine_e2e::/)`,
so at most one j-test is in flight at a time. The j-tests themselves are
untouched (out of this task's scope; full hermetic isolation for them is
being filed as a separate strand by the controller).

**Run 3** (log: `leg-c-xtask-verify-3.log`, clean machine + serialization):

**Run 3 (post-serialization, log: `leg-c-xtask-verify-3.log`): ALL GREEN.**
Tail: `✓ All verification steps passed!`; nextest summary:
`10589 tests run: 10589 passed, 198 skipped` (0 failed — the julia_engine_e2e
tests now run serialized in the `julia-shared-transport` group). All downstream
legs (ts-packages builds + mcp smoke, hub-client `build:all` incl. WASM,
hub-client `test:ci`) completed successfully.
*(Filled in by the controller from the run-3 log after the implementer session
ended; independently re-verified by the P3 reviewer.)*

## 4. PC6 gate decision

**Decision: PC6 stays opt-in (`QUARTO_PC6_LIVE=1`), per the predecessor's
already-recorded rationale — confirmed correct by this session, plan updated to
match.**

Read `isolate_julia_project()` (`julia_engine_e2e.rs:124-138`) and
`isolateJuliaProject()` (`engine-capture-splice-julia.spec.ts`), and the spec's
file-header comment. The isolation code is real (copies `Project.toml`/
`Manifest.toml` into a per-test temp dir, re-points `QUARTO_JULIA_PROJECT`,
layered on top of the pre-existing temp-`HOME` transport override) and its
safety claim is backed by the predecessor's two live runs (evidence above) — not
independently re-run by this session (re-running would spawn another real julia
server on an already-contaminated machine — 49 leaked processes present — for no
new information; the predecessor's evidence already satisfies "prove it").

Per the brief: "drop the `QUARTO_PC6_LIVE` opt-in gate IF the isolation makes it
safe for the julia-gated tier, else record explicitly why it stays opt-in." The
isolation DOES make PC6 safe from a shared-state-corruption standpoint — but
"safe" and "fast/deterministic enough for the default CI suite" are different
questions. PC6 spawns a real julia server (network-installed julia binary,
multi-second boot, ~6.5-8.6s per the recorded runs) — the same class of cost
that keeps PC4a behind `#[ignore]` on the Rust side. The gate stays for that
reason, not for isolation safety.

**Plan/code disagreement found and fixed**: the plan's seam table PC6 row was
STALE — it still read "Opt-in because a temp HOME does NOT isolate the shared
julia transport... un-deferral tracked in P3" (the PRE-P3 rationale), even
though the P3 checklist item immediately above it (already correct, predecessor's
text) already recorded the isolation-closed / speed-is-the-reason-now decision.
Fixed in this session: the PC6 seam-table row now states the post-P3 rationale
explicitly and points to this report. (Commit: bundled with the plan-checklist
reconciliation below.)

## 5. TDD evidence provenance

**RED/GREEN evidence in §1 above (PC3, PC8, PC4a/PC6 live runs) is the
predecessor's** — read from commit messages and code comments, not independently
re-derived or re-run by this session. This session's own verification work is:
the dangling-edit compile check (§2), the full ladder (§3), and the isolated
re-runs of `j1`/`j2` that confirmed those two failures are transient (§3, Leg A).
