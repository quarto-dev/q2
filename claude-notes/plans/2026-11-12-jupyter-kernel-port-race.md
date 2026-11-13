---
title: "Jupyter kernel startup: 'Address already in use' flake"
strand: bd-e87d5tmn
date: 2026-11-12
---

# Jupyter kernel startup: 'Address already in use' flake

**Strand:** bd-e87d5tmn (caused-by bd-i7yz08uh, PR #1173)

## Symptom

After PR #1173 (one Jupyter kernel per document) merged on 2026-11-11, CI on
`main` began to fail Jupyter end-to-end tests intermittently. The python
kernel dies during startup, and the render fails with:

```
Execution failed in jupyter: kernel 'python3' exited while starting (exit code Some(1)); its stderr:
...
zmq.error.ZMQError: Address already in use (addr='tcp://127.0.0.1:38829')
```

Two failures with this cause, both on `main` and both after the merge:

| Run | Test |
| --- | --- |
| 43118290398 (2026-11-11) | `jupyter_kernel_cleanup_e2e::render_leaves_no_kernel_behind` |
| 43235574747 (2026-11-12) | `jupyter_document_isolation_e2e::parallel_documents_do_not_interleave_on_a_kernel` |

None of the 13 failed runs before the merge that were checked show
`Address already in use`.

## Root cause

`JupyterDaemon::reserve_ports` (`crates/quarto-core/src/engine/jupyter/daemon.rs`)
gets five ports from `runtimelib::peek_ports`. That function binds port 0,
records each port number and closes the listeners. The daemon writes the
ports into the connection file and spawns the kernel, and the kernel binds
the ports only after it has started up. In between, the ports are free, and
any socket on the machine can take one, including an outgoing connection's
source port, which comes from the same ephemeral range. runtimelib's own
documentation for `peek_ports` names this TOCTOU race.

The race predates #1173, which made it much more likely in three ways:

1. **Startups within a process are concurrent.** Before #1173, one lock
   serialized kernel startup until the kernel was ready. Now each document
   starts its own kernel, and they start in parallel. The `reserved_ports`
   set added in #1173 stops two starting kernels from being *handed* the
   same port. It does nothing about our own ZMQ client sockets: while kernel
   A starts, its shell and iopub connects retry for up to 30 s, and every
   attempt draws a fresh ephemeral source port, which can be a port peeked
   for kernel B but not yet bound.
2. **Across processes, the reservation does nothing.** nextest runs tests in
   parallel processes, and #1173 added more real-kernel e2e tests. The
   failure in `render_leaves_no_kernel_behind`, a single-document render, is
   this case.
3. **There are more kernel starts.** One kernel per document instead of one
   per project means more starts, and every start can lose the race.

## Fix

Since the kernel must bind the ports itself, no reservation can close the
gap. The fix retries instead:

- `JupyterError::is_port_conflict()` (`error.rs`) recognizes an
  `ExitedDuringStartup` error whose stderr reports `EADDRINUSE`. It matches
  the POSIX `strerror` text (Linux and macOS), the MSVC CRT text
  (`zmq_strerror` on Windows), `EADDRINUSE` and Winsock's `WSAEADDRINUSE`
  text.
- `JupyterDaemon::start_kernel` now wraps the old body, which is renamed
  `start_kernel_once`. If an attempt fails with a port conflict, it starts
  the kernel again, up to `KERNEL_START_ATTEMPTS = 3` attempts. Each attempt
  drops its port reservation and peeks fresh ports. Any other failure is
  returned at once.

#1173 already provided what the retry relies on: a kernel that exits while
starting is reported immediately, with its stderr, instead of after the
connect's 30 s timeout. A retry therefore costs only the kernel's startup
time.

## Tests

In `fake_kernel.rs`, `QUARTO_FAKE_KERNEL_FAIL_FIRST=<n>:<path>` makes the
fake kernel count its spawns in `<path>` and exit with the ipykernel
port-conflict message on its first `n` spawns.

- `test_kernel_losing_a_port_at_startup_is_retried`: the first spawn fails,
  the second serves, and the lease works (2 spawns).
- `test_kernel_always_losing_a_port_fails_after_bounded_retries`: every
  spawn fails, and the start gives up after 3 spawns with a port-conflict
  error.
- `test_kernel_exiting_for_another_reason_is_not_retried`: any other startup
  exit causes exactly 1 spawn.
- `test_kernel_exiting_at_startup_reports_its_stderr` now uses a non-port
  error message, so it still checks the single-attempt path.
- `error::tests` cover the marker matching.

With `KERNEL_START_ATTEMPTS` set to 1, the first two tests fail, which
confirms that they exercise the retry.

## Not done (possible follow-ups)

- **IPC transport on Unix** would avoid TCP ports entirely. Windows still
  needs TCP, so the retry would still be needed there.
- **Keeping the peek listeners open until just before spawn** would narrow
  the window slightly but cannot close it.
