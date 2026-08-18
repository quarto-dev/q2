# Task P1c report — Bug C reader-side resilience (quarto-core ts_process, seam PC-C resilience leg)

**Status: DONE.** The engine-host stdout reader (`reader_loop`,
`crates/quarto-core/src/engine/ts_process.rs`) now log-and-skips up to a
bounded number of consecutive non-JSON stray lines instead of escalating a
single stray line into a whole-subprocess kill that broadcasts an error to
every in-flight request. TDD RED→GREEN proven; the frozen P0 framing probes
stay untouched and GREEN; `quarto-core` + `quarto-preview` nextest suites are
fully green (2721 passed, 0 failed, 35 skipped).

File touched: `crates/quarto-core/src/engine/ts_process.rs` (only file changed
— path-scoped, no other file in the tree modified).

---

## 1. TDD RED transcript

Two tests were added (replacing the stale `test_malformed_distinct_from_crash`,
whose name/assertions described exactly the escalate-on-first-line behavior
this task changes):

- `test_stray_lines_below_bound_are_skipped_not_fatal` — the discriminating
  test: two unrelated in-flight requests (A, B); two stray non-JSON lines
  land on the shared channel between them; both A and B must still receive
  their own real responses, and `shutting_down` must stay `false`.
- `test_malformed_beyond_bound_escalates_distinct_from_crash` — the
  invariant-preservation test: `MAX_CONSECUTIVE_MALFORMED_LINES + 1`
  consecutive stray lines (no valid frame in between) must still escalate to
  the pre-existing kill-channel behavior (`Other`, not `ProcessCrashed`;
  `shutting_down` set).

Supporting test infra: `MockState.malformed` was widened from
`Option<String>` to a `VecDeque<String>`, and a new
`MockWriteHalf::signal_malformed_many(&[&str])` was added so a test can queue
N consecutive stray lines **atomically under one lock** — avoiding a race
where the reader thread could drain a `signal_malformed` call before a
second one is queued (the queue is what makes the below/above-bound split
testable at all).

Ran against the **unmodified** `reader_loop` (constant
`MAX_CONSECUTIVE_MALFORMED_LINES` declared but not yet wired into the match
arm):

```
$ cargo nextest run -p quarto-core --lib -- \
    ts_process::tests::test_stray_lines_below_bound_are_skipped_not_fatal \
    ts_process::tests::test_malformed_beyond_bound_escalates_distinct_from_crash

     Summary [   0.225s] 2 tests run: 1 passed, 1 failed, 2272 skipped
        FAIL (2/2) quarto-core engine::ts_process::tests::test_stray_lines_below_bound_are_skipped_not_fatal
```

Failure (RED for the right reason — the discriminating test failed on the
exact old-code escalation path, not a typo/compile error):

```
thread '<unnamed>' panicked at crates/quarto-core/src/engine/ts_process.rs:1940:13:
A must still receive its own response after unrelated stray lines: Err(Other(
  "engine-host protocol error: non-JSON line on stdout (likely a stray
   console.log/console.info in the engine): \"[ Info: Log started at
   2026-07-02T13:11:20.379\""
))
```

(The `test_malformed_beyond_bound_escalates_distinct_from_crash` test passed
even pre-fix — expected, since old code already escalates on the *first*
malformed line, so N≥1 stray lines trivially also escalate. That test exists
to lock the invariant across the refactor, not to redden; the discriminating
RED evidence is the first test above.)

## 2. Implementation (minimal)

`reader_loop` now tracks a local `consecutive_malformed: u32` counter:

- Reset to `0` on every well-formed frame (the `Ok(Response {..})` arm).
- On `RecvError::Malformed(line)`: increment the counter, log an `ERROR` with
  a 200-char excerpt and the `n/BOUND consecutive` count. If
  `consecutive_malformed <= MAX_CONSECUTIVE_MALFORMED_LINES`, `continue` the
  loop — `shutting_down`/`pending`/`child` are left untouched. Otherwise, fall
  through to the **unchanged** pre-existing escalation: set `shutting_down`,
  drain and error every pending slot, kill the child, `break`.

No other logic changed (routing, EOF/crash, I/O-error arms are untouched).

## 3. TDD GREEN transcript

```
$ cargo nextest run -p quarto-core --lib -- \
    ts_process::tests::test_stray_lines_below_bound_are_skipped_not_fatal \
    ts_process::tests::test_malformed_beyond_bound_escalates_distinct_from_crash

    Starting 2 tests across 1 binary (2272 tests skipped)
        PASS [   0.086s] (1/2) quarto-core engine::ts_process::tests::test_malformed_beyond_bound_escalates_distinct_from_crash
        PASS [   0.171s] (2/2) quarto-core engine::ts_process::tests::test_stray_lines_below_bound_are_skipped_not_fatal
     Summary [   0.174s] 2 tests run: 2 passed, 2272 skipped
```

## 4. Chosen bound + rationale

`const MAX_CONSECUTIVE_MALFORMED_LINES: u32 = 5;`

- Small enough that a channel producing genuine, sustained garbage (wrong
  binary spawned, protocol version mismatch, a child that never stops writing
  to the shared fd) is still caught and terminated quickly — within 6 bad
  lines, not an unbounded amount of silently-dropped protocol traffic.
- Large enough to absorb the two concrete P0 live symptoms with margin: (a) a
  single leaked ANSI julia startup banner line, (b) one executeResult frame
  corrupted mid-flight by an interleaved foreign write (which framing-splits
  into at most a couple of bad lines, per the `pc_c_b_prime_interleaved_bytes_
  corrupt_frame` probe). Both are single-digit events, not sustained streams.
- The counter is **consecutive**, resetting on every well-formed frame — so a
  channel that's mostly healthy but occasionally emits one stray line (e.g. a
  future engine with a similar transient-leak bug) never accumulates toward
  the bound across its lifetime; only a *burst* of bad lines with no good
  frame in between trips escalation.

This is a judgment call, not a value derived from a hard constraint; if a
future symptom needs a larger/smaller bound, this is a one-line change with a
name attached (`MAX_CONSECUTIVE_MALFORMED_LINES`), not a design change.

## 5. No-hang investigation (brief item 2)

**Question:** what happens to an in-flight request whose response frame was
itself corrupted/consumed as one of the skipped stray lines — does it hang
forever now that a single stray line no longer broadcasts an error to every
pending slot?

**Finding: `TsEngineHost::request()` already has its own per-request timeout
mechanism, independent of the reader thread and unaffected by this change.**

`request()` (ts_process.rs, unchanged by this task) takes a `window:
Option<Duration>` and loops on `rx.recv_timeout(tick)` on the **caller's own
thread** — polling `cancellation.is_cancelled()` and `start.elapsed() >= w`
every tick (`CANCEL_TICK` = 250ms, or the window itself if shorter). When the
window elapses, it fires a cooperative `Cancel{target}` and returns
`Err(ExecutionError::Timeout{..})`. This loop does **not** depend on the
reader thread doing anything — even if the reader were fully blocked, the
caller's own timer still fires.

Auditing every `request()` call site:

| Call site | `window` |
|---|---|
| `ts_engine.rs:270` (dynamic `ClaimsLanguage` validation) | `Some(10s)` |
| `ts_engine.rs:317` (`ClaimsFile` validation) | `Some(10s)` |
| `ts_engine.rs:638`, `:704`, `:766` | `Some(10s)` |
| `ts_engine.rs:815` | `Some(30s)` |
| `load_engine` / `launch_engine` (internal) | `Some(DISCOVERY_WINDOW)` = `Some(10s)`, hardcoded |
| `ts_engine.rs:740` (**`Execute`** — the long-running user render/capture path, the one Bug C's symptom #2 corrupted frame hit) | `ctx.execute_timeout` |

`ctx.execute_timeout` (`crates/quarto-core/src/engine/context.rs:110`, wired
from `resolve_execute_timeout` in `engine_execution.rs:597`) resolves from
document metadata `execute.timeout`:

| metadata | window |
|---|---|
| absent / `true` (**default**) | `Some(DEFAULT_EXECUTE_TIMEOUT)` = `Some(300s)` |
| integer `N` | `Some(N seconds)` |
| `false` (**explicit opt-out**) | `None` |

**Conclusion:**
- In the **default configuration** (no `execute: timeout: false` in the doc),
  every request — including `Execute`, the path Bug C's corrupted-frame
  symptom hit — is bounded by a caller-side timeout (300s default, or a
  user-configured integer). A corrupted/dropped frame under the new
  bounded-skip policy causes that one request to time out after its window
  and return `Err(Timeout)`, exactly as any other slow/silent engine would.
  This is a strict improvement over the pre-fix behavior: previously the
  *entire host* died immediately (destroying every sibling in-flight
  request too); now only the one request whose frame was actually lost is
  affected, and only after its own timeout.
- **Concern (explicit, per brief item 2 — not fixed, not scope-expanded):**
  if a user explicitly sets `execute: timeout: false`, `request()`'s `window`
  is `None`. In that mode `request()` still polls `cancellation.is_cancelled()`
  every 250ms but has **no time-based bound at all**. If that specific
  request's response frame is the one that gets corrupted/dropped as a
  skipped stray line, and no *additional* stray lines subsequently arrive to
  trip `MAX_CONSECUTIVE_MALFORMED_LINES` (which would broadcast an error to
  it via the escalation path), that request now hangs until explicit
  cancellation (e.g. the user aborting the preview/render) — there is no
  automatic recovery. Before this change, ANY single stray line anywhere on
  the channel would have unblocked it (at the cost of also killing every
  other in-flight request). This is a narrow, `execute: timeout: false`-gated
  regression in "self-heals eventually" behavior, traded for the much more
  common-case win of not destroying unrelated in-flight work over one leaked
  banner line. I have **not** built new timeout machinery for this — per the
  brief's explicit instruction not to invent one without reporting first.
  If the controller wants this closed, the narrowest fix would be: give
  `None`-window requests an internal maximum wait distinct from
  "no timeout" (e.g. still respect `MAX_CONSECUTIVE_MALFORMED_LINES` but also
  cap total wall-clock wait even absent stray lines) — but that is a genuine
  design decision (what should "no timeout" mean when the channel is
  degraded but not dead?) and is out of this task's scope.

## 6. WASM-applicability note

`ts_process.rs` line 23: `#![cfg(not(target_arch = "wasm32"))]` — the entire
module (including `reader_loop`, the new constant, and the mock test infra)
is **compiled out on `wasm32` targets**. This crate/module is native-only
subprocess/thread infrastructure; it has no `wasm32`-visible code path. Per
the WASM rules (`.claude/rules/wasm.md`), no `npm run build:wasm` or hub
WASM rebuild was required or performed for this change — confirmed by
inspection of the `#![cfg(...)]` gate, not just assumption.

## 7. Verification counts

```
$ cargo nextest run -p quarto-core --lib
     Summary [   9.733s] 2251 tests run: 2251 passed, 23 skipped

$ cargo nextest run -p quarto-core -p quarto-preview
     Summary [  50.505s] 2721 tests run: 2721 passed, 35 skipped
```

The three frozen P0 framing probes (`ts_process_framing_probe.rs`) ran as
part of the combined suite and are unmodified and GREEN:

```
PASS quarto-core::integration ts_process_framing_probe::pc_c_b_foreign_line_is_malformed
PASS quarto-core::integration ts_process_framing_probe::pc_c_a_large_single_line_frame_parses
PASS quarto-core::integration ts_process_framing_probe::pc_c_b_prime_interleaved_bytes_corrupt_frame
```

Also ran (not requested by the brief, but touching shared infra warranted the
extra check): `cargo clippy -p quarto-core --lib --tests -- -D warnings`.
One pre-existing failure in `crates/quarto-core/tests/integration/
julia_engine_e2e.rs:1053` (`clippy::map_unwrap_or`) — **this file was not
touched by this task** (confirmed via `git status`/`git diff --stat`: the
only modified file in the tree is `ts_process.rs`); it predates this change
and is unrelated to it. No clippy warnings were reported for `ts_process.rs`
itself. `cargo fmt -p quarto-core -- --check` is clean (the repo's post-edit
hook runs `cargo fmt` automatically).

## 8. Contract comment update (brief item 3)

The `:930-935`-era comment ("Set shutting_down FIRST so the kill below
doesn't re-enter the crash path (finding #7 — one terminal error per exit)")
was rewritten in place. The new comment (at the top of the `Malformed` arm)
explains: what the old policy was and why it was too aggressive (Bug C —
engine-side leaks, e.g. a detached child inheriting the host's stdout fd, can
inject stray lines; killing all pending work over one banner is worse than
skipping it), what the new bounded policy is, and that the original
kill-everything behavior is deliberately preserved unchanged beyond the
bound (still needed to catch a genuinely broken wire). The original
"finding #7" rationale for the shutting_down-before-kill ordering is kept,
relocated to sit directly above the `shutting_down.store(true, ...)` line it
actually applies to now (only reached in the escalation branch).

## Not done / out of scope (confirmed correctly excluded)

- The engine-side root fix (detached QNR launcher stdio redirection) — P1's
  job, already landed upstream per the plan; not touched here.
- Un-freezing/modifying the P0 framing probes — explicitly frozen; untouched
  and still green.
- Any new timeout machinery for the `execute: timeout: false` + corrupted-
  frame edge case (§5 concern) — reported, not implemented, per brief
  instruction.
