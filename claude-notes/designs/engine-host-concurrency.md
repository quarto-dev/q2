# Engine-host concurrency: async multiplexing over one Deno subprocess

**Status:** canonical reference for the TS-engine subprocess concurrency model.
Pointed at by plan1a-protocol (Phase 1.5), plan1a-host, plan1a-engine, and
Plan 1b.

**Why this exists.** The original Plan 1a design assumed a **lockstep**
request/response protocol over stdio: one request, one response, no
correlation, serialized by a single `Mutex<transport>`, with whole-subprocess
SIGKILL on timeout/cancel. That was correct while **Pass-2 (per-file render)
was serial**. Pass-2 is now **parallel** (rayon + `pollster`-per-worker), and
all workers share **one Deno subprocess per project render**. Under the lockstep
model that means (a) every engine round-trip serializes through one lock, and
(b) one document's timeout/cancel SIGKILLs the subprocess out from under
siblings still mid-`execute`. This note defines the replacement.

## The key realization

**Deno is single-threaded but asynchronous.** One process can hold many
requests in flight at once — engine A's `execute()` parked on a Julia daemon
socket while engine B's is parked on a Jupyter kernel — with the event loop
interleaving them. Nothing about "one subprocess" forces serialization; only
our **framing** did (**one-in-flight, no correlation** — independent of which
OS channel carries it). The fix is to reframe the one process as an **async
multiplexed RPC channel** over a dedicated transport — originally the
*existing* stdin/stdout (v1), and since Plan 1a.6 a private loopback-TCP
socket (see "Phase 1.6" below) — not to spawn N processes (which wastes
exactly that async capability and N×s the memory).

## Architecture (three layers)

### 1. Protocol (plan1a-protocol Phase 1.5)

- **Correlation envelope.** Every frame carries a monotonic `id: u64`; the
  response echoes it. Nested envelope (`{ id, msg: {type, …} }`), not
  `serde(flatten)` — flatten round-trips poorly with internally-tagged enums.
  The existing `ToEngine`/`FromEngine` enums are unchanged.
- **Cooperative cancel.** `ToEngine::Cancel { target: u64 }` (fire-and-forget,
  references an in-flight id) and `FromEngine::Cancelled {}` (delivered under
  the target id).
- **Channel: stdio in the original v1 design; loopback TCP since Plan 1a.6.**
  The envelope + `Cancel` are all that parallel Pass-2 *requires* — they
  multiplex fine over a JSON-lines channel (the harness write-serializes its
  frames; single-threaded Deno writes each line atomically, so frames never
  interleave), independent of which OS channel carries it. In the original v1
  design, stdout was the protocol channel, so the "stdout is protocol; a stray
  `console.log`/non-JSON line is malformed → kill" contract held at the time
  (owned by plan1a-host), and it was two-sided: stdin was the protocol *input*,
  so an engine reading `Deno.stdin` stole frames just as writing to stdout
  corrupted output. **That contract is gone.** Plan 1a.6 has since landed (see
  "Phase 1.6" below and
  `claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`): the
  protocol now rides a private loopback-TCP socket, and stdout/stderr are
  diagnostic-only — a stray `console.log` no longer corrupts anything. The
  bidirectional **continuous drain** (q2's demux reader thread + the harness's
  non-blocking loop) is still what keeps a large `Execute` payload from
  deadlocking the channel — load-bearing regardless of transport, not
  optional. The `EngineTransport` trait already abstracted the channel, which
  is why the swap to TCP was localized.

### 2. Rust host — a demux, not a Mutex (plan1a-host)

Standard async-RPC-over-a-pipe, composed entirely from **blocking std
primitives** so it fits blocking rayon+`pollster` workers:

- `TsEngineHost` owns a **write half behind a short-lived mutex** (held only for
  the microseconds of one framed write) and a **`pending: Mutex<HashMap<u64,
  Slot>>`**.
- One **reader thread** owns the read half (the accepted loopback-TCP socket's
  read half since Plan 1a.6; the child's stdout in the original v1 design), parses
  each `Response`, reads its `id`, and delivers to `pending.remove(id)`. A
  response whose `id` is no longer pending (late reply after a cancel) is
  dropped.
- A worker calls `host.request(msg, window, &cancellation)`: allocate `id` →
  register a blocking `Slot` (e.g. `sync_channel(1)`) → write the framed line
  under the write-mutex → block on the slot with `recv_timeout`, polling
  `is_cancelled()` on each tick. On timeout/cancel it sends `Cancel { target:
  id }` and resolves the slot with a cancelled/timeout error.

**The crucial property: no lock is held across the wait.** Worker A blocking on
a 5-minute `Execute` does not block worker B's discovery call — they hold
different slots; the write-mutex and pending-mutex are each held for
microseconds. Head-of-line blocking is gone.

### 3. Deno harness — a non-blocking read loop (Plan 1b)

```ts
for await (const frame of readFrames(conn)) {   // originally Deno.stdin (v1); now the loopback-TCP conn (Plan 1a.6)
  if (frame.msg.type === "cancel") { abort(frame.msg.target); continue; }
  dispatch(frame);   // fire-and-forget; the loop does NOT await engine work
}
```

`dispatch` runs the request **serialized per engine instance** (chained on a
per-engine promise queue) and, when done, writes `{ id, msg: result }` to the
protocol channel (the loopback-TCP connection since Plan 1a.6; the captured
`Deno.stdout` in the original v1 design) under a write-mutex. Because
the read loop never awaits engine work, requests to *different* engines run
concurrently on the event loop; the per-engine queue serializes requests to the
*same* instance. An `AbortController` per `id` implements `Cancel`.

## The concurrency ceiling (and why it's physics, not a compromise)

- **Document pipeline** (parse → merge → transforms → write): fully parallel
  across workers, always — it never touches the subprocess.
- **Cross-engine execution:** parallel (distinct daemons, interleaved on the
  Deno event loop).
- **Same-engine-instance execution:** serial — and this is **forced by the
  daemon, not chosen.** There is one `julia` instance per render talking to one
  Julia daemon (single fixed `julia_transport.txt` per runtime dir); Jupyter
  shares a kernel keyed by (kernelspec, target). Two concurrent Julia documents
  *cannot* truly parallelize their execute — they'd be two render requests to
  one kernel. Serializing same-engine costs nothing we could have had anyway.

Escape hatch if same-engine parallelism ever matters: instance-per-worker (N
kernels). Not built now — for Julia it wouldn't even help (single shared
daemon).

## Cancellation / timeout — contained blast radius

A per-document timeout/cancel **must not SIGKILL the shared subprocess** (that
murders sibling documents). So:

1. Per-request timeout/cancel → send `Cancel { target: id }` → harness aborts
   *that task's* `AbortSignal`. Contained to one request.
2. **Poison policy — scoped by request type, not by guessing daemon state.**
   Rust cannot distinguish "ambiguous" from "clean" (the daemon is a separate
   process; aborting the JS-side `AbortSignal` never proves it went idle). So
   don't classify — scope by which request was interrupted. **`Execute` is the
   only daemon-engaging request v1 issues** (this is a v1 fact, not an absolute:
   Q1's `dependencies()`/`run()` also spawn subprocesses — knitr's `Rscript`,
   `rmd.ts:407-489` — so the poison scope must be revisited if/when those
   requests are added), so cancel/timeout of an `Execute` **always poisons that
   engine instance**; every other request engages no daemon and is just failed. Poison = invalidate the instance on both sides (harness drops
   its `instance` entry; `TsEngine` clears its cached launched-state — which is
   why that cache is a clearable `Mutex<Option<…>>`, not a `OnceLock`), so the
   next instance request re-runs `LaunchEngine` (~0) and gets a fresh
   `ExecutionEngineInstance` re-discovering/restarting the detached daemon.
   Blast radius shrinks from "whole subprocess" to "one engine instance." *(Future
   opt-in: an engine that performs a real interrupt may carry `clean: true` on
   `Cancelled` to skip the poison — not v1.)*
3. **Concurrent same-instance requests transparently re-launch — they are not
   failed.** Parallel Pass-2 means the poisoned instance can have a *concurrent*
   user, not just a future one: worker A's `Execute` times out and poisons the
   julia instance while worker B has an `Execute` queued behind it on the
   harness's per-engine queue (same instance ⇒ serialized). When the harness
   dequeues B and finds the instance entry dropped, it **re-runs
   `engine.launch(stashedContext)` to reconstruct it, then runs B** — B never
   sees a half-torn-down instance and never fails for A's timeout. (Composes
   with idempotency: a *present* instance makes `LaunchEngine` a no-op; a
   *missing* one triggers exactly one lazy re-construct, whether the trigger is
   B's queued request or a fresh `LaunchEngine` from q2.) Self-healing: if the
   detached daemon is genuinely wedged by A's aborted-but-still-running work,
   B's re-run hits **its own** `window` and poisons again on its own merits —
   no special-casing needed. This is plan1b's harness contract; q2 is unaware.
4. **Whole-subprocess SIGKILL is reserved** for what genuinely affects everyone:
   subprocess crash, a compromised/unparseable control channel, and final
   teardown. **This is safe only because execution daemons are spawned truly
   detached** (own process group / `Deno.Command … detached`; Q1: julia control
   server, `julia-engine.ts:377`), so SIGKILL of the subprocess never cascades to
   the daemon. A harness that spawns a daemon *inside* the subprocess's process
   group breaks this **silently** — lost daemon warmth, a perf regression that
   won't surface in tests. Spawning execution daemons detached is therefore a
   **plan1b harness contract.**

**Two invariants this poison/relaunch design rests on.**

- **The cached launched-instance is stateless.** It holds no
  kernel/socket/dependency state; all durable execution state lives in the
  detached, transport-file-keyed daemon (model §4.2). That is *why* poison can
  drop the instance and re-`LaunchEngine` cheaply, and why the cache is a
  clearable `Mutex<Option<…>>` rather than a `OnceLock`. If a cached instance
  ever starts holding durable state, this cache (and the poison design) must be
  revisited.
- **q2 never touches engine transport files.** Survive-respawn — a re-launched
  instance reconnecting to its still-running daemon — is delegated wholly to the
  engine via its on-disk transport file (Q1: `jupyter-kernel.ts:305-324`,
  `julia-engine.ts:962-964`). q2 never reads, writes, deletes, or keys on those
  files; it owns only the subprocess lifecycle. This hands-off contract is what
  makes the cheap drop-and-relaunch above correct.

This reverses the original plan's "no cooperative cancel; SIGKILL is the honest
path" stance. That reasoning was sound only because SIGKILL was assumed to
coincide with "q2 is exiting anyway" — which parallel Pass-2 breaks. The
daemon-ambiguity objection is answered by scoping it to one poisoned instance
rather than letting it justify killing the world.

## Why Rust stays synchronous despite an async harness

The concurrency lives on the **Deno event loop**, surfaced to Rust through the
**reader-thread demux**, not through async Rust. Rust workers are blocking
rayon+`pollster` threads; each blocks on its own slot. There is no tokio on the
Rust side, no `block_on` in the pipeline, no reactor to drive. So the
`EngineTransport` trait and `TsEngine`'s calls remain **synchronous** — the
earlier "the Rust transport is sync" conclusion survives; only the "because the
protocol is lockstep / async buys no concurrency" *justification* is retired.

`EngineTransport` splits into a write half (shared, internally mutexed) and a
read half (owned by the reader thread). The original v1 impl was
`StdioTransport` (the child's stdin/stdout); Plan 1a.6 replaced it with
`TcpTransport` (the loopback-TCP socket) as the sole transport —
`StdioTransport` has been deleted from the code. The future
`WebSocketTransport` (WASM) shares the same trait seam.

## Phase 1.6 — the protocol moved off stdout (loopback TCP)

> **Landed (plan dated 2026-07-08):**
> `claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md` (Plan 1a.6)
> moved the protocol off stdout onto a private loopback-TCP socket. This
> section remains the canonical design rationale; the plan file carries the
> checklist, tests, and engine-impact analysis.

Multiplexing (above) did **not** require leaving stdout; it was orthogonal.
The *reason* to leave stdout was to delete the `console.log` footgun (an
engine writing to stdout corrupted the protocol). That cleanup was **Phase
1.6**, and it has now landed.

The channel is **loopback TCP**, not a Unix-domain socket + Windows named
pipe:

- `std::net::TcpListener`/`TcpStream` is **blocking std, uniformly
  cross-platform, no new dependency and no `#[cfg]` fork** — it actually
  satisfies the "blocking std primitives" property that UDS + named-pipe
  *cannot* (std has `UnixListener` but **no** Windows named-pipe support; that
  needs the `interprocess` crate or raw winapi). The earlier "Unix socket /
  named pipe" framing in these notes was wrong on that point.
- It **matches the existing local-IPC precedent** in the tree — Jupyter kernels
  already talk over localhost TCP via `runtimelib` (`daemon.rs`).
- q2 binds `127.0.0.1:0` (ephemeral) and passes the non-secret address via
  argv (`--control 127.0.0.1:<port>`); the one-time token is written as the
  first line on the child's stdin (not env/arg — see Plan 1a.6's "Token
  delivery" section for why argv/env were rejected), and the child's stdin is
  closed after that line. The harness dials back and presents the token as a
  pre-line on the socket before any framed message. The token (not filesystem
  perms) closes the "any local process can connect" gap — and the Deno
  subprocess is already fully-trusted/local.

Phase 1.6 also flipped stdout/stderr to **diagnostic-only** (drained by
`stdout_loop`/`stderr_loop` and forwarded to tracing) and deleted the
"console.log corrupts the protocol" contract. That contract no longer holds:
a stray `console.log` (or anything an engine writes to stdout) is harmless.

## Cross-cutting consequences

- **WASM gating.** `ts_protocol.rs` (pure serde) compiles for `wasm32`, but
  `ts_process.rs` (host + demux + `TcpTransport`) uses
  `std::thread`/`std::process`/`std::net` and must be entirely
  `#[cfg(not(target_arch = "wasm32"))]`; `TsEngine` is registered native-only.
  Skipping this breaks the `wasm-quarto-hub-client` build while
  `cargo build --workspace` still passes (the `cargo xtask verify` trap). The
  thread-based **demux is native-specific** — a future `WebSocketTransport`
  multiplexes on the JS event loop; only the *trait* is the shared seam.
- **No new socket dependency.** The original v1 stdio transport had no
  dependency; Plan 1a.6's loopback-TCP transport uses `std::net` (already
  available; no `interprocess`, no winapi, no `#[cfg]` fork), so this remains
  true post-landing.
- **Timeout includes same-engine queue wait.** A request's `recv_timeout(window)`
  ticks while it waits in the harness's per-engine queue behind another
  document's `Execute`, so under same-engine contention the Execute timeout
  measures "queue-wait + execution." Accepted for v1; the fix (if needed) is a
  harness `started`-ack that resets the Rust timer at actual execution start.

## Layer ownership

| Concern | Owner |
|---|---|
| Envelope (`id`), `Cancel`/`Cancelled`, off-stdout channel contract | plan1a-protocol (Phase 1.5) |
| `TcpTransport` (loopback listen/handshake, landed via Phase 1.6; `StdioTransport` deleted), demux (writer-mutex + reader thread + pending map), `TsEngineHost::request`, poison/relaunch, `MockTransport` | plan1a-host |
| `TsEngine` issuing requests/`Cancel`; `ExecutionContext.cancellation` (consumes `MockTransport`) | plan1a-engine |
| Non-blocking read loop, per-engine serialization queue, `AbortController` dispatch, instance lifecycle | Plan 1b (Deno harness) |
