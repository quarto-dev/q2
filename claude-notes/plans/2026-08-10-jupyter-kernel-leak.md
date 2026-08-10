# Jupyter kernel process leak (bd-hxhnnlzs)

**Strand:** bd-hxhnnlzs — "Jupyter kernels leak as orphan processes from test runs (2338 accumulated)"
**Status:** investigated 2026-08-10; root cause confirmed on both suspected paths. Fix not yet started.
**Investigation record:** braid comment `c-1jzpdrgf` on the strand (this document is the durable copy).

## Overview

Every process that executes a Jupyter document — the test suite *and* the
production `q2 render` CLI — leaks one ipykernel process per
`(kernel_name, working_dir)` pair. The kernel reparents to launchd
(PPID 1), idles forever holding ~6 listening TCP sockets, and leaves its
`kernel-<uuid>.json` connection file behind in `~/Library/Jupyter/runtime/`.
On 2026-08-10 a dev machine had accumulated 2338 orphans and 4932 stale
connection files.

### Root cause

The kernel daemon is a process-global static:

- `crates/quarto-core/src/engine/jupyter/daemon.rs:330` —
  `static DAEMON: OnceLock<Arc<JupyterDaemon>>` holds every
  `KernelSession` in a `RwLock<HashMap<…>>`.
- `daemon.rs:155` — kernel `Child` processes are spawned with
  `kill_on_drop(true)`, so the **only** automatic cleanup is the drop of
  the `Child` handle.
- Rust never drops statics at process exit → the `Child` is never
  dropped → `kill_on_drop` never fires → the kernel outlives the
  spawning process and reparents to PID 1.
- `KernelSession::Drop` (session.rs) is what deletes the connection
  file; since it never runs, the file leaks too.

Supporting facts:

- No non-test code calls `shutdown_all()` or `shutdown_session()`.
- `cleanup_idle_sessions()` (the 5-minute idle reaper) has **zero
  callers** anywhere in the tree — dead code. The
  `DEFAULT_IDLE_TIMEOUT` gives a false sense of a safety net.
- `KernelSession::shutdown()` never sends a Jupyter
  `shutdown_request`; it goes straight to `start_kill()` (SIGKILL).
  Acceptable as a backstop, but not a graceful shutdown.

### Empirical confirmation (2026-08-10)

Both leak paths were reproduced by diffing
`ps -axo pid,ppid,command | grep '[i]pykernel_launcher'` before/after:

1. **Test path.** One test:

   ```
   cargo nextest run -p quarto-core -E 'binary(integration) & test(plain_cell_error_fails_the_render)'
   ```

   → exactly 1 new PPID-1 orphan, 6 listening sockets (`lsof -i`),
   connection file intact (proving `Drop` never ran). nextest is
   process-per-test, so **each python-executing test leaks one
   kernel**. The observed ~15 orphans per `cargo nextest run
   --workspace` matches the python-executing tests that go through the
   render path with no shutdown: `engine_error_policy` (7) +
   `engine_output_parity` (7–8) + `capture_splice_engines` (2–3).
   `jupyter_integration.rs` tests call `shutdown_session` explicitly
   and only leak if an assert fails first. During the investigation a
   concurrent workspace run on the same machine independently leaked
   exactly 15 orphans, all with cwds in nextest temp dirs.

2. **Production path.** A minimal doc (`engine: jupyter`, one
   `{python}` cell) rendered with
   `cargo run --bin q2 -- render doc.qmd` → 1 orphan whose cwd (via
   `lsof -d cwd`) was the doc's directory. **Every `q2 render` of a
   jupyter document leaks a kernel.** This — not the test suite — is
   what explains 2338 accumulated orphans.

Attribution technique for future debugging: an orphan's cwd identifies
its spawner (nextest temp dir vs. document dir), and connection-file
mtime brackets the spawn time.

## Work Items

Phase 1 — regression test (TDD: must fail before the fix):

- [x] `crates/quarto-core/tests/integration/jupyter_kernel_cleanup.rs`
      — drives `record_capture`, watches an isolated
      `JUPYTER_RUNTIME_DIR` for `kernel-*.json`, asserts kernel ports
      are dead and files removed after the call returns. Verified
      failing pre-fix ("kernel still listening on shell port …").
- [x] `crates/quarto/tests/integration/jupyter_kernel_cleanup_e2e.rs`
      — drives the real `q2 render` on a two-python-doc project;
      additionally asserts exactly ONE kernel served both docs
      (pinning warm-kernel reuse). Verified failing pre-fix.

Phase 2 — fix the lifecycle:

- [x] Refcounted `KernelScope` (daemon.rs): inner scope in
      `execute_qmd` (no caller can leak), outer scopes in `q2 render`
      (around `pipeline.run()`, dropped before any `process::exit`),
      `q2 preview` and `q2 provide-hub` (server lifetime, sync side of
      `block_on` so the graceful path is available). Last scope out
      runs `shutdown_all_blocking()`.
- [x] Graceful shutdown: `KernelSession::shutdown_blocking` sends
      `shutdown_request` on the control channel (private
      current-thread runtime; skipped when already on a tokio runtime
      thread), waits bounded, then `start_kill()` backstop + reap +
      connection-file removal. Async `shutdown()` delegates to it.
- [x] Deleted the never-called idle-timeout machinery
      (`cleanup_idle_sessions`, `with_idle_timeout`,
      `DEFAULT_IDLE_TIMEOUT`, `last_used`/`touch`).
- [x] **Discovered + fixed: startup TOCTOU race** — concurrent renders
      sharing a session key both spawned kernels (read-check → long
      spawn → insert; loser evicted+killed). Every project render hit
      it. Fixed with a `start_lock` + re-check.
- [x] **Discovered + fixed: cross-call session reuse never worked** —
      sessions held ZeroMQ sockets/Child bound to a per-call
      throwaway runtime; genuine reuse failed with "Tokio context …
      being shutdown" (masked by the TOCTOU race always re-spawning).
      Sessions now live on a shared `ENGINE_RUNTIME`
      (`engine_runtime()` in daemon.rs); `execute_blocks_async` uses
      it instead of building a runtime per call.
- [x] Panic-safety scopes added to the direct-daemon-API tests in
      `jupyter_integration.rs`.
- [x] Full jupyter test set green (24/26; the 2 failures are
      pre-existing rot in `#[ignore]`d tests that nested-runtime-panic
      on main too — filed as bd-yaccefzk). Zero new orphans after the
      run (was ~15).
- [ ] `cargo nextest run --workspace` + `cargo xtask verify` green;
      zero new orphans across the full workspace run.

Phase 3 — hygiene (optional, decide with user):

- [ ] Consider a startup sweep for stale `kernel-*.json` files whose
      port owner is gone. (Not in the bd-hxhnnlzs PR.)

## Details / design notes

- **Why an exit hook and not per-render teardown everywhere:** kernel
  reuse across renders is the point of the daemon for `q2 preview`
  (warm kernel = fast re-render). For one-shot `q2 render`, teardown at
  end of invocation is correct. The fix likely wants both: an owned
  daemon handle whose scope ends with the CLI invocation, plus a
  signal/exit path for preview.
- **Why the tests leak even though some call `shutdown_session`:** the
  leaking tests never touch the daemon API — they render documents
  through the normal pipeline (`text_execute.rs:167` calls the global
  `daemon()`), so there is no handle for them to shut down. Fixing the
  production lifecycle fixes the tests for free.
- **`kill_on_drop(true)` is fine but insufficient:** it only helps if
  the `Child` drops while the process is alive. Keep it as a backstop
  for panic/error paths; do not rely on it for normal exit.
- Cleanup performed 2026-08-10: user killed all 2338 orphans and
  deleted stale connection files; the investigation session killed the
  2 orphans it created.
