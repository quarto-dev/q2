---
title: 'Flaky fake_kernel two-names test exits during startup on ubuntu CI (bd-0w8rgjdf)'
date: 2026-11-13
description: 'The fake Jupyter kernel exits 0 with empty stderr when it loses a peeked port, because it discards TestKernel''s bind error, so the daemon''s port-conflict retry never fires; surface the error the way ipykernel does.'
status: implemented
braid:
  strand: bd-0w8rgjdf
---

**Branch:** `braid/bd-0w8rgjdf-fake-kernel-startup-flake` (topic branch in the main checkout, based on `main` @ `4f0c82a01`)

## Triage verdict

**Ready to design.** The root cause is identified with high confidence: the
fake kernel turns a lost-port bind error into a silent `exit 0`, which bypasses
the daemon's port-conflict retry (bd-e87d5tmn). A fix of a few lines in
`fake_kernel.rs` is available. The open questions concern scope only.

## Issue context

P3 bug, labels `engine`, `flaky`, filed 2026-11-11. CI has seen it twice on
ubuntu-latest and never on macOS:

- PR #1182 (run 43109976168):
  `ExitedDuringStartup { kernel: "fake-a", code: Some(0), stderr: "" }`.
- PR #1201 (run 43480871431, 2026-11-13):
  the same error for `kernel: "fake-b"`.

Neither PR touched kernel startup. Each failure cancelled about 10.6k tests
through nextest fail-fast.

## Dependency graph

- **discovered-from** bd-f63cqoke (closed): the Jupyter cell-fence-length fix
  (PR #1182). The relationship is incidental. The flake was only *observed* on
  that PR's CI, and there is no causal link.
- No `blocks` or `related` edges. Nothing depends on this strand, so the urgency
  is the CI noise itself (a full-suite cancel each time).
- The relevant prior art, which is not linked, is **bd-e87d5tmn**. That strand
  added the retry-on-port-conflict in `JupyterDaemon::start_kernel` and the
  `FAIL_FIRST_ENV` fake-kernel hook that tests it.

## What the code looks like today

### The exit-0 path

`fake_kernel_process_entry` → `run_kernel_process`
(`crates/quarto-core/src/engine/jupyter/fake_kernel.rs:229-257`):

```rust
let handle = TestKernel::start_from_file(connection_file, config_from_script(script))
    .await
    .expect("start fake kernel");
let _ = handle.await;          // <- JoinHandle<Result<()>>: the Err is discarded
```

`TestKernel::start` spawns `run`, so `start_from_file` only fails if reading
or parsing the connection file fails. Pinned runtimelib is
`cscheid/runtimed@fc58eb4`, `crates/runtimelib/src/test_kernel.rs:385-400`.
`run` binds the five sockets with `?`:

```rust
let mut heartbeat = create_kernel_heartbeat_connection(connection_info).await?;
let shell_connection = create_kernel_shell_connection(connection_info, &session_id).await?;
...
```

When one of those ports is already taken, `run` returns
`Err(Network Error: Address already in use (os error 98))` inside the
JoinHandle. The fake drops that error, and `fake_kernel_process_entry` returns
normally. libtest then reports the entry test as passing and exits **0**. The
result line goes to stdout because of `--nocapture`, so **stderr is empty**.
This is the exact signature in both CI failures. No other path in the fake
produces an exit code of 0 with nothing on stderr:

- A panic gives exit code 101 and output on stderr.
- `EXIT_ENV` gives 3.
- `FAIL_FIRST_ENV` gives 1.
- `(None, None)` scripts cannot happen here, because the test passes `env_all()`.

### Why the daemon doesn't retry

`JupyterDaemon::start_kernel` (`daemon.rs:300-319`) retries up to
`KERNEL_START_ATTEMPTS = 3` times when `JupyterError::is_port_conflict()` is
true (`error.rs:132`). That check matches only on **stderr**, looking for
markers such as `"Address already in use"`. Real ipykernel prints a `ZMQError`
and gets retried. The fake prints nothing, so the daemon classifies the failure
as a non-port-conflict and returns `ExitedDuringStartup` immediately.

### Why ubuntu, and why this test

Ports are only peeked (`runtimelib::peek_ports`) before spawning. The daemon's
own comment at `daemon.rs:288-297` says a peeked port can be taken before the
kernel binds it, by "a concurrently starting kernel's client sockets, whose
connect retries draw ephemeral ports from the same range, or by another
process."

- **Why this test:** `test_fake_kernel_two_names_one_binary` starts a second
  kernel while the first kernel's five client connections are live in the same
  process.
- **Why ubuntu:** on ubuntu CI, nextest runs many port-heavy tests in parallel
  on a small runner. The ephemeral range is also shared with the port-0 binds
  that `peek_ports` makes, so the collision window is real. macOS's ephemeral allocation pattern
  is different, and local machines are much less loaded.

All fake-kernel tests are exposed in principle. This test has twice the
startups plus a live sibling kernel.

### Repro

`claude-notes/plans/fake-kernel-startup-flake-investigation/repro-port-taken.sh`
holds the connection file's shell port in another process, then runs the fake
kernel entry directly.

Result on macOS at `4f0c82a01`:

```text
shell port 58819 held elsewhere -> kernel exit code: 0
--- stderr ---
--- stdout ---
running 1 test
test engine::jupyter::fake_kernel::tests::fake_kernel_process_entry ... ok
test result: ok. 1 passed; 0 failed; ...
```

This is the CI signature, reproduced deterministically: a lost port gives
`code: Some(0)` and `stderr: ""`.

## Proposed phases (draft)

- **Phase 0: test first.** Add a fake-kernel test that hands the kernel a
  connection file with a port that is already bound. It should assert that
  `acquire` retries and then succeeds, or that the error `is_port_conflict()`.
  Possible approaches:
  - Hold a listener on a port and pre-seed it. This needs a seam to influence
    which ports the daemon picks, which may not exist.
  - Unit-test the fake process directly, as the repro script does: spawn the
    entry with a pre-made connection file and assert a non-zero exit and an
    `Address already in use` message on stderr.
- **Phase 1: fix the fake.** In `run_kernel_process`, match on `handle.await`.
  On `Ok(Err(e))`, run `eprintln!("{e}")` and `std::process::exit(1)`. On
  `Err(join_error)`, treat it the same way. `Ok(Ok(()))` (shutdown) keeps
  exiting 0. The fake then behaves like ipykernel, and the bd-e87d5tmn retry
  covers it.
- **Phase 2 (optional): harden the daemon.** See question 2.
- **Phase 3: upstream (optional).** Consider making `TestKernel` log its own
  `run` errors in `cscheid/runtimed`. That fork is ours.

## Open design questions for the user

1. **Fake-only fix, or daemon too?** Phase 1 alone fixes the flake. Separately,
   should `start_kernel` treat *any* exit during startup with code 0 and empty
   stderr as suspicious, or retry it? A real kernel that exits 0 during startup
   is a different bug. My recommendation is a fake-only fix, so that a
   misbehaving real kernel is not masked.
2. **Regression test shape.** Is a test that runs the entry process directly
   against a pre-bound port (like the repro script) acceptable? Or do you want
   a daemon-level test that drives `acquire` into the collision? That would
   need a port-picking seam, which is more invasive.
3. **Upstream `TestKernel`.** Should runtimelib's `run` also `eprintln!` its own
   errors, or should the error be returned in a way `start_from_file` callers
   can't ignore? Or is fixing our caller enough?

## Risks / tradeoffs (draft)

- The fix does not *prevent* port collisions. It makes the existing retry (3
  attempts) cover the fake too. Three consecutive collisions remain
  theoretically possible but very unlikely.
- Confidence comes from code reading plus the deterministic local repro of
  the exit-0 behaviour. The CI collision itself has not been reproduced, and there are no
  CI logs with `tracing` output to confirm which port was lost.

## Pre-flight notes

`cargo xtask verify --skip-hub-build` at `4f0c82a01` ran 19970 tests, and
19969 passed. The one failure was
`jupyter_record_shadow::shadow_record_sizes_for_the_plan`, which uses a *real*
ipykernel and reported "timeout waiting for kernel to become ready after 300s".
The same test passes in isolation (4.6s). That failure is unrelated to this
strand and is filed as bd-7j80wlkp (related to bd-v0ddo4o4).

A separate question: bd-v0ddo4o4 covers real-kernel "shell connect timed out
after 30s" flakes, and it may share a root with this strand. A port-collision
mechanism on the *client* side would be one candidate. This investigation did
not check that.

## Decisions and outcome (2026-11-13)

The user answered the design questions:

1. Fake-only fix; the daemon is unchanged.
2. Smallest regression test.
3. No upstream runtimelib change.

What landed:

- `run_kernel_process` now matches on `handle.await`. On an error it prints
  `fake kernel failed: <e>` to stderr and exits 1.
- `test_fake_kernel_losing_a_port_reports_a_port_conflict` holds a port with
  a `TcpListener` and runs the fake-kernel entry on a connection file that
  names it. The test asserts a non-zero exit and that the result
  `is_port_conflict()`.

Verification:

- Before the fix, the new test failed with
  `ExitedDuringStartup { code: Some(0), stderr: "" }`, the CI signature.
- After the fix, all 12 fake_kernel tests pass, and the 199 jupyter tests in
  the quarto-core lib pass in each of 5 runs.
- `cargo clippy -p quarto-core --all-targets -D warnings` is clean on macOS.
- The Windows cross-check cannot run from macOS, because the `tree-sitter`
  and `aws-lc-sys` build scripts fail. The change uses only std/tokio, and
  Winsock's `WSAEADDRINUSE` text is already one of the `is_port_conflict`
  markers.
