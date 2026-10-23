# Investigation notes: bd-cig41dcs

## Code anchors (as of main @ 8ddbf8b17)

- `crates/quarto-core/src/engine/ts_engine.rs:1357`: `watchdog` helper (an mpsc `recv_timeout`)
- `crates/quarto-core/src/engine/ts_engine.rs:1462`: `test_race_free_instance_exclusive`
- `crates/quarto-core/src/engine/ts_engine.rs:494`: `ensure_launched` holds the `instance` mutex across `launch_engine`
- `crates/quarto-core/src/engine/ts_engine.rs:337-347`: documented double-send of LoadEngine under concurrency
- `crates/quarto-core/src/engine/ts_process.rs:756`: `DISCOVERY_WINDOW = 10s` (LaunchEngine window)
- `crates/quarto-core/src/engine/ts_process.rs:1176`: id allocated (`fetch_add`) *before* `write.send` pushes to `sent`
- `crates/quarto-core/src/engine/ts_process.rs:1475`: reader drops unknown ids silently
- `crates/quarto-core/src/engine/ts_process.rs:1906`: `sent_messages()` strips ids

## Timeline arithmetic for the observed 15.010 s

The watcher misses the delivery. T_a holds the instance mutex and waits
10 s for LaunchEngine, then gets `Err(Timeout)`. T_b takes the mutex and
sends a second LaunchEngine, which waits another 10 s. The total is ≥ 20 s,
more than the 15 s watchdog, so the test panics at 15.0 s.

## Local stress runs (macOS, 18 cores, 36 `yes` CPU hogs)

The set covered was `race_free` + `poison_policy` (+ `mock_auto_respond`
after the fix), with 40 back-to-back nextest runs each:

| Version | Failed runs |
|---|---|
| Old tests (HEAD `8ddbf8b17`) | 0/40 |
| Rewritten tests | 0/40 |

The CI flake did **not** reproduce locally, even on the old code. This is
consistent with cause B depending on a rare preemption between id
allocation and `send()`. The local run therefore shows only that the new
tests are not themselves flaky under load. The case for the fix rests on
the code analysis: no test infers ids from `sent` order anymore, and none
has a poll budget.

## Named-revert check (Phase 2)

With `ensure_launched` temporarily reverted to a naive get/launch/set
(instance lock released around `launch_engine`),
`test_race_free_instance_exclusive` failed **20/20** runs with
`exactly 1 LaunchEngine expected`. The sent log in the failure showed
**two LoadEngine** messages before the two LaunchEngines. This confirms
that the concurrent LoadEngine double send (the precondition for cause B)
happens routinely, not just in theory.
