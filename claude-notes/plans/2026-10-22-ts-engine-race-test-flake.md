---
title: 'CI flake: ts_engine test_race_free_instance_exclusive times out (bd-cig41dcs)'
date: 2026-10-22
description: 'A polling watcher thread in the ts_engine race test can give up early or answer the wrong request id, which leaves both ensure_launched calls waiting until the watchdog fires; fix it by matching on recorded request ids and waiting until the response is delivered.'
status: done  # Implemented 2026-10-22 on braid/bd-cig41dcs-ts-engine-race-flake
braid:
  strand: bd-cig41dcs
---

**Branch:** `braid/bd-cig41dcs-ts-engine-race-flake` (topic branch in the main checkout, based on `main` @ `8ddbf8b17`)

## Triage verdict

**Ready to implement** (design questions settled; see Decisions). Reading the code is enough to find the cause: two
separate timing paths each produce the exact CI signature (failure at
15.010 s with `DEADLOCK DETECTED`). The fix is test-only and stays inside
the `MockWriteHalf` test harness and the tests that use it.

## Issue context

Filed 2026-10-22 by Carlos Scheidegger. P2 bug, labelled `flake`. The test
failed on PR #906 (ubuntu-latest, run 39851900839, job 120081314475), and
the PR's diff did not touch this area:

```
FAIL [15.010s] quarto-core engine::ts_engine::tests::test_race_free_instance_exclusive
panicked at crates/quarto-core/src/engine/ts_engine.rs:1367:14: DEADLOCK DETECTED: test timed out: Timeout
```

The strand suggests the 3 s watcher budget as the cause (an untested
guess) and notes that the watcher uses `sent_messages()` indices as
response ids.

## Dependency graph

- **discovered-from**: bd-t3n1nghb (link-external-* for HTML output). This
  is only where the flake was seen: it hit PR #906's CI. That strand has
  no other connection to this one.
- No `blocks` or `related` edges. Nothing depends on this strand, so the
  only pressure is CI noise on unrelated PRs.

## What the code looks like today

The test is at `crates/quarto-core/src/engine/ts_engine.rs:1462`
(`test_race_free_instance_exclusive`) and is unchanged since the strand
was filed. The mock is `crates/quarto-core/src/engine/ts_process.rs`
§`mod mock` (~:1709). The relevant facts:

- `MockState.sent` stores `(id, ToEngine)`, but `sent_messages()` drops the
  id (`ts_process.rs:1906`).
- `TsEngineHost::request` takes `id = next_id.fetch_add(..)` and only then
  calls `write.send()`, which pushes onto `sent` (`ts_process.rs:1176–1200`).
  When two threads send at once, their **push order can differ from their
  id order**.
- `launch_engine` waits up to `DISCOVERY_WINDOW` = **10 s**
  (`ts_process.rs:756`). `ensure_launched` holds the `instance` mutex for
  the whole request (`ts_engine.rs:494`).
- The reader drops responses for ids that have no pending slot without
  reporting anything (`ts_process.rs:1475`).

### Why a missed delivery shows up as a 15 s timeout

If the watcher never answers the real LaunchEngine id, the thread holding
the `instance` mutex times out after 10 s. The other thread then takes the
mutex and sends a **second** LaunchEngine, which waits another 10 s. That
adds up to ≥ 20 s, so the 15 s watchdog fires first. The reported
15.010 s matches this.

### Cause A: the watcher gives up early (the strand's hypothesis)

The watcher makes 300 polls 10 ms apart, which is about 3 s at minimum,
and then exits. If neither thread has sent LaunchEngine by then (for
example, a CI runner under heavy load delays thread start, `ensure_started`
or the LoadEngine round trip), nothing ever answers it.

### Cause B: index ≠ id (found during this investigation; probably the more likely one)

Both threads pass the `Barrier` and call `ensure_loaded` together. Neither
has discovery yet, so **both send LoadEngine**. `ensure_loaded` documents
this double send as accepted (`ts_engine.rs:337–342`). With auto-echo on,
the following interleaving occurs:

1. T1: `fetch_add` → id 0, then T1 is preempted before `send`.
2. T2: `fetch_add` → id 1. It sends `(1, LoadEngine)` at **index 0**, gets
   the auto-echo reply, sets discovery, takes the `instance` mutex, and
   sends `(2, LaunchEngine)` at **index 1**.
3. T1 resumes and sends `(0, LoadEngine)` at index 2.
4. The watcher sees LaunchEngine at index 1 and calls
   `deliver_late(1, Launched)`. Id 1's slot has already been consumed, so
   the reader drops the response silently. The watcher still counts it as
   delivered and **breaks**.
5. T2 waits for id 2 until it times out at 10 s. T1 then relaunches and
   waits again, and the watchdog fires at 15 s.

The preemption window in step 1 is small, but it covers a `Mutex` lock
(`pending.insert`), so it is a realistic window on an oversubscribed
runner. Cause B needs no slow thread start at all, only one unlucky
preemption.

Both causes have the same fix: answer the **recorded request id** and keep
watching until the real LaunchEngine has been answered.

### Sibling tests with the same pattern

| Test | Pattern | Risk |
|---|---|---|
| `test_race_free_discovery_convergence` (:1552) | 300×5 ms poll budget; uses delivery count as id; **breaks after the first delivery** | Real flake risk. If one poll falls between the two LoadEngine sends, only id 0 gets a reply. Id 1 then times out at 10 s, `name2 = None`, and the `name1 == name2` assertion fails. Ids 0 and 1 are both LoadEngine here, so using the count as the id happens to be correct. |
| `test_poison_policy_timeout_poisons` (:1644) | 200×10 ms budget; uses `sent.len()-1` as id; calls `deliver_late(idx+1, ExecuteResult)` **before** Execute is sent | The ExecuteResult is always dropped (no slot yet), so the second execute always waits out its 5 s timeout. The test only asserts the relaunch count, so it passes, but it takes about 5 s longer than it should, and a missed launch can push it toward the watchdog. |
| `test_poison_policy_cancel_poisons` (:1772) | Same as above | Same as above |

The strand only covers the first test. The fix should go into the shared
mock helper so all four tests use it.

## Reproduction

The CI failure was not reproduced locally (see the investigation notes
under `claude-notes/plans/ts-engine-race-test-flake-investigation/`). Cause
B can only be triggered deterministically by adding a delay between
`fetch_add` and `send`, which would mean changing production code. The
proposed fix removes both causes structurally, so a deterministic repro is
not required. A stress run before and after the fix (Phase 3) is the
practical check.

## Decisions (2026-10-22, with the user)

1. **Mock API: option (b).** Predicate-keyed auto-responses with an
   optional delay replace the polling watcher threads.
2. **Scope: all four tests.** `test_race_free_instance_exclusive`,
   `test_race_free_discovery_convergence`,
   `test_poison_policy_timeout_poisons` and
   `test_poison_policy_cancel_poisons` all move to the new API in this
   strand.
3. **Make the poison tests stronger.** The relaunch `execute` must
   actually receive its `ExecuteResult` and assert `Ok`, instead of
   timing out after 5 s.
4. **Leave `sent_messages()` unchanged.** Add an id-carrying accessor
   (`sent_requests() -> Vec<(u64, ToEngine)>`) next to it, for assertions
   that need ids.

## Phases

- **Phase 1 — Mock API** (`ts_process.rs` §`mod mock`).
  - Add `auto_respond(pred, responses, delay)` to `MockWriteHalf`. It holds
    a list of `(predicate, queue of FromEngine, Option<Duration>)`
    responders. In `send()`, after recording the frame, the first
    responder whose predicate matches pops its next response and answers
    under the frame's **real id**. The delayed path reuses the
    `script_response_delayed` thread. A queue rather than a single
    response is needed because the convergence test answers its first
    and second LoadEngine with different payloads (v1, v2). When the
    queue holds one response, it repeats.
  - Decide whether `enable_auto_echo` becomes a thin wrapper over
    `auto_respond`. Its response depends on the message's `engine_path`,
    so it would need a response *function* rather than a value. Either
    form is fine; pick whichever reads better in the code.
  - Add `sent_requests()`.
  - Unit-test the responder: it matches by predicate, answers with the
    real id, applies the delay, and yields responses from the queue in
    order.
- **Phase 2 — Rewrite the four tests.**
  - `test_race_free_instance_exclusive`: delete the watcher and call
    `auto_respond(LaunchEngine, [launched], Some(~50ms))`. The delay keeps
    the named-revert sensitivity.
  - `test_race_free_discovery_convergence`: delete the deliver thread and
    call `auto_respond(LoadEngine, [v1, v2], Some(small))`.
  - Both poison tests: once the first `execute` has failed (Timeout or
    Cancelled), register auto-responders for LaunchEngine and Execute
    *before* the relaunch `execute`. Then assert `Ok` with markdown
    `"# done"` and keep the existing launch-count assertion. Remove the
    `deliver_relaunch` thread.
  - Check the named-revert property by hand once: temporarily revert
    `ensure_launched` to a naive get/launch/set and confirm
    `test_race_free_instance_exclusive` fails, ideally in most of ~20
    runs.
- **Phase 3 — Stress check and verify.** Loop the four tests under CPU
  load, then run `cargo xtask verify --skip-hub-build`.

## Risks / tradeoffs

- An instant auto-response (option b without a delay) shrinks the race
  window that the named-revert check depends on, so a naive implementation
  might pass by luck. Use a delay of about 50 ms on the LaunchEngine
  auto-response.
- The change is test-only (`#[cfg(test)] mod mock`), so it has no
  production risk.
