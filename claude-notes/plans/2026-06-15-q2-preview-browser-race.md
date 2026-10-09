---
title: 'q2 preview: browser opens before the server accepts connections'
date: 2026-06-15
---

**Strand:** bd-a6dvrdg1 (bug, p2) — related to the q2 preview epic bd-kw93
**Reported:** 2026-06-15 by Carlos (two Firefox screenshots: "Unable to connect"
on first open, correct render after a manual reload)
**Status:** diagnosed; awaiting go-ahead to implement.

## Overview

`q2 preview` auto-opens the user's browser at `http://<host>:<port>/?page=<initial>`
*before* the preview HTTP server is actually accepting connections. On small or
single-file projects the server comes up fast enough that the browser's first
request lands after the listener is accepting, so it "just works." On larger
projects the server's startup work (samod init + project discovery + index load
+ initial filesystem sync) takes long enough that the browser loses the race and
Firefox shows **"Unable to connect to 127.0.0.1:&lt;port&gt;"**. Hitting reload works
because by then the server is up.

This is a genuine ordering bug, not flakiness: the browser open is *unconditionally
sequenced before* the call that starts the server.

## Diagnosis (verified against the code)

### The ordering

`crates/quarto/src/commands/preview.rs`, in `async fn run(...)`:

| Line | What happens |
| ---- | ------------ |
| 113-116 | Resolve the port via `probe_free_port` (or validate an explicit `--port`). |
| 135 | `build_boot_url(&host, port, initial_page)` — compose the URL. |
| 137-140 | `println!` the URL for copy-paste. |
| **145** | **`open_browser_or_log(&url, args.no_browser)` — opens the browser NOW (synchronously).** |
| 147-188 | Resolve engine policy, resources, build `PreviewConfig`. |
| **189** | **`quarto_preview::run(config).await` — this is what actually starts the server.** |

So the browser is launched at line 145, and the server doesn't even *begin* its
startup until the `.await` at line 189.

### Why the printed port is not yet reachable

`probe_free_port` (`preview.rs:194-202`) binds a throwaway `StdTcpListener` on
`(host, 0)`, reads the OS-assigned port, and **immediately drops the listener**:

```rust
fn probe_free_port(host: &str) -> Result<u16> {
    let listener = StdTcpListener::bind((host, 0))...?;
    let port = listener.local_addr()?.port();
    drop(listener);          // <-- nothing is listening on `port` after this
    Ok(port)
}
```

The design comment at lines 103-109 explains the intent: probe up front so the
URL can be printed *before* the long-running server starts. That part is fine.
The defect is that the **browser open** was placed on the same "before the server
starts" side of the boundary as the URL print.

### Where the server actually becomes reachable

`quarto_preview::run` → `run_with_on_ready` → `quarto_hub::server::run_server_with`
(`crates/quarto-hub/src/server.rs`, \~1161-1304):

1. `~1181` `HubContext::new(storage, config).await` — **the expensive, size-dependent
   step**: samod repo init, project discovery (directory walk), index load, initial
   sync. This is what grows with project size.
2. `~1183-1188` the `on_ready` callback fires (spawns the eager-capture driver).
   **Note:** this is *after context init* but *before bind* — see the caveat below.
3. `~1198-1202` build/extend the axum router.
4. `~1204` `TcpListener::bind(&addr).await` — listener created.
5. `~1273` `axum::serve(listener, router).await` — **connections are accepted from
   here on.** This is the true readiness point.

The window between the browser open (step at preview.rs:145) and step 5 above is
the race. It widens with `HubContext::new` cost, i.e. with project size — exactly
the reported symptom.

### Caveat on the existing `on_ready` seam

There is already an `on_ready: OnReadyCallback` plumbed through
`run_with_on_ready` (`crates/quarto-preview/src/lib.rs:143-228`). It is tempting to
reuse it to signal "open the browser now," but **it fires before `TcpListener::bind`**
(step 2 above, vs. bind at step 4 and accept at step 5). Gating the browser on the
current `on_ready` would shrink the race but not close it. A correct fix must key off
the listener actually accepting, not off context-ready.

## Fix options

### Option A — Poll the real readiness condition (recommended)

Keep the server on the main task (it is awaited directly today; spawning it onto the
multi-threaded runtime would impose a `Send` bound we deliberately avoid — see
`.claude/rules/wasm.md`). Instead, move only the lightweight *browser open* onto a
spawned task that first waits until the port is accepting:

```rust
// in preview.rs run(), replacing the line-145 call:
if !args.no_browser {
    let url = url.clone();
    let host = host.clone();
    tokio::spawn(async move {
        if !wait_until_accepting(&host, port, Duration::from_secs(10)).await {
            tracing::warn!(
                %host, port,
                "preview server took >10s to accept connections; opening browser anyway"
            );
        }
        open_browser_or_log(&url, false); // open::that is quick; blocking is fine here
    });
}
quarto_preview::run(config).await
```

`wait_until_accepting` loops `TcpStream::connect((host, port))` until it succeeds
(returns `true`) or the **total** timeout elapses (returns `false`). Backoff between
attempts starts tight and decays to a 1s cap:

- start \~20ms, grow \~1.6× per miss, **cap at 1s**;
- **10s total ceiling**; on ceiling, open anyway (degrade to today's behavior rather
  than never opening) **and `tracing::warn!`** so a pathologically slow start is
  visible instead of silent.

Rationale for these numbers comes from the benchmark below: the common case is
\~250ms, so the tight early interval opens the tab with no perceptible delay; the 1s
cap bounds post-ready latency on a slow project to ≤1s; the 10s ceiling + warn turns
"server eventually works but browser errored" into a logged, diagnosable event.

#### Benchmark: actual race window (`q2 preview docs/`, release)

Measured 2026-06-15 with `claude-notes/research/bench-preview-port-ready.py` — spawns
`q2 preview docs/ --no-browser`, stamps the moment the URL is printed (= when
`open::that` fires today, `preview.rs:145`), then polls `connect()` until the port
accepts. `docs/` has 167 `.qmd` files (a medium project).

| Trial | Port-ready after URL print |
| ----- | -------------------------- |
| 1 | 280.8 ms |
| 2 | 249.9 ms |
| 3 | 248.5 ms |
| 4 | 250.0 ms |
| 5 | 250.7 ms |

**min 248.5 / median 250.0 / max 280.8 ms (n=5).**

Takeaways:
- The race window is **\~250ms even on a medium project** — comfortably long enough for
  a launching/running browser to connect-and-fail, which is exactly the reported bug.
  This is the first quantification of the window.
- The tight clustering (variance &lt;35ms) suggests a **fixed-cost** startup component
  (samod repo init, or a debounce/sleep) dominates here rather than per-file work —
  reassuring for "even larger projects" (the floor isn't simply linear in file count),
  but **smells like a constant delay worth a separate look**. Filed as a follow-up, not
  folded into this fix (see Follow-ups).

Why this is the right shape:
- It observes the **actual** condition the browser cares about (TCP accept), not an
  internal server phase. The router is fully built before bind (step 3 before 4), so
  TCP-accept ⇒ all routes mounted ⇒ HTTP-ready.
- **Robust to future startup-order changes.** A keys off an externally-observable
  fact; B re-encodes the internal invariant "signal after bind, never from `on_ready`,"
  which a later refactor of `run_server_with` could silently violate and reintroduce
  the race with all tests green. A can't drift that way.
- **Separation of concerns.** Browser-opening is a CLI concern; A keeps it in
  `preview.rs` instead of threading a UI signal through the `quarto-hub` /
  `quarto-preview` libraries (or pushing browser logic down into `run`).
- No change to the server's `Send`-ness: the server still runs on the main task; only
  the poll+open runs on a spawned task.
- Failure-safe: a timeout fallback means we never silently fail to open.

These reasons, not "don't break a published API," are why A is preferred. We are the
sole consumers of these signatures, so B's blast radius is fully internal and small
(≈3 source files + 1–6 mechanical test edits — see below); on raw diff size A and B
are close. A wins on robustness and concern-separation, and B's only edge
(event-driven vs. a few sub-second localhost `connect()` polls) is negligible here.

Open question to settle: TCP-connect vs. an HTTP `GET <boot-url>` 200 check. TCP
connect is simpler and sufficient (accept implies the router is serving). An HTTP
probe is marginally stronger but pulls in a client dependency and handles redirects;
likely overkill. **Lean: TCP connect.**

### Option B — Add a "now accepting" signal from the server

Introduce a new `oneshot`/`watch` channel (or callback) that fires immediately after
`TcpListener::bind` succeeds inside `run_server_with`, thread it out through
`run_with_on_ready`, and await it in `preview.rs` before opening.

**Internal blast radius** (we are the sole consumers — nothing is published, so this
is the real cost, not "breaking an external API"):
- Core source: **3 files** — `quarto-hub/src/server.rs` (new signal param on
  `run_server_with` at 1161, send after bind at 1204, and `run_server` at 1105 passes
  `None`); `quarto-preview/src/lib.rs` (plumb the sender through `run`/`run_with_on_ready`);
  `quarto/src/commands/preview.rs` (create the channel, await it, open).
- Test ripple depends on *where* the new param lands. The signal has to reach the CLI
  through `run` → `run_with_on_ready` → `run_server_with`. `PreviewConfig` is built
  with full struct literals in the integration tests (no `..Default::default()`), so:
  - param on `run_with_on_ready`\'s signature **or** on `PreviewConfig` → breaks every
    builder: `config_endpoint.rs`, `diagnostics_capture_failure.rs`,
    `diagnostics_endpoint.rs`, `eager_capture.rs`, `staleness.rs`, plus `boot.rs` —
    **≈6 test files** of mechanical edits.
  - param confined to `run` only (leaving `run_with_on_ready`/`PreviewConfig` alone) →
    **just `boot.rs`** (1 file), at the cost of `run` owning browser-open logic.
- The new signal type inherits the same `Send + 'static` obligation as `OnReadyCallback`.

**Signal-placement subtlety (corrected).** The signal must be sent *after*
`TcpListener::bind` returns (`server.rs:1204`), **not** in the existing `on_ready`
callback — `on_ready` fires at `server.rs:1186`, *before* bind, so reusing it leaves
the exact connection-refused window we're fixing. Bind is the correct threshold (not,
as an earlier draft of this plan claimed, "after `axum::serve` is polled once"):
`tokio`\'s `TcpListener::bind` performs both `bind()` and `listen()`, so once it returns
the kernel completes TCP handshakes into the accept backlog on its own. A client that
connects between line 1204 and the `axum::serve` accept loop at line 1273 gets a
*successful* connection whose request just waits a few ms in the backlog — not a
"connection refused." So signaling right after bind is sufficient to avoid the error
screen.

Option A sidesteps the API churn entirely by probing the same externally-observable
condition (`TcpStream::connect` succeeds exactly once bind+listen has happened, i.e.
the line-1204 threshold). **Not recommended unless A proves insufficient.**

### Option C — Fixed sleep before opening

A `sleep(500ms)` between print and open. Rejected: it is the crude hack the project
guidance warns against — wrong for both very small projects (needless delay) and very
large ones (still races). Documented only to mark it considered-and-declined.

## Test plan (TDD — write first)

The bug is timing-dependent, but the *fix* is mechanically testable:

1. **Unit test for `wait_until_accepting`:**
   - Bind a `TcpListener` on an ephemeral port, then assert `wait_until_accepting`
     returns promptly (well under the timeout).
   - Pick a port with **nothing** listening and assert it returns only after the
     timeout (and that the elapsed time is ≥ timeout). Use a short timeout (e.g.
     200ms) to keep the test fast.
   - Bind a listener *after* a short delay on a spawned task, and assert
     `wait_until_accepting` blocks until the bind happens, then returns. This is the
     direct regression for the race.
2. **Integration test (preview surface)** — *folded into unit test #3 + e2e, by
   design.* The "late listener appears while we're already waiting" unit test is a
   faithful, fast proxy for "the gate precedes reachability": it exercises the exact
   readiness logic against a real `bind`-after-delay, without standing up a full hub
   server (samod, watcher, SPA) just to assert the same property more slowly. The real
   wiring through `run()` is covered by the end-to-end step below. Booting a server in
   a quarto-crate integration test would duplicate that coverage at much higher cost,
   so it is intentionally not added.
3. Confirm `--no-browser` still suppresses the open entirely: the spawned task is
   guarded by `if !args.no_browser`, so suppression skips the spawn altogether. The
   existing `open_browser_or_log_is_noop_when_suppressed` unit test covers the open
   helper; the e2e benchmark runs with `--no-browser` and confirms no browser opens.

## End-to-end verification (before declaring done)

Per CLAUDE.md's end-to-end policy, unit/integration green is necessary but not
sufficient. Before closing:

1. Run `cargo run --bin q2 -- preview docs/` (a large-enough project to have
   reproduced the failure) and confirm the browser tab loads the rendered site on the
   **first** open, with no "Unable to connect" and no manual reload.
2. Run it on a single-file project to confirm no regression / no added latency that a
   user would notice.
3. Record the exact invocation + observation in this plan and in the strand.

## Work items

- [x] Write `wait_until_accepting` unit tests (failing first). — 3 tests in
      `preview.rs` (`returns_true_when_listener_present`,
      `times_out_when_nothing_listening`, `unblocks_when_listener_appears_late`).
      Confirmed red first (E0425: function not found).
- [x] Implement `wait_until_accepting` (TCP connect + decaying backoff 20ms→1s cap +
      total-timeout fallback). Green: 3/3, timings match design (9ms fast path,
      \~161ms timeout, \~205ms late-bind unblock).
- [x] Move the browser open behind the readiness gate on a spawned task; keep
      `--no-browser` semantics (the spawn is guarded by `if !args.no_browser`). 10s
      ceiling + `tracing::warn!` on miss. Full preview unit set: 15/15.
- [x] Run `cargo nextest run --workspace` — **10043/10043 passed**, 197 skipped, 0
      failures.
- [x] `cargo xtask verify --skip-hub-build` — **All verification steps passed** (Rust
      build + workspace tests + ts-packages build/smoke; matches CI `-D warnings`).
- [x] End-to-end: `q2 preview docs/` first-open success — see evidence below.
- [x] Commit on `beads/bd-a6dvrdg1-preview-browser-race`. **Awaiting user review before
      pushing** (git push policy).

### End-to-end evidence (2026-06-15)

Harness: `claude-notes/research/e2e-preview-open-after-ready.py` (uses the **release**
binary). The `open` crate calls absolute `/usr/bin/open` on macOS (BROWSER ignored),
so PATH-shimming can't observe the open; instead the harness reads the binary's own
tracing logs and compares ordering, with an independent external accept-poll as a
cross-check. A new `info!("…opening browser")` in the spawned task — emitted right
before `open::that`, genuinely useful operationally — is the observable.

Invocation (inside the harness): `q2 preview docs/` with
`RUST_LOG=q2=info,quarto_hub=info` (note: the bin is named `q2`, so the crate's log
target is `q2::…`, not `quarto::…`; and `-v` alone only raises `quarto_hub`).

Observed log lines (inspected, not inferred):

```
2026-06-15T20:05:21.772704Z  INFO quarto_hub::server: Hub server listening (project mode) addr=127.0.0.1:54622
2026-06-15T20:05:21.834530Z  INFO q2::commands::preview: preview server accepting connections; opening browser host=127.0.0.1 port=54622
```

The browser-open log is **\~62ms after** the listener bound, and the harness's
independent `connect()` poll succeeded in the same window. A browser tab opened against
the live preview. **Result: PASS** — the open is gated on real readiness through the
actual CLI path. (Pre-fix, `open::that` fired at URL-print, \~250ms *before* this point —
see the benchmark above.)

## Follow-ups (out of scope for this fix)

- **Constant \~250ms preview startup floor.** The benchmark above shows `q2 preview docs/`
  takes \~250ms to accept connections with variance &lt;35ms across trials, despite 167
  `.qmd` files — consistent with a fixed-cost startup step (samod repo init, or a
  debounce/sleep) rather than per-file work. Worth profiling `HubContext::new`
  (`server.rs:1181`) to confirm where the floor comes from and whether it can be cut.
  This readiness-gate fix makes the symptom invisible to users; it does not reduce the
  startup time itself. File as a separate strand if confirmed.

## Key references

- `crates/quarto/src/commands/preview.rs:135-189` — the ordering bug and the
  `open_browser_or_log` / `probe_free_port` helpers.
- `crates/quarto-preview/src/lib.rs:129-284` — `run` / `run_with_on_ready`; the
  existing `on_ready` seam (fires before bind — see caveat).
- `crates/quarto-hub/src/server.rs:~1161-1304` — `run_server_with`; bind at \~1204,
  accept at \~1273.
