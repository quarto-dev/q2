# Plan 1a.6: Move the engine-host protocol off stdout → loopback TCP

> **Status:** ○ **Implementation-ready 2026-07-08** (0 impl; design ratified,
> Phase-0 spike PASS, fail-on-revert-bound Test Seam Spec frozen, and a
> blank-slate implementer review folded in). All design questions resolved from
> three code-research passes; the risky dial-back mechanics proved out end-to-end
> against real `deno`; every test is bound to a named revert hunk. The phase split
> is ordered so **each phase's HEAD is green**: build both transports (production
> stays stdio) → teach the harness to dial back → flip + validate → cutover.
> Additive cleanup over the completed 1a/1b transport; blocks nothing; pullable
> whenever the stdout footgun is judged worth retiring. Owned jointly by
> **plan1a-host** (Rust transport swap) and **Plan 1b** (Deno-side dial-back);
> **plan1a-protocol contributes nothing** — the wire *shapes* do not change.

## Provenance — why this plan exists

"Phase 1.6" is referenced ~50 times across the epic but was never a plan of
record. It is *defined* in exactly two prose design-notes, both verbatim
"Deferred:" notes rather than actionable phases:

- `claude-notes/designs/engine-host-concurrency.md:195` (canonical)
- `claude-notes/plans/2026-04-16-plan1a-host.md:1193`

The `1.x` numbering is the **`plan1a-protocol` phase line** — `Phase 1` (JSON
types, shipped) → `Phase 1.5` (concurrency multiplexing, shipped) → **1.6**.
The number is principled; the work simply never got promoted out of the notes.
This plan promotes it. It is filed as **1a.6** to signal "the 1.6 phase of the
1a series," matching the `plan1c2` naming precedent.

**The transport work lives in the 1a series, not the protocol plan:**
plan1a-host owns `EngineTransport`/`StdioTransport` and already anticipates the
concrete handshake (`--control <addr> --token <nonce>`, `plan1a-host.md:571`,
`:1448`). On the Deno side `connectControl` is **only a doc comment**
(`framing.ts:11`, `plan1b.md:1473-1477`) — there is **no stub**; it is written
from scratch (see "Deno side").

## Overview

**Goal:** stop carrying the multiplexed JSON `Request`/`Response` protocol on the
Deno subprocess's stdin/stdout. Instead, q2 binds an ephemeral loopback TCP
listener, passes the address + a one-time token to the subprocess, and the
subprocess dials back and presents the token as a **transport pre-line, consumed
before any frame** (never a protocol message — this is what preserves "no
protocol-type change"). After this, stdin/stdout/stderr are **diagnostic-only**,
and the entire "console.log corrupts the protocol" contract — plus its Rust-side
log-and-skip mitigation — is deleted (at the Phase-4 cutover; see Rollout).

**Motivating bug (the thing this actually fixes):** a child process spawned by an
engine (e.g. the Julia control server, or any `respectStreams` passthrough in
`deno-host.ts:219`) can leak non-JSON bytes onto the Deno host's stdout fd,
corrupting frame parsing. Today the Rust reader tolerates this only up to
`MAX_CONSECUTIVE_MALFORMED_LINES = 5` (`ts_process.rs:435`, `:1087-1164`) with a
documented **"structural" residual risk that a real response frame is skipped**,
then kills the whole subprocess. A dedicated channel makes the entire class
impossible.

**Non-goals:** no change to any protocol message shape; no change to the
multiplexing/demux model (Phase 1.5 is orthogonal and stays); no engine-author
API change (see "Effect on engines"); no pooling (that is Plan 5).

## Effect on engines — none required; strictly permissive

Engines **never touch the protocol channel** — the harness does. An engine
implements `ExecutionEngine` methods and returns values; the only code that wires
the **protocol channel** to Deno streams is `@quarto/engine-host-deno/src/main.ts`
(`runHost(Deno.stdin.readable, Deno.stdout, denoHost)`). (`main.ts`'s own header
comment claiming it is the sole `Deno.*` touch-point is stale — `deno-host.ts`
uses `Deno.*` extensively for the `PlatformHost`; it just never touches the
channel.) Under 1.6 **only the harness changes** — `main.ts` (channel selection)
and `framing.ts` (the new `connectControl`). **No engine recompiles or adapts.**

**Worked example — `~/src/quarto-julia-engine` (the validation engine):**

1. **Already speaks the 1.6 model internally.** The Julia control server returns
   a transport file `{ port, key, pid, … }` (`src/julia-engine.ts:490-497`) and
   the engine talks to it over **localhost TCP + a token key**
   (`getReadyServerConnection` / `writeJuliaCommand`). 1.6 simply makes the
   q2↔host hop match the host↔julia hop the engine already uses. Direct
   in-repo precedent, inside the validation engine itself.
2. **Diagnostics already avoid stdout.** ~20 `quarto.console.{info,warning,error}`
   sites; the harness routes them to **stderr** with level prefixes
   (`deno-host.ts:249-256`). Unchanged after 1.6 — but an accidental
   `console.log` / leaked banner stops being fatal.
3. **Its two direct `Deno.stdout.writeSync` calls become universally safe.**
   `logStatus()` (`:1062`) and `printJuliaServerLog()` (`:1083`) run only on the
   `q2 call engine julia status/log` path — Plan 9's **one-shot mode with
   inherited stdio**, where stdout *is* the terminal. Today they are safe only
   because they never run under the render host. After 1.6 stdout is free on both
   paths.
4. **Detached-server output already goes to a log file** (`juliaServerLogFile()`,
   `:443`) and the launcher is captured via `outputSync()`. The engine is
   careful; the leak comes from the shared-fd transport design, not engine
   sloppiness — which is why the fix belongs in the transport.

**Interaction with Plan 9 `call engine` (one-shot mode):** unaffected. The
one-shot `call-engine` process never uses the render-host transport — it keeps
inherited stdio for Q1 byte-parity. 1.6 only touches the shared render host.

**Bonus unlocked (not in scope here):** with stdin/stdout freed, a future
interactive `run()` mode could let engines own real stdin/stdout. Recorded, not
built.

## Design — loopback TCP

> **Design ratified 2026-07-08** from three code-research passes over the actual
> transport (`ts_process.rs`), harness (`engine-host-deno/src/*`), and in-repo
> TCP precedents (`daemon.rs`, `loopback.ts`, `preview.rs`). Facts below are
> grounded in that code, not assumed.
>
> **Phase-0 spike: PASS (2026-07-08).** A throwaway `rustc` host + `deno` child
> exercised the full bind→spawn→accept-poll→token→framed-round-trip path against
> `deno 2.9.0`, importing the **real** `readFrames`/`writeFrame` unmodified over a
> `Deno.Conn`. Both failure modes (child-death-before-dial, wrong-token) behaved
> as designed with no hang. Measured spawn→accept: **~75 ms cold, ~27 ms warm**.
> No design change forced. Two findings folded in below (the reader-handoff rule
> in step 5, and the generous deadline in step 4).

### Handshake (ratified)

The child is stored in `spawn_into` **before any thread exists**
(`ts_process.rs:352`), and `preview.rs:273-295` establishes that a bound
listener backlogs connections from `bind` onward — so binding before spawn has
no race window. Sequence, inside `ensure_started_inner`'s init closure so a
failure leaves `write` unset and the existing retry contract holds
(`ts_process.rs:636-637`):

1. **Bind** `std::net::TcpListener` on the `127.0.0.1` **literal** `:0`
   (ephemeral). Read the OS-assigned port from `local_addr()`.
2. **Generate** a one-time token — 128-bit+ CSPRNG (`getrandom`), hex-encoded.
3. **Spawn** `deno run --allow-all <bundle> --control 127.0.0.1:<port> --token
   <tok>` (arg-passed; `--allow-all` already grants net, no perm change).
4. **Accept with child-liveness polling** (the one pattern with no in-repo
   precedent — composed here): `listener.set_nonblocking(true)`, then loop
   `accept()` ⇄ `child.try_wait()` on `WouldBlock`, ~20 ms poll, against a
   **generous ~10 s deadline**. (Spike observed ~75 ms cold / ~27 ms warm, so
   10 s is ~130× headroom; keep it generous for a cold-cache or loaded CI box —
   tunable.) Child exits first → error carrying `recent_stderr` (likely a
   Deno/engine load failure); deadline with child alive → `child.kill()` +
   error; connection arrives → proceed.
5. **Validate token** — **build one `BufReader` over a `try_clone()` of the
   accepted stream, read the token line with `read_line`, then reuse that *same*
   `BufReader` for every subsequent frame.** This is load-bearing: the spike
   confirmed the child's eager first frame rides the same TCP segment as the
   token, so ~130 bytes sit in the `BufReader`'s buffer past the token's `\n` —
   a *fresh* reader would silently drop the first frame. So `TcpReadHalf` must be
   constructed *from the handshake `BufReader`*, never from the raw stream.
   **Bound the token read** (reject > `MAX_TOKEN_LINE` = 256 bytes before a
   newline) so a hostile local dialer can't force an unbounded read. Compare
   **constant-time**. Mismatch → drop connection + kill child + error.
6. **Commit** — **exactly one `try_clone()`** (the spike's shape): the
   `BufReader` from step 5 wraps the clone and becomes `TcpReadHalf`; the
   **original `stream`** becomes the write half (`TcpTransport`). Then **close
   the listener** (exactly one dial expected; a second is then impossible).
   Optionally assert the accepted peer is loopback.

**The handshake is a standalone, unit-testable function** — the init closure is a
thin caller, not the implementation (seam rows #2/#4 drive this function directly
with their own listener + child):

```rust
/// Accept exactly one loopback connection, validate the one-time token, and
/// return the split halves. Polls `child.try_wait()` while accepting so a child
/// that dies before dialing back fails fast instead of waiting out `deadline`.
fn accept_and_handshake(
    listener: TcpListener,
    child: &Arc<Mutex<Option<Child>>>,   // the slot `spawn_into` already populates
    token: &str,
    deadline: Duration,                  // injectable so tests use a short one
) -> Result<(Arc<TcpTransport>, TcpReadHalf), ExecutionError>;
```

**Token generation: `uuid::Uuid::new_v4().to_string()`** — 122 bits of CSPRNG
entropy, `uuid` is already a workspace dep that `quarto-core` already uses, and
it is precisely what the cited precedent `daemon.rs:111` uses for its per-session
key. **No new crate.** Constant-time compare is a ~5-line XOR-fold helper
(`ct_eq(&[u8], &[u8]) -> bool`), not a new dependency: it is **hygiene, not
load-bearing** (the listener accepts once, then closes), mirroring
`loopback.ts:169-179`. Its constant-timeness is deliberately **not** asserted by
any seam — row #2(b) only asserts that a wrong token is rejected.

The token (not filesystem perms) closes the "any local process can race the
ephemeral port between bind and the child's dial" gap; the child we spawned is
already fully-trusted and local. (A stronger-but-optional defense against the
race is to verify the peer against the child pid; the token alone is sufficient
for v1.)

### Rust side (plan1a-host)

- New `TcpTransport` (`EngineTransport`, `Send+Sync`; the write half, mirroring
  `StdioWriteHalf`) + `TcpReadHalf` (`EngineReadHalf`, owned). **Exactly one
  `try_clone()`**: `TcpReadHalf` wraps the handshake `BufReader` (over the clone)
  — *never* a fresh reader (byte-loss hazard); `TcpTransport` wraps the original
  `TcpStream`. **Framing is byte-identical**: newline-JSON, flush per frame;
  socket read-0 and empty line → `RecvError::Eof`; parse failure →
  `RecvError::Malformed`. The demux, `pending`/`sync_channel(1)` slots,
  `spawn_count` generation guard, timeout/`Cancel`, `recent_stderr`, and the
  field-clone thread-capture pattern are **all transport-agnostic and unchanged**
  (`ts_process.rs:1053-1229`). `MockTransport`/`with_transport` are unaffected.
- **Plumbing the new halves + the freed stdout (signature changes — do not skip):**
  - `spawn_into` (`:319-361`) today returns `(Arc<StdioWriteHalf>, StdioReadHalf,
    ChildStderr)` and consumes `ChildStdout` into the read half. Add a sibling
    `spawn_into_tcp(cmd, child_slot, listener, token, deadline)` returning
    `(Arc<TcpTransport>, TcpReadHalf, ChildStderr, ChildStdout)` — it spawns, then
    delegates to `accept_and_handshake`. On the TCP path `ChildStdout` comes back
    **free** and is handed to the drain thread.
  - `ensure_started_inner`'s `init` closure (`:566-575`) returns
    `(Arc<dyn EngineTransport>, Box<dyn EngineReadHalf>, Option<ChildStderr>)` —
    **grow it with `Option<ChildStdout>`** (`None` on the stdio path).
  - `TsEngineHost` has `reader` + `stderr_reader` join handles (`:404-406`) —
    **add `stdout_reader: Mutex<Option<JoinHandle<()>>>`** and join it at **both**
    sites: `shutdown()` (`:951-956`) and `Drop` (`:1038-1043`).
- **Freed stdout drain (`stdout_loop`) is TCP-only.** On the stdio path stdout
  *is* the protocol channel and is owned by the reader thread — spawning a drain
  there would steal frames. So: spawn `stdout_loop` **only when the transport is
  TCP**. It mirrors `stderr_loop` (`:1237-1267`), drains `ChildStdout`, logs via
  `tracing` at INFO; no demux, no crash-ring (stderr covers crashes).
- **Shutdown/Drop parity (must replicate):** `shutdown()` must make the child
  see EOF — half-close the write side (`TcpStream::shutdown(Shutdown::Write)`)
  so the peer gets EOF and exits, waking `reader_loop` under `shutting_down`.
  `Drop` still `child.kill()`s (`:1032`); a killed peer closes the socket →
  `TcpReadHalf::recv` returns `Eof`, so `reader.join()` is instant. **Handle the
  never-connected case** (accept failed: no socket to close, no reader/stdout
  thread to join — the child-kill path must not hang).
- **Malformed = fatal — but this is a Phase-4 change, not Phase 1.** The
  `MAX_CONSECUTIVE_MALFORMED_LINES` leniency (`:435-440,1087-1164`) lives in the
  **transport-agnostic** `reader_loop`, and `RecvError::Malformed` carries no
  transport tag — so making it fatal is necessarily a **global** switch that also
  changes stdio. Since the rollout keeps stdio alive (unselected) through Phase 3
  and explicitly forbids a per-transport malformed policy, the deletion lands at
  the **Phase-4 cutover**, together with `StdioTransport` and the two `#[cfg(test)]`
  tests that bind the leniency (`test_stray_lines_below_bound_are_skipped_not_fatal`
  `:2088`, `test_malformed_beyond_bound_escalates_distinct_from_crash` `:2174`).
  *Consequence, accepted:* during Phases 1–3 a malformed frame on the TCP socket is
  log-and-skipped rather than fatal. Harmless (nothing benign can write to a private
  socket) and Phase 4 makes it strict, eliminating the frame-skip residual (`:1111-1119`).
- **Dependencies: none new.** `std::net` (std), token via `uuid` (already a
  workspace dep, already used by `quarto-core`, and the `daemon.rs` precedent),
  constant-time compare via a local XOR-fold helper. No `#[cfg]` fork.

### Deno side (Plan 1b)

- **Write `connectControl(args)` from scratch** in `framing.ts` — it does **not
  exist today** (only a `Phase 1.6 (deferred)` doc comment at `framing.ts:11`).
  It parses `--control`/`--token` from `Deno.args`, `Deno.connect({ hostname:
  "127.0.0.1", port, transport: "tcp" })`, `conn.setNoDelay(true)`, writes
  `token + "\n"` as the **first bytes**, and returns `{ reader: conn.readable,
  writer }`. **There is no `FrameReader` interface** — the reader contract is
  literally `ReadableStream<Uint8Array>`, which `conn.readable` already is
  (verified against Deno docs + `framing.test.ts` mocks, and **confirmed by the
  Phase-0 spike**: the real `readFrames(conn.readable)` and `writeFrame` ran
  unmodified). The writer is a **short-write-safe `writeAll` loop** over
  `conn.write` satisfying `FrameWriter` (`{ write(bytes): Promise<number> }`) —
  partial socket writes are legal even though the spike didn't hit one. Because
  `connectControl` lives *inside* the package, its `import type … from
  "./types.js"` resolves normally (the spike's only wrinkle was a cross-dir
  `deno check` on `.js`, which does not apply in-package and never affects
  `deno run`).
- `main.ts` (currently 23 lines, no arg parsing): when `--control` is present,
  `await connectControl(Deno.args)` and pass the returned `reader`/`writer` into
  the **unchanged** `runHost(reader, writer, denoHost)`. `runHost`/`host.ts`
  need **no signature change** (verified). The token line is consumed by
  `connectControl` before `runHost`, so it never reaches the demux — this is why
  **no `ToEngine`/`FromEngine` protocol type changes**.
- `deno-host.ts` needs **zero changes** — the `respectStreams ? Deno.stdout`
  passthrough (`:219`) silently becomes correct once stdout is no longer the
  channel.
- **Bundle rebuild trap:** editing `main.ts`/`framing.ts` requires
  `cargo xtask build-engine-host-bundle` + `cargo build` for `q2` to pick it up
  (the embedded `dist/engine-host-deno.js` via `include_str!`).
- **Coverage honesty:** `main.ts` is **excluded from `tsconfig.json` + vitest**,
  so **D-MAIN has no unit test** — it is covered only by integration rows #6a/#7
  and E2E row #9. Row #8 (vitest) covers **D-CONNECT** in `framing.ts` only. Do
  not expect (or add) a vitest test for `main.ts`.

### Rollout — decided: staged hard-swap (4 stages, mapped to the checklist)

**End state is a single TCP path** (the stdout footgun and its contract fully
deleted). Staged so each phase's HEAD is green and shippable:

1. **Phase 1 — build both transports; production stays stdio.** Land
   `TcpTransport`/`TcpReadHalf` + `accept_and_handshake`. Production
   `ensure_started` does **not** yet pass `--control`; the TCP path is reached
   **only by the new in-proc Rust tests** (rows #1–#5, #10, which use in-test
   peers and need no Deno side). This is what keeps every existing deno-gated
   e2e test green.
2. **Phase 2 — teach the harness to dial back.** `connectControl` + `main.ts`
   selection + bundle rebuild. Still no production flip: the bundle now
   *understands* `--control` but never receives it.
3. **Phase 3 — flip production to TCP and validate.** `ensure_started` now always
   passes `--control`/`--token`. Integration + E2E + **Windows CI** exercise the
   real dial-back. Delete the obsolete stdout-contamination probes.
4. **Phase 4 — cutover.** Delete `StdioTransport`/`StdioReadHalf`/
   `StdioWriteHalf` (and migrate `spawn_into`/`start_with_command`), the
   `MAX_CONSECUTIVE_MALFORMED_LINES` leniency (a *global* switch — see Rust side),
   and the "console.log corrupts the protocol" contract. No permanent
   dual-transport, no per-transport malformed policy.

**The flip cannot precede Phase 2**: q2 passing `--control` to a bundle that
cannot dial back would make every real-subprocess test wait out the accept
deadline and fail.

(Rejected: *permanent stdio fallback* — it would retain the exact footgun this
plan removes and force two code paths forever. *Immediate hard-swap* — loses the
Windows-validation cushion. Decision recorded 2026-07-08.)

## Cross-platform safety (Windows / Linux / macOS)

Loopback TCP is the **most** portable dedicated-channel option:

- `std::net::TcpListener`/`TcpStream` and `Deno.connect`/`Deno.listen` over TCP
  work identically on all three OSes with no conditional compilation.
- **Windows Firewall:** loopback (`127.0.0.1`) is exempt from WFP filtering —
  no prompt. **Bind the v4 literal `127.0.0.1`, not `localhost`/`0.0.0.0`** (v4
  literal avoids a `localhost → ::1` listener/dialer mismatch and the `0.0.0.0`
  firewall prompt).
- **Precedent:** Jupyter kernels already use localhost TCP via `runtimelib`
  (`daemon.rs`) on all platforms, and the Julia control server does the same
  inside the validation engine.
- Ephemeral `:0` + a single long-lived connection ⇒ no port-exhaustion / TIME_WAIT
  concern.

## Considered and rejected

- **Status quo (stdout protocol).** Rejected: the leak class is unfixable in
  principle on a shared fd; the log-and-skip mitigation has a documented
  frame-loss residual.
- **Dedicated fd 3 (LSP-note suggestion).** Rejected on portability. Rust `std`
  has **no** portable extra-fd API — Unix needs `pre_exec` (unsafe) or the
  Unix-only `google/command-fds`; Windows needs raw
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` winapi (CRT wires only fds 0–2). Native
  `Deno.Command` exposes only stdin/stdout/stderr (the arbitrary-fd `stdio`
  array is a Node-compat feature); a running Deno reading its own fd 3 needs
  `Deno.openSync("/dev/fd/3")` — **no `/dev/fd` on Windows**. So fd 3 is a
  two-sided `#[cfg]` fork with possibly no clean Windows story — strictly worse
  than TCP for the *same* isolation benefit. (Aside: "LSP servers use fd 3" is
  inaccurate — LSP uses stdio or a socket.)
- **Unix-domain socket + Windows named pipe.** Rejected: `std` has
  `UnixListener` but no Windows named-pipe support (needs `interprocess` or
  winapi) ⇒ a `#[cfg]` fork. Windows *has* had `AF_UNIX` since Win10 1803, but
  neither Rust `std` nor Deno exposes it portably, so it doesn't rescue this.

## Interim mitigation (optional, decoupled from 1.6)

If 1.6 is not scheduled soon, the specific leak funnels through one place —
`respectStreams ? Deno.stdout : undefined` in `deno-host.ts:219`. Redirecting
that write-through to **stderr** (or buffering it) stops nested-child stdout from
hitting the protocol channel without any transport change. Smaller blast radius;
a patch, not the fix. Track separately if desired.

## Test plan — frozen Test Seam Spec (TDD — write these first)

Every test below is bound to a **named production hunk** whose revert reddens a
**named assertion** (per `prevalidating-test-seams` + `fail-on-revert`). An
executor implements these seams as written; once a test goes green its assertions
and harness are **frozen** — never edited to go green.

### Production-hunk legend (the revert targets)

| ID | Hunk (in `ts_process.rs` unless noted) |
|----|----------------------------------------|
| **H-SPAWN** | append `--control 127.0.0.1:<port> --token <tok>` to the `deno run` args |
| **H-ACCEPT** | nonblocking accept loop with `child.try_wait()` liveness + deadline |
| **H-TOKEN** | bounded token-line read + constant-time compare + reject-on-mismatch |
| **H-FRAME** | newline framing in `TcpTransport::send` / `TcpReadHalf::recv` |
| **H-READER** | build `TcpReadHalf` from the **handshake `BufReader`** (shared, not fresh) |
| **H-COMMIT** | `set_nodelay` + `try_clone` split + **close the listener after accept** |
| **H-MALFORMED** | malformed frame → immediate kill (deletion of `MAX_CONSECUTIVE_MALFORMED_LINES`) |
| **H-STDOUT** | `stdout_loop` diagnostic drain thread; joined on teardown |
| **H-SHUTDOWN** | `shutdown()` half-close (`Shutdown::Write`) + `Drop` kill→socket-close→`Eof` |
| **D-CONNECT** | `connectControl` (`framing.ts`): dial, `setNoDelay`, write `token+"\n"` **first**, return `{conn.readable, writeAll-writer}` |
| **D-MAIN** | `main.ts`: parse `--control`, wire `connectControl` result into `runHost` |
| **WASM-GATE** | `ts_process.rs` stays `#[cfg(not(target_arch="wasm32"))]`; no `std::net` in wasm build |

### Seam table (tier · real unit · seam · mock boundary · named revert)

| # | Test | Tier | Real unit (never mocked) | Seam: mount · events · assertion surface | Mock boundary | Revert → RED |
|---|------|------|--------------------------|------------------------------------------|---------------|--------------|
| 1 | Transport round-trip | Rust in-proc | `TcpTransport::send`+`TcpReadHalf::recv` (H-FRAME) | real in-test loopback socket; peer echoes a `Response` line; assert `recv()`==`Response{id}` sent | counterparty peer thread (not the unit) | Drop `write_all(b"\n")` in **H-FRAME** `send` → recv never yields frame |
| 2 | Token handshake (a–e) | Rust in-proc | `accept_and_handshake(listener, child, token, deadline)` (H-ACCEPT/H-TOKEN/H-COMMIT) — called directly with a test-owned listener + child + short deadline | bind; client presents {ok/wrong/overlong-no-`\n`} token; **each case wrapped in a 2 s test-timeout so a hang = RED** | dialing client + trivial real child handle | (b) revert **H-TOKEN** compare→wrong accepted; (c) revert **H-COMMIT** listener-close→2nd connect succeeds; (d) revert **H-ACCEPT** deadline→timeout hangs; (e) revert **H-TOKEN** length-cap→unbounded read hangs |
| 3 | Reader-handoff integrity | Rust in-proc | **H-READER** | client writes `token+"\n"` **coalesced in ONE `write_all`** with a full frame line; build read half from handshake reader; assert first `recv()`==that frame | dialing client | revert **H-READER** (rebuild read half from raw stream/fresh `BufReader`) → first frame dropped |
| 4 | Child-death / never-connected | Rust (real exiting child) | **H-ACCEPT** try_wait branch + failed-spawn contract | bind; spawn child that exits without dialing; assert `Err(recent_stderr)` **arrives <2 s (≪ 10 s deadline)**; failed-spawn contract observed via the **public `spawn_count()`** (unchanged after the failure — `init` errors before the `:593` bump) and a subsequent `ensure_started()` re-attempting the spawn; never-connected: `Drop` returns promptly (no reader/stdout thread to join) | child = real short-lived process | revert **H-ACCEPT** `try_wait` branch (deadline-only) → error waits full deadline → the "<2 s" assertion RED |
| 5 | Shutdown/Drop over TCP | Rust in-proc | **H-SHUTDOWN** | TcpTransport to in-test peer that echoes then blocks; `shutdown()` → assert peer sees EOF & reader breaks with **NO `ProcessCrashed`**; `Drop` → `reader.join()` returns <1 s | in-test peer | revert **H-SHUTDOWN** `Shutdown::Write` → peer never EOFs → shutdown blocks / reports `ProcessCrashed` |
| 6a | stdout garbage harmless | Integration, deno-gated | transport move (**D-MAIN**) | deno child serves protocol over TCP while writing non-JSON to **stdout**; assert round-trip completes, no kill | real deno fixture | revert **D-MAIN** (wire `runHost` back to `Deno.stdin/stdout`) → stdout garbage corrupts frames |
| 6b | Malformed **socket** frame fatal — **Phase 4** | Rust in-proc | **H-MALFORMED** | in-test peer sends ONE non-JSON line **on the socket**; assert immediate "channel compromised" kill + broadcast (not tolerated). **Lands in Phase 4 only**: `reader_loop` is transport-agnostic, so this is a global switch that must arrive with stdio's deletion (and with the deletion of the two leniency unit tests at `:2088`/`:2174`, which it is mutually exclusive with) | in-test peer | revert **H-MALFORMED** (re-add leniency) → single malformed line tolerated |
| 7 | `console.log` harmless | Integration, deno-gated | transport move + **H-STDOUT** | fixture engine calls `console.log("MARK")` in execute; render over TCP; assert **(i)** render succeeds **and (ii)** "MARK" observed on the stdout drain | real deno fixture engine | revert **D-MAIN** → console.log corrupts frames. **Exercised-guard: (ii) proves the engine actually logged (else vacuous)** |
| 8 | `connectControl` round-trip | Deno/vitest | **D-CONNECT** | inject **mock `Deno.Conn`** (readable=canned Response stream, write=recording); assert **first bytes == `token+"\n"` (order-checked)**, then a Request round-trips via real `readFrames`/`writeFrame` | `Deno.connect` (network) | revert **D-CONNECT** `writeAll(token+"\n")` → "first bytes==token" RED; revert `{reader: conn.readable}` → round-trip RED |
| 9 | E2E Julia over TCP | Full binary | whole stack | `cargo run --bin q2 -- render <julia fixture>`; assert success, **byte-parity vs stdio baseline**, and a tracing marker confirms TCP was used | none (real binary) | revert **H-SPAWN** (omit `--control`) → child never dials → ensure_started times out → render fails. **Exercised-guard: assert the "connected 127.0.0.1:<port>" marker** |
| 10 | Large-payload deadlock-freedom | Rust in-proc | continuous-drain over TCP (H-FRAME + demux) | round-trip an Execute-shaped frame with a **>64 KB** input; assert it completes | in-test peer | a `send` that holds the write lock across a socket-full blocking write (or buffers whole msg before the reader drains) → deadlock → test-timeout RED |
| — | `cargo xtask verify` (gate) | gate | **WASM-GATE** | run verify | — | un-gate `ts_process.rs` / leak `std::net` into wasm → wasm build breaks |

### Refactor-induced vacuity guards (do NOT skip)

- **#3** is vacuous unless the `token+"\n"`+frame are sent in **one coalesced
  write** — sent as two writes, the kernel socket buffer retains the frame and a
  *fresh* reader passes anyway, hiding the very bug the test exists to catch. The
  coalesced write is the discriminator.
- **#4** must assert the error arrives **≪ the accept deadline** — asserting only
  "an error occurs" passes via the deadline path even if `try_wait` never fires.
- **#5**'s discriminator is **"no `ProcessCrashed`"**, not "shutdown returned" —
  a missing half-close can still return (via the `Drop` kill) while wrongly
  routing through the crash path; only the crash-vs-graceful surface distinguishes.
- **#7**'s **(ii)** is the exercised-guard — without it the test passes when the
  engine never logs (sibling trap).

### Missing-test pass (reasoned, per the skill)

- **Added** because the raw change-list didn't name them: #6b (socket-malformed
  *fatality*, distinct from stdout garbage — **lands in Phase 4**, since it is a
  global `reader_loop` switch), #10 (large-payload deadlock-freedom — the plan
  calls the continuous-drain property load-bearing, not an optimization), and
  #7(ii)'s exercised-guard.
- **Accepted-untested (rationale, not silently omitted):**
  - **Constant-timeness of the token compare.** Row #2(b) asserts only that a
    wrong token is *rejected*. Timing behavior is hygiene, not load-bearing (the
    listener accepts once, then closes), and is impractical to assert reliably.
  - **`main.ts` (D-MAIN) has no unit test** — it is excluded from
    `tsconfig.json`/vitest. Covered by integration rows #6a/#7 and E2E row #9 only.
  - **Bind→dial race** (a hostile local process connecting before the child): v1
    relies on the 128-bit token's unguessability as sole defense; the optional
    pid/peer-loopback check is deferred. No deterministic way to *win* the race
    in a unit test, so there is nothing to bind — the contract is token entropy.
  - **`stdout_loop` non-blocking while the child spews during teardown**:
    partially covered by #5 (Drop-join) and #7 (drain). A dedicated
    spew-during-shutdown test is accepted-untested; the `Drop` SIGKILL backstop
    bounds it. Flag for addition if parallel-Pass-2 teardown ever races.
  - **Windows** is a *platform gate*, not a seam — the same seam tests run under
    Windows CI (Phase 3). Logged so "green on macOS" isn't mistaken for "covered."

### Probe-file disposition (frozen-assertion change)

`ts_process_framing_probe.rs`'s `pc_c_b_foreign_line_is_malformed` /
`pc_c_b_prime_interleaved_bytes_corrupt_frame` assert **stdout-contamination
tolerance** — a scenario that becomes **impossible** post-swap (stdout is no
longer the channel). **Delete** them (not migrate — migrating would assert
tolerance of a thing that can't happen = vacuous), replaced by #6a (stdout
garbage harmless) + #6b (socket-malformed fatal). Vacuity-checked: #6a's "succeeds
despite stdout garbage" strictly differs from the pre-swap state (would corrupt).

## Checklist

Design ratification is **complete** — handshake, token shape, malformed policy,
stdout drain, and rollout are all decided above (2026-07-08). The Phase-0 spike
is **done** (PASS). Remaining is the staged build.

### Phase 0 — spike (de-risk before TDD)
- [x] Prototype the dial-back end-to-end on macOS: q2 binds, spawns `deno …
      --control/--token`, child dials back and round-trips over TCP using the
      **real `framing.ts`** primitives. **PASS 2026-07-08** (deno 2.9.0;
      ~75 ms cold / ~27 ms warm; child-death + wrong-token failure modes clean;
      reader-handoff byte-loss hazard confirmed and folded into step 5). Files
      were throwaway (scratch dir); repo untouched.

The **Test Seam Spec is frozen** (2026-07-08) — every test bound to a named
revert hunk.

> **The fail-on-revert cycle** (per test, in this order): write the test → it is
> **RED from feature-absence** → implement its hunk → **GREEN** → revert the hunk
> → confirm **RED** (this proves binding) → reapply. You cannot revert a hunk
> before it exists; the revert step comes *after* green, not before.

### Phase 1 — build both transports; **production stays stdio** (plan1a-host)
- [ ] Seam rows **#1–#5, #10** (all in-proc, in-test peers — no Deno side needed),
      each through the fail-on-revert cycle above.
- [ ] `TcpTransport`/`TcpReadHalf` (H-FRAME/H-READER) + `accept_and_handshake`
      (H-ACCEPT/H-TOKEN/H-COMMIT) in `ts_process.rs`; `spawn_into_tcp`; grow the
      `init`-closure tuple with `Option<ChildStdout>`; add the `stdout_reader`
      field + both join sites; `stdout_loop` (H-STDOUT) **TCP-path only**;
      shutdown/Drop parity (H-SHUTDOWN).
- [ ] **Do NOT flip production.** `ensure_started` still spawns without
      `--control`; the TCP path is exercised only by the tests above. Every
      existing deno-gated e2e test must stay green at this HEAD.
- [ ] **Not in this phase:** H-MALFORMED and seam #6b (global switch — Phase 4).

### Phase 2 — Deno dial-back (Plan 1b) — still no production flip
- [ ] Seam row **#8** (`connectControl` over a mock `Deno.Conn`) via the
      fail-on-revert cycle.
- [ ] Write `connectControl(args)` in `framing.ts` (from scratch; D-CONNECT);
      `main.ts` arg parsing + channel selection (D-MAIN); `writeAll` writer;
      `setNoDelay`. (No vitest for `main.ts` — it is excluded from tsconfig/vitest.)
- [ ] Rebuild the embedded bundle (`cargo xtask build-engine-host-bundle`) +
      `cargo build`. The bundle now *understands* `--control` but never gets it.

### Phase 3 — flip production to TCP, then validate (incl. Windows)
- [ ] **Flip:** `ensure_started` now always passes `--control 127.0.0.1:<port>
      --token <tok>` (H-SPAWN). This is the first HEAD where real subprocesses
      speak TCP.
- [ ] Integration seam rows **#6a** (stdout garbage harmless) + **#7**
      (console.log harmless, with the exercised-guard) green.
- [ ] Delete the two obsolete `ts_process_framing_probe.rs` stdout-contamination
      probes (see Probe-file disposition).
- [ ] E2E seam row **#9**: Julia render over TCP; record invocation + output
      snippet here; assert byte-parity vs the stdio baseline + the TCP marker.
      (Capture the baseline **before** the flip — after Phase 4 there is no stdio
      path to compare against.)
- [ ] **Windows CI** exercises the loopback + accept-poll + token path.
- [ ] `cargo xtask verify` green (WASM-GATE: `ts_process.rs` stays wasm-gated).

### Phase 4 — hard-swap cutover (delete stdio + make malformed fatal)
- [ ] Seam row **#6b** (malformed socket frame fatal) via the fail-on-revert cycle.
- [ ] H-MALFORMED: delete the `MAX_CONSECUTIVE_MALFORMED_LINES` leniency
      (`:435-440,1087-1164`) **and** the two `#[cfg(test)]` tests that bind it
      (`test_stray_lines_below_bound_are_skipped_not_fatal` `:2088`,
      `test_malformed_beyond_bound_escalates_distinct_from_crash` `:2174`) — they
      are mutually exclusive with #6b on the shared `reader_loop`.
- [ ] Delete `StdioTransport`/`StdioReadHalf`/`StdioWriteHalf`. **Migrate their
      consumers — this is the largest Phase-4 item, scope it before starting:**
      - `spawn_into` (`:319`, `pub`) — callers: `start_with_command` (`:510`),
        `ensure_started` (`:543`), and `ts_process_framing_probe.rs` (already
        deleted in Phase 3). Production moves to `spawn_into_tcp`.
      - `TsEngineHost::start_with_command` (`:503`) is **`#[cfg(test)]`-gated,
        "test-only"** — but it has **~10 call sites** across `ts_process.rs`'s own
        test module and `registry.rs:509`, and they spawn **stdio-speaking**
        children (`sh -c 'cat >/dev/null'`, `deno eval …`) that will never dial
        back. Port the helper to the TCP handshake (bind + spawn + handshake) and
        replace those child programs with tiny dial-back scripts. These tests
        exercise real reaping/teardown, so port rather than delete.
- [ ] Delete the stdout-contract language across plan1a-host / plan1b /
      `engine-host-concurrency.md`; update the grand-plan overview.
- [ ] Re-run `cargo xtask verify`.

## Open questions (remaining)

None blocking. Everything below is a tunable or a Phase-4 confirmation, not a
fork.

- **Accept deadline value** — settled at ~10 s (spike measured ~75 ms cold /
  ~27 ms warm, so ~130× headroom). Injectable so tests use a short deadline.
  Revisit only if a cold-cache CI box ever trips it.
- **Phase-4 `start_with_command`** — confirm whether `behave_engine_e2e` can move
  to the TCP path or whether the helper should be deleted outright. Determined at
  cutover by what the test actually needs; not a design fork.

*Resolved 2026-07-08:* token = transport **pre-line**, not a protocol frame (this
is what preserves "no protocol-type change"); rollout = **staged hard-swap** with
the production flip pinned to **Phase 3** (cannot precede the bundle); malformed
-fatal is a **global** switch, so it lands at the **Phase-4** cutover with stdio's
deletion; token uses **`uuid`** (existing dep, `daemon.rs` precedent) so there is
**no new crate**; Plan 5 pooling needs no transport-lifecycle change since the
persistent `TcpStream` outlives renders like a persistent pipe.

## References

- `claude-notes/designs/engine-host-concurrency.md:195` — canonical deferred note
- `claude-notes/plans/2026-04-16-plan1a-host.md:1193`, `:212`, `:571`, `:1448`
- `claude-notes/plans/2026-04-16-plan1a-protocol.md:388` (Phase 1.5 origin of the numbering)
- `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md:1473-1477` (Phase 1.6 doc comment; `connectControl` written from scratch)
- `crates/quarto-core/src/engine/ts_process.rs` — `EngineTransport`/`EngineReadHalf` (`:169-193`), `spawn_into` (`:319-361`), demux/crash (`:1053-1229`), malformed (`:435-440,1087-1164`), stderr_loop (`:1237-1267`)
- `ts-packages/quarto-engine-host-deno/src/{main,framing,host,deno-host}.ts` (reader = `ReadableStream<Uint8Array>`; `FrameWriter` only; no `connectControl` yet)
- `crates/quarto-core/src/engine/jupyter/daemon.rs` — ephemeral loopback + per-session key precedent
- `ts-packages/quarto-hub-mcp/src/auth/loopback.ts:169-179` — constant-time token compare + settle-once teardown precedent
- `crates/quarto/src/commands/preview.rs:273-337` — ephemeral bind + connect-poll race precedent
- `crates/quarto-core/tests/integration/ts_process_framing_probe.rs` (the leak probes to invert)
- `~/src/quarto-julia-engine/src/julia-engine.ts` (worked example; TCP+key precedent)
