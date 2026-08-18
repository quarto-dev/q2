# Plan 1a.6: Move the engine-host protocol off stdout → loopback TCP

> **Status:** ● **COMPLETE 2026-07-23** (Phases 1–4 all landed + verified; the
> loopback-TCP transport is the sole engine-host transport, stdio deleted). The
> only remaining gate is Windows CI (a platform gate that fires when CI picks up
> the branch). Originally implementation-ready 2026-07-08; revised 2026-07-22.
> The phase split is ordered so **each phase's HEAD is green**: build both
> transports (production stays stdio) → teach the harness to dial back → flip +
> validate → cutover. Additive cleanup over the completed 1a/1b transport; blocks
> nothing; pullable whenever the stdout footgun is judged worth retiring. Owned
> jointly by **plan1a-host** (Rust transport swap) and **Plan 1b** (Deno-side
> dial-back); **plan1a-protocol contributes nothing** — the wire *shapes* do not
> change.
>
> **2026-07-22 revision (post-review + code research):** five corrections folded
> in. (1) **Token delivery moved to stdin**, not argv (argv leaks the secret via
> `ps`/`cmdline` to the exact different-uid attacker the token defends against).
> (2) **`handle_crash` must kill before `wait()`** (H-CRASH-REAP) — under TCP a
> socket-EOF no longer implies process-exit, so the bare blocking `wait()` can
> hang the reader thread. (3) **Drains spawned before the accept** (H-DRAIN) — a
> chatty child would otherwise fill its pipes and deadlock the up-to-10 s
> handshake, and the child-death error would carry no stderr. (4) **`connectControl`
> lives in a new Deno-only module** (excluded from tsconfig), tested via a
> **`.deno-test.ts` on a real socket** — it cannot go in the `Deno.*`-free
> `framing.ts`, and seam #8 cannot be a vitest mock; that tier is **CI-only**.
> (5) **`pc_c_a` probe migrates to TCP** rather than being dropped. Also: the
> "eager first frame" byte-loss rationale was a spike artifact (the real host is
> reactive) — the reader-handoff rule stays, its justification is corrected.
>
> **2026-07-22 (review-3 pass):** seven spec corrections folded in after a third
> code-vs-plan review (the code assumptions all verified — this pass fixes gaps in
> the *spec*, not the design). (1) **Accepted socket must be reset to blocking**
> (`set_nonblocking(false)`) before the token `read_line` — on **Windows** the
> accepted socket inherits the listener's non-blocking flag (Linux/macOS do not), so
> the macOS Phase-0 spike could not catch this; validated on Windows CI at Phase 3.
> (2) **Deno-side stdin token read** must be line-bounded and must not over-consume
> past the token's `\n` — the symmetric H-READER hazard on the Deno side, previously
> undiscussed. (3) **Seam #10 shrinks the in-test socket buffers via `socket2`** (a
> `dev-dependency`, already transitive via tokio) so a small, deterministic payload
> blocks — replacing the hardcoded 1 MB, which Linux's multiple-MB loopback autotune
> could render vacuous. (4) **H-SPAWN(a)** (token→stdin write) is now
> **bound in Phase 1 by seam #4** (its child echoes the stdin token to stderr), not
> left unbound until Phase 3. (5) **Row #4's injected `init` calls the real
> `spawn_into_tcp`** — spelled out, since H-DRAIN/H-SPAWN(a) live there. (6) **stdin
> is closed (EOF) after the token**, not "diagnostic-only"; the future `run()` mode
> is reframed as an explicit "keep stdin open" change. (7) init-closure vs
> `spawn_into_tcp` **division of labor** made authoritative.

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
listener, passes the **non-secret address in argv** (`--control 127.0.0.1:<port>`)
and delivers the **one-time token as the first line on the child's (now-freed)
stdin** (see "Token delivery" below — *not* argv). The subprocess reads the token
from stdin, dials back, and presents it as a **transport pre-line on the socket,
consumed before any frame** (never a protocol message — this is what preserves "no
protocol-type change"). After the token line, **stdin is closed (EOF)** and stdout/stderr are
**diagnostic-only**, and the entire "console.log corrupts the protocol" contract —
plus its Rust-side log-and-skip mitigation — is deleted (at the Phase-4 cutover;
see Rollout).

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

**What this secures (and what it does not).** 1.6 gives the protocol channel
**integrity and confidentiality**: with a dedicated socket + one-time token, no
other local process can inject or read frames, and the stdout-leak corruption
class becomes impossible. It does **not** sandbox the extension: the Deno child
still runs `--allow-all`, so a malicious extension can open its own sockets, read
files, and spawn processes — the token stops a *third party* racing the port, not
a *hostile extension* (which is fully trusted by design, decided 2026-07-01). The
`ts_process.rs` comment that cites "Phase 1.6" as the future justification for
narrowing `--allow-all` is about a **later, separate** change: once the control
channel is a single known `127.0.0.1:<port>`, a future revision *could* replace
`--allow-all` with `--allow-net=127.0.0.1:<port>` (+ whatever `--allow-read/write/run`
engines legitimately need) — the ephemeral port is already in hand at spawn, so
1.6 **enables** that narrowing but does not itself perform it. (Caveat: engines
open their own loopback sockets — e.g. the Julia control server — so a real
narrowing must allowlist those too; per-engine policy, not a transport blocker.)

### Token delivery — stdin pre-line, not argv (ratified)

The token authenticates the dialer against a local process racing the ephemeral
port between `bind` and the child's dial-back. The **only** attacker it
meaningfully defends against is a **different-uid** local process (a same-uid
attacker can ptrace the child, read its memory, or shadow `deno` on `PATH` — the
token is irrelevant to them). So the token must not be delivered by any channel a
different-uid process can read:

- **argv (rejected).** `/proc/<pid>/cmdline` is world-readable on Linux and `ps`
  shows args on macOS, for the child's whole lifetime — leaking the secret to
  exactly the attacker the token exists to stop. It defeats its own purpose.
- **environment variable (rejected).** `/proc/<pid>/environ` is mode 0600 (not
  world-readable), but env is **inherited by every descendant** — and engines
  spawn grandchildren (the Julia control server; anything through the
  `respectStreams` passthrough at `deno-host.ts:219`). Every one would inherit the
  token.
- **stdin pre-line (chosen).** The pipe is shared only by parent and child. No
  same-uid *or* different-uid process can read it — no `ps`, no `cmdline`, no
  `environ`, no inheritance. This plan **frees stdin** (the protocol moves to
  TCP), so writing `<token>\n` as the first bytes of stdin costs nothing: the
  child reads exactly one line at startup, after which **1.6 closes stdin** (the
  parent drops `ChildStdin`, so the child sees EOF) and stdin carries nothing more.
  (A future interactive `run()` mode would **keep stdin open** past the token line
  instead of dropping it — an explicit future change, not something 1.6 leaves
  available by default.) It is
  symmetric with the socket-side "token as a pre-line consumed before any frame"
  discipline — two pre-lines, same idea.

Residual (accepted, same as any in-memory secret): the token transits the
parent's memory and the pipe, so a same-uid attacker with ptrace / `/proc/<pid>/mem`
can recover it — but that attacker already owns the process. stdin does not widen
this.

## Effect on engines — none required; strictly permissive

Engines **never touch the protocol channel** — the harness does. An engine
implements `ExecutionEngine` methods and returns values; the only code that wires
the **protocol channel** to Deno streams is `@quarto/engine-host-deno/src/main.ts`
(`runHost(Deno.stdin.readable, Deno.stdout, denoHost)`). (`main.ts`'s own header
comment claiming it is the sole `Deno.*` touch-point is stale — `deno-host.ts`
uses `Deno.*` extensively for the `PlatformHost`; it just never touches the
channel.) Under 1.6 **only the harness changes** — `main.ts` (channel selection)
and a new Deno-only `control-transport.ts` (the new `connectControl` — **NOT**
`framing.ts`; see "Deno side"). **No engine recompiles or adapts.**

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

**Coordination with plan1c3 (hermetic fixtures) — sequential on this branch.**
plan1c3 (executing now on `feature/ts-engine-extensions`) deletes the committed
synth/echo bundles and **regenerates them at test time** via
`crate::engine_fixture_build::ensure_bundle` — a fixture is only regenerable if it
imports `@quarto/api/claims` and is listed in `HERMETIC_FIXTURES`. 1.6 is
**file-disjoint** from 1c3 (1c3 never touches `engine/ts_process.rs` or
`engine-host-deno/`; 1.6 never touches `extension::build`/`CallCommands`), with one
shared file, `behave_engine_e2e.rs`, edited in non-overlapping regions. The one
thing 1.6 must respect: its deno-gated seams **#6a (stdout garbage) / #7
(`console.log`)** need a fixture engine that fits 1c3's regime. **Prefer reusing an
already-hermetic engine (`echo-engine`) and having it emit the marker** rather than
adding a new committed bundle; if a new fixture is unavoidable, register it in
`HERMETIC_FIXTURES` and import `@quarto/api/claims`. (If 1.6 lands *before* 1c3 the
committed bundles still exist and this is moot.)

**Bonus unlocked (not in scope here):** with stdout no longer the protocol channel
and stdin unused after the token, a future interactive `run()` mode could **keep
stdin open** past the token line and let engines own real stdin/stdout. Recorded,
not built.

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
> in step 6, and the generous deadline in step 5). **Caveat (2026-07-22):** the
> spike's throwaway child sent an eager frame and never blocked its pipes; the
> real host does neither — see the eager-frame correction in step 6 and the
> drain-before-accept requirement in step 4, both added after code research the
> spike did not cover.

### Handshake (ratified)

The child is stored in `spawn_into` **before any thread exists**
(`ts_process.rs:352`), and `std::net::TcpListener::bind` calls `listen()`
internally, so the kernel backlogs connections from `bind` onward — so binding
before spawn has no race window. (`preview.rs:273-337` is a connect-*retry* probe
whose doc comment documents this backlog property for tokio's bind; it is a
weaker precedent than first cited — it does not itself bind an ephemeral
listener — but the std guarantee is what we actually rely on.) Sequence, inside
`ensure_started_inner`'s init closure so a failure leaves `write` unset and the
existing retry contract holds (`ts_process.rs:636-637`):

1. **Bind** `std::net::TcpListener` on the `127.0.0.1` **literal** `:0`
   (ephemeral). Read the OS-assigned port from `local_addr()`.
2. **Generate** a one-time token — `uuid::Uuid::new_v4().to_string()` (122 bits of
   CSPRNG entropy, hyphenated hex; existing workspace dep, `daemon.rs:111`
   precedent — see below). (The earlier "128-bit getrandom, hex-encoded" phrasing
   was inconsistent with the `uuid` choice; `uuid` it is — 122 bits is ample for a
   single-accept, immediately-closed listener.)
3. **Spawn** `deno run --allow-all <bundle> --control 127.0.0.1:<port>`. The
   **address is argv (non-secret)**; the **token is written as `<token>\n` to the
   child's stdin** (piped, not inherited) — see "Token delivery." `--allow-all`
   already grants net, no perm change.
4. **Spawn the stderr (and freed-stdout) drain thread(s) BEFORE the accept loop.**
   This is load-bearing: the accept below can block up to the ~10 s deadline, and
   if nothing is draining the child's pipes, a child that writes more than one pipe
   buffer (~64 KiB) of diagnostics *before* it dials back blocks on its own
   `write()`, never dials, and is killed at the deadline — with the very stderr we
   want for the error message stuck in the pipe. Draining concurrently with the
   accept both prevents that deadlock and lets a child-death error carry real
   `recent_stderr`. (This is why the drain lives in `spawn_into_tcp`, *before*
   `accept_and_handshake` — not in `ensure_started_inner` after `init()` returns,
   where the existing stdio drain is spawned. See "Rust side.")
5. **Accept with child-liveness polling** (the one pattern with no in-repo
   precedent — composed here): `listener.set_nonblocking(true)`, then loop
   `accept()` ⇄ `child.try_wait()` on `WouldBlock`, ~20 ms poll, against a
   **generous ~10 s deadline**. (Spike observed ~75 ms cold / ~27 ms warm, so
   10 s is ~130× headroom; keep it generous for a cold-cache or loaded CI box —
   tunable. Note the deadline is held under the coarse init lock — see "Rust
   side" — so the worst case stalls concurrent spawns; the normal path is ~75 ms.)
   Child exits first → error carrying the (now-populated) `recent_stderr` ring
   (likely a Deno/engine load failure); deadline with child alive → `child.kill()`
   + error; connection arrives → proceed.
6. **Validate token** — **first put the accepted stream back in blocking mode**
   (`stream.set_nonblocking(false)`): the listener was switched to non-blocking for
   the step-5 poll loop, and on **Windows the accepted socket inherits that flag**
   (Linux/macOS do not), so the blocking `read_line` below would otherwise return
   `WouldBlock` and break the handshake (see Cross-platform safety). Then **build
   one `BufReader` over a `try_clone()` of the
   accepted stream, read the token line with `read_line`, then reuse that *same*
   `BufReader` for every subsequent frame.** This is load-bearing hygiene:
   whenever the child's first *response* (or any pipelined request) rides the same
   TCP segment as bytes past the token's `\n`, those bytes sit in the `BufReader`'s
   buffer — a *fresh* reader would silently drop them. So `TcpReadHalf` must be
   constructed *from the handshake `BufReader`*, never from the raw stream.
   **(Correction to the earlier rationale:** the real host is purely reactive —
   `runHost` writes nothing before it receives and dispatches a request, and `init`
   gets no response, so there is **no unsolicited "eager first frame"**; that was a
   spike-child artifact. Moreover q2 does not send its first request until *after*
   the read half is built from the handshake `BufReader`, so in the **current**
   reactive protocol the "bytes past the token in the same segment" case essentially
   never fires in production. The rule is kept as cheap **defensive hygiene** — it
   costs nothing, and it is the correct construction the moment anything ever
   coalesces (a future pipelined or eager sender). It is **not** a claim that
   production coalescing happens today. Seam #3 still binds it, driving an **in-test
   peer** that deliberately coalesces `token+frame` in one write — see the seam
   table.) **Bound the token read**
   (reject > `MAX_TOKEN_LINE` = 256 bytes before a newline) so a hostile local
   dialer can't force an unbounded read. Compare **constant-time**. Mismatch → drop
   connection + kill child + error.
7. **Commit** — **exactly one `try_clone()`** (the spike's shape): the
   `BufReader` from step 6 wraps the clone and becomes `TcpReadHalf`; the
   **original `stream`** becomes the write half (`TcpTransport`). The `listener`
   (moved into `accept_and_handshake` by value) drops when the function returns —
   so a second dial is then impossible; **no explicit `drop` call is needed** (this
   is why #2c has no revert hunk; see the H-COMMIT note below). Optionally assert
   the accepted peer is loopback. **Emit the connected marker here:**
   `tracing::info!(target: "engine_host", port, "engine-host connected over loopback TCP")`
   — this is the exact line seam #9's exercised-guard asserts (proving TCP was used,
   not a fallback); it is the one net-new tracing event 1.6 adds beyond `stdout_loop`.

**Division of labor (authoritative).** The numbered sequence above is the
end-to-end flow; this is which function owns each step. The `init` closure does the
**setup** — bind the listener, read the ephemeral port, generate the token, and
assemble the `deno … --control 127.0.0.1:<port>` command (sequence steps 1–2 and the
*command-building* half of step 3) — then hands `cmd` + the bound `listener` to
`spawn_into_tcp`, which does the **spawn-and-handshake** (the actual spawn in step 3
onward through step 7: spawn the child, write the token to its stdin, spawn the
drains *before* the accept, call `accept_and_handshake`, commit the split halves).
`spawn_into_tcp` receives the already-built `cmd` and the already-bound `listener`
and moves the listener onward into `accept_and_handshake` by value.

**The handshake is a standalone, unit-testable function** — the init closure is a
thin caller, not the implementation (seam rows #2/#4 drive this function directly
with their own listener + child):

```rust
/// Accept exactly one loopback connection, validate the one-time token, and
/// return the split halves. Polls `child.try_wait()` while accepting so a child
/// that dies before dialing back fails fast instead of waiting out `deadline`.
/// The drain threads are already running (spawned by `spawn_into_tcp` before this
/// call), so a chatty child cannot fill its pipes and deadlock the accept; error
/// enrichment with `recent_stderr` is the caller's job (it owns the ring).
fn accept_and_handshake(
    listener: TcpListener,
    child: &Arc<Mutex<Option<Child>>>,   // the slot `spawn_into_tcp` already populated
    token: &str,
    deadline: Duration,                  // injectable so tests use a short one
) -> Result<(Arc<TcpTransport>, TcpReadHalf), ExecutionError>;
```

**Listener-close testability (H-COMMIT).** Because `accept_and_handshake` takes
`listener` **by value**, the listener is dropped (closed) on return regardless —
so a "revert the explicit `drop(listener)`" hunk would be a no-op and seam #2(c)
would be vacuous. The load-bearing close is therefore **structural** (move-by-value
ownership), and #2(c) asserts it differently: after a *successful* handshake the
function has returned and the listener is gone, so a second `connect()` to the
same address must fail to establish (`ECONNREFUSED`) — not merely "not be
accepted." See the seam table for the corrected assertion.

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
- **Plumbing the new halves, the token-to-stdin write, and the drains (signature
  changes — do not skip):**
  - `spawn_into` (`:319-361`) today returns `(Arc<StdioWriteHalf>, StdioReadHalf,
    ChildStderr)` and consumes `ChildStdout` into the read half. Add a sibling
    `spawn_into_tcp(cmd, child_slot, listener, token, deadline, recent_stderr)`.
    It: (1) spawns the child with **all three** stdio piped; (2) writes
    `<token>\n` to the child's `ChildStdin`, then **drops it — closing stdin, so the
    child sees EOF and stdin carries nothing after the token** (see "Token
    delivery"); (3) **spawns the `stderr_loop` and
    `stdout_loop` drain threads NOW, before accept** (this is the deadlock fix —
    the child's pipes drain during the up-to-10 s accept window); (4) calls
    `accept_and_handshake`; (5) **on accept/handshake error, owns its own cleanup**
    — the drain `JoinHandle`s do not escape on the error path, so it must
    `child.kill()` (closing the pipes) and `join()` both drain threads itself, then
    enrich the error with the (now-populated) `recent_stderr` ring before returning.
    On success it returns `(Arc<TcpTransport>, TcpReadHalf, JoinHandle /*stderr*/,
    JoinHandle /*stdout*/)` — both drains already running, handed to the host.
  - **`ensure_started_inner`'s `init` closure (`:566-575`) return grows to carry
    the drains.** Model it as an enum, because the callers supply drains at
    different times. Note there are **three** live callers today, not two — the
    signature change must keep all three compiling: the two production callers
    (`ensure_started:543`, `start_with_command:510`) pass `Some(stderr)`, and
    **`test_race_free_ensure_started:2399` passes `None`** (a mock transport with no
    real child). So the enum needs a **third `None` variant** (decided 2026-07-22 —
    the alternative, `Option<StartedDrains>`, is uglier at every match site):
    ```rust
    enum StartedDrains {
        None,                                     // mock/no-child: ensure_started_inner spawns nothing (test_race_free_ensure_started)
        Stdio(ChildStderr),                       // ensure_started_inner spawns stderr_loop (today's Some(stderr) path)
        Tcp { stderr: JoinHandle<()>, stdout: JoinHandle<()> },  // already spawned in spawn_into_tcp
    }
    // init: FnOnce() -> Result<(Arc<dyn EngineTransport>, Box<dyn EngineReadHalf>, StartedDrains), _>
    ```
    `ensure_started_inner` matches: `None` → spawn nothing (replaces today's
    `if let Some(stderr) = stderr_opt`); `Stdio(stderr)` → spawn `stderr_loop` as at
    `:612-618`; `Tcp { stderr, stdout }` → store both handles, spawn nothing. The
    stdio path's *behavior* is thereby **untouched** (only the `Option`→enum
    mechanical rewrite touches it); the mock path stays a no-op; only the TCP arm is
    new. (`with_transport:472` bypasses `ensure_started_inner` entirely — it inlines
    the reader spawn — so it needs no change.)
  - `TsEngineHost` has `reader` + `stderr_reader` join handles (`:404-406`) —
    **add `stdout_reader: Mutex<Option<JoinHandle<()>>>`** and join it at **both**
    sites: `shutdown()` (`:951-956`) and `Drop` (`:1038-1043`).
- **Freed stdout drain (`stdout_loop`) is TCP-only.** On the stdio path stdout
  *is* the protocol channel and is owned by the reader thread — spawning a drain
  there would steal frames. So `stdout_loop` exists **only** on the TCP path
  (spawned inside `spawn_into_tcp`). It mirrors `stderr_loop` (`:1237-1267`),
  drains `ChildStdout`, logs via `tracing` at INFO; no demux, no crash-ring
  (stderr covers crashes). **Test observation (seam #7(ii)) — decided
  2026-07-22: use `set_global_default`.** A `tracing::info!` from this background
  thread is **not** visible to a `with_default` thread-local subscriber (the J8
  unit-test pattern), so #7(ii) observes it via a process-global
  `set_global_default` subscriber in the integration test (the
  `behave_engine_e2e.rs:445` pattern, safe under nextest's process-per-test). The
  shared-`Arc<Mutex<Vec<String>>>`-buffer alternative was **rejected**: it would
  need a `#[cfg(test)]` injection point wired into the *production* `stdout_loop`,
  a seam that does not exist and is not worth adding. Do **not** try to bind
  #7(ii) with `with_default`.
- **Crash reaping must kill before wait — TCP makes this load-bearing (H-CRASH-REAP).**
  `handle_crash` reaps with a bare blocking `child.wait()` (`:1187-1192`) and **no
  prior kill**, on both the `Eof` (`:1084`) and `Io` (`:1170`) crash arms. Under
  stdio that is safe: stdout-EOF ≈ process exit, so `wait()` returns at once. Under
  TCP a socket EOF (child closed the conn, or a framing-layer exception tore down
  the socket **without** exiting) does **not** imply exit — so `wait()` can block
  the reader thread forever on a live child. **Fix: `child.kill()` before the
  `wait()` in `handle_crash`**, matching the three paths that already do so
  (malformed escalation `:1159`, `reset_after_crash` `:736`, `Drop` `:1032`). This
  is a **global, transport-agnostic** change (harmless for stdio — the child is
  exiting anyway) and belongs in Phase 1, not the Phase-4 cutover.
- **Shutdown/Drop parity (must replicate):** `TcpTransport::shutdown` mirrors
  `StdioWriteHalf::shutdown` (`:259-278`) **in both steps**: (1) send a best-effort
  `ToEngine::Shutdown` frame (id `u64::MAX`, newline-framed, flushed), then (2)
  half-close the write side (`TcpStream::shutdown(Shutdown::Write)`) — the TCP
  analogue of dropping stdin. The peer gets EOF on its read side and exits, waking
  `reader_loop` under `shutting_down` (the expected-exit arm at `:1079`); leaving
  the read half open lets the reader observe that clean EOF. `Drop` still
  `child.kill()`s (`:1032`); a killed peer closes the socket → `TcpReadHalf::recv`
  returns `Eof`, so `reader.join()` is instant. **Never-connected case is handled
  entirely inside `spawn_into_tcp`** (its error path kills the child and joins its
  own drains — see above), so `init` returns `Err`, `write`/`reader`/`stdout_reader`
  are never committed, and the host's `shutdown()`/`Drop` find nothing to close or
  join. The child-kill path cannot hang.
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
- **Dependencies: none new in production; one test-only.** Production code is
  `std::net` (std) only — token via `uuid` (already a workspace dep, already used by
  `quarto-core`, `daemon.rs` precedent), constant-time compare via a local XOR-fold
  helper, no `#[cfg]` fork. **Seam #10 alone** adds `socket2` as a `quarto-core`
  **`dev-dependency`** (already present transitively via tokio; MIT/Apache) to shrink
  the in-test socket buffers so the deadlock payload is small and deterministic. It
  is dev-only, so it never enters the wasm production build (WASM-GATE unaffected).

### Deno side (Plan 1b)

- **`connectControl` must live in a NEW Deno-only module — NOT in `framing.ts`.**
  This corrects the earlier plan and the `framing.ts:11` "deferred" comment.
  `framing.ts` is *in* the tsc graph and is deliberately **`Deno.*`-free** (header:
  "Pure TypeScript — no `Deno.*` APIs … runs under vitest/Node and Deno alike"),
  and the package's `tsconfig.json` has **no Deno typings** (`lib: ["ES2022",
  "DOM"]`, no `deno.ns`, no `@types/deno`). A `connectControl` that calls
  `Deno.connect` there would fail `tsc` ("Cannot find name 'Deno'") — and `tsc`
  runs in `cargo xtask verify`'s ts-packages build. So put it in a new module
  (e.g. `src/control-transport.ts`) **added to `tsconfig.json`'s `exclude` list**,
  exactly like `deno-host.ts`/`main.ts`. It:
  - reads the **token from the first line of `Deno.stdin`** (NOT `Deno.args` — see
    "Token delivery"; only `--control` is parsed from `Deno.args`) — reading with a
    **line-bounded reader that stops at the first `\n` and does not consume past
    it** (the Deno-side analogue of the Rust H-READER byte-loss hazard). This is
    moot *today* only because Rust closes stdin right after the token, but the
    future `run()` "keep stdin open" mode depends on this discipline,
  - `Deno.connect({ hostname: "127.0.0.1", port, transport: "tcp" })`,
    `conn.setNoDelay(true)`,
  - writes `token + "\n"` as the **first bytes on the socket** (a short-write-safe
    `writeAll` loop over `conn.write`, satisfying `FrameWriter` =
    `{ write(bytes): Promise<number> }` — partial socket writes are legal),
  - returns `{ reader: conn.readable, writer }`. **There is no `FrameReader`
    interface** — the reader contract is literally `ReadableStream<Uint8Array>`,
    which `conn.readable` already is (confirmed by the Phase-0 spike: real
    `readFrames(conn.readable)`/`writeFrame` ran unmodified).
- `main.ts` (currently 23 lines, no arg parsing): when `--control` is present,
  `await connectControl()` (from the new module) and pass the returned
  `reader`/`writer` into the **unchanged** `runHost(reader, writer, denoHost)`.
  `runHost`/`host.ts` need **no signature change** (verified: `runHost`'s reader
  param is already `ReadableStream<Uint8Array>` and its writer is `FrameWriter`;
  `Deno.stdout` and `conn` both satisfy these). The token is consumed from stdin
  and the socket pre-line by `connectControl` before `runHost`, so it never
  reaches the demux — this is why **no `ToEngine`/`FromEngine` protocol type
  changes**. `main.ts` is where the transport is selected, so the new module is
  imported here (esbuild bundles it — entry is `main.ts`, and `exclude` only
  affects `tsc`, not the bundle).
- `deno-host.ts` needs **zero changes** — the `respectStreams ? Deno.stdout`
  passthrough (`:219`) silently becomes correct once stdout is no longer the
  channel.
- **Bundle rebuild trap:** editing `main.ts`/`control-transport.ts` requires
  `cargo xtask build-engine-host-bundle` + `cargo build` for `q2` to pick it up
  (the embedded `dist/engine-host-deno.js` via `include_str!`; esbuild entry is
  `src/main.ts`, so the new module is bundled iff `main.ts` imports it). *(Note:
  `main.ts`'s header comment "Bundle with: esbuild (Phase 4 — not yet)" is **stale** —
  the bundle pipeline is already live, exactly as this trap describes; do not read it
  as "bundling isn't wired up yet.")*
- **Coverage honesty (corrected — seam #8 cannot be a vitest test):** the new
  Deno-only module references the `Deno` global, so it is untypeable and
  unrunnable under vitest/Node. **D-CONNECT is covered by a new `.deno-test.ts`**
  (the `deno-host.deno-test.ts` idiom: stand up a real `Deno.listen({ port: 0 })`
  in-test, call `connectControl` against it, assert on the **real** `Deno.Conn` —
  no mock `Deno.Conn`). **This tier runs ONLY in the `ts-test-suite` GitHub
  workflow (`deno test …deno-test.ts`), NOT in `cargo xtask verify`** (which only
  *builds* ts-packages). If we want local coverage, add a verify step — otherwise
  D-CONNECT is CI-gated, and that must be stated, not assumed. **D-MAIN** (arg
  parsing + wiring) remains untested at unit tier (excluded from tsconfig/vitest)
  — covered by integration rows #6a/#7 and E2E row #9 only.

### Rollout — decided: staged hard-swap (4 stages, mapped to the checklist)

**End state is a single TCP path** (the stdout footgun and its contract fully
deleted). Staged so each phase's HEAD is green and shippable:

1. **Phase 1 — build both transports; production stays stdio.** Land
   `TcpTransport`/`TcpReadHalf` + `accept_and_handshake` + `spawn_into_tcp`
   (with the token-to-stdin write and drains-before-accept) + the global
   H-CRASH-REAP fix. Production `ensure_started` does **not** yet pass `--control`;
   the TCP path is reached **only by the new in-proc Rust tests** (rows #1–#5, #2c,
   #5r, #10, which use in-test peers and need no Deno side). This is what keeps
   every existing deno-gated e2e test green.
2. **Phase 2 — teach the harness to dial back.** `connectControl` + `main.ts`
   selection + bundle rebuild. Still no production flip: the bundle now
   *understands* `--control` but never receives it.
3. **Phase 3 — flip production to TCP and validate.** `ensure_started` now always
   passes `--control` (and writes the token to the child's stdin). Capture the
   stdio byte-parity baseline **before** the flip. Integration + E2E + **Windows
   CI** exercise the real dial-back. Delete the obsolete `pc_c_b*` probes.
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
- **Accepted-socket blocking mode (Windows-specific trap):** the accept loop sets
  the *listener* non-blocking for the `try_wait` poll; on **Windows the accepted
  socket inherits that flag**, so `accept_and_handshake` must call
  `set_nonblocking(false)` on the accepted stream before the blocking token
  `read_line` (Linux/macOS do not inherit it, so this is a no-op there). Unvalidated
  until Windows CI at Phase 3 — the macOS Phase-0 spike could not surface it.
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
**named assertion** (per `prevalidating-test-seams` + `fail-on-revert`), with **two
deliberate exceptions** — **#2c** and **#10** are property/invariant tests, not
named-hunk reverts (both flagged in the vacuity guards below; do not fake a hunk
for them). An executor implements these seams as written; once a test goes green
its assertions and harness are **frozen** — never edited to go green.

**Placement note.** The new in-proc seams (#1–#5, #2c, #5r, #10) land in `mod tests`,
which already defines the `watchdog` helper at `:1629`; a *second* `watchdog` exists
at `:2728` in `mod proc_tests`. Put these seams in `mod tests` (not `mod proc_tests`)
so they reach the cited helper rather than duplicating it.

### Production-hunk legend (the revert targets)

| ID | Hunk (in `ts_process.rs` unless noted) |
|----|----------------------------------------|
| **H-SPAWN** | **two-part hunk:** (a) `spawn_into_tcp` writes `<tok>\n` to child stdin — token via stdin, NOT argv — lands **and is bound in Phase 1** (seam #4: its injected child echoes the stdin token to stderr, so reverting the write reddens #4); (b) `ensure_started` appends `--control 127.0.0.1:<port>` to `deno run` args — the production flip — lands Phase 3, bound end-to-end by #9 |
| **H-ACCEPT** | nonblocking accept loop with `child.try_wait()` liveness + deadline; `set_nonblocking(false)` on the accepted stream before the token read (Windows inherits the listener's flag) |
| **H-TOKEN** | bounded token-line read + constant-time compare + reject-on-mismatch |
| **H-FRAME** | newline framing in `TcpTransport::send` / `TcpReadHalf::recv` |
| **H-READER** | build `TcpReadHalf` from the **handshake `BufReader`** (shared, not fresh) |
| **H-COMMIT** | `set_nodelay` + `try_clone` split; listener closed **structurally** by move-by-value into `accept_and_handshake` |
| **H-DRAIN** | `spawn_into_tcp` spawns stderr+stdout drains **before** the accept loop (deadlock fix) |
| **H-CRASH-REAP** | `child.kill()` **before** `wait()` in `handle_crash` (socket-EOF ≠ process-exit) |
| **H-MALFORMED** | malformed frame → immediate kill (deletion of `MAX_CONSECUTIVE_MALFORMED_LINES`) |
| **H-STDOUT** | `stdout_loop` diagnostic drain thread; joined on teardown |
| **H-SHUTDOWN** | `shutdown()` sends `Shutdown` frame then half-close (`Shutdown::Write`) + `Drop` kill→socket-close→`Eof` |
| **D-CONNECT** | `connectControl` (new Deno-only `control-transport.ts`, excluded from tsconfig): read token from **stdin**, dial, `setNoDelay`, write `token+"\n"` **first on socket**, return `{conn.readable, writeAll-writer}` |
| **D-MAIN** | `main.ts`: parse `--control`, wire `connectControl` result into `runHost` |
| **WASM-GATE** | `ts_process.rs` stays `#[cfg(not(target_arch="wasm32"))]`; no `std::net` in wasm build |

### Seam table (tier · real unit · seam · mock boundary · named revert)

| # | Test | Tier | Real unit (never mocked) | Seam: mount · events · assertion surface | Mock boundary | Revert → RED |
|---|------|------|--------------------------|------------------------------------------|---------------|--------------|
| 1 | Transport round-trip | Rust in-proc | `TcpTransport::send`+`TcpReadHalf::recv` (H-FRAME) | real in-test loopback socket; peer echoes a `Response` line; assert `recv()`==`Response{id}` sent | counterparty peer thread (not the unit) | Drop `write_all(b"\n")` in **H-FRAME** `send` → recv never yields frame |
| 2 | Token handshake (a–e) | Rust in-proc | `accept_and_handshake(listener, child, token, deadline)` (H-ACCEPT/H-TOKEN) — called directly with a test-owned listener + child + short deadline; **each case wrapped in `watchdog(2s, …)` (`ts_process.rs:1629`) so a hang = RED** | bind; client presents {ok/wrong/overlong-no-`\n`} token | dialing client + a real child handle (cases a/b/e: any live short-lived process; **case (d) — the deadline test — needs a child that stays alive past the injected deadline**, e.g. `sleep 30`, so the timeout exercises the deadline, not `try_wait`'s fast-exit) | **(a) is the positive control** (correct token → proceed; no revert hunk — it is the green that b/d/e redden from); (b) revert **H-TOKEN** compare→wrong accepted; (d) revert **H-ACCEPT** deadline→timeout hangs (watchdog RED); (e) revert **H-TOKEN** length-cap→unbounded read hangs (watchdog RED). **(c) is an invariant, not a revert seam** — see below |
| 2c | Single-dial invariant | Rust in-proc | H-COMMIT (structural listener-close) | after a **successful** `accept_and_handshake` returns, a second `TcpStream::connect` to the same addr must fail (`ECONNREFUSED`) | dialing client | **no paired revert hunk** — the close is structural (listener moved by value, dropped on return), so this asserts the post-success invariant directly; reddens only if someone refactors the listener to outlive the handshake (e.g. stores it on the transport) |
| 3 | Reader-handoff integrity | Rust in-proc | **H-READER** | **in-test peer** writes `token+"\n"` **coalesced in ONE `write_all`** with a full frame line (the real host sends no eager frame, so this hazard is exercised by the peer, not the bundle); build read half from handshake reader; assert first `recv()`==that frame | dialing client | revert **H-READER** (rebuild read half from raw stream/fresh `BufReader`) → first frame dropped |
| 4 | Child-death / never-connected | Rust in-proc via `ensure_started_inner` injected `init` **that calls the real `spawn_into_tcp`** (the `test_race_free_ensure_started` `:2380` pattern, but with a real child + listener — H-DRAIN and H-SPAWN(a) live *inside* `spawn_into_tcp`, so the init must go through it, not hand-roll bind+spawn) | **H-ACCEPT** try_wait branch + **H-DRAIN** + **H-SPAWN(a)** + failed-spawn contract | drive `ensure_started_inner` with an `init` that binds a listener + spawns a real child that **reads its first stdin line and echoes it to stderr, then exits without dialing** (NOT the real bundle — `ensure_started` can't be redirected). Because `spawn_into_tcp` writes the generated token to that stdin, **the echoed stderr marker IS the token**; assert `Err` **carries that exact token** — this binds two hunks at once: H-DRAIN (drains ran during accept, so the dead child's stderr reached the ring) *and* H-SPAWN(a) (the token was actually written to stdin) — a bare "non-empty" check is vacuous when the child is silent and can false-pass on Deno's own noise. Also assert the error **arrives <2 s (≪ 10 s deadline)**; failed-spawn contract via **public `spawn_count()`** (unchanged — `init` errors before the `:593` bump) + a subsequent call re-running `init`; never-connected: teardown returns promptly (`spawn_into_tcp` already killed+joined its own drains) | child = real short-lived process | revert **H-ACCEPT** `try_wait` branch (deadline-only) → error waits full deadline → "<2 s" RED; revert **H-DRAIN** (drains after accept) → `recent_stderr` empty → the "carries the token" assertion RED; revert **H-SPAWN(a)** (omit the token→stdin write) → child reads EOF, echoes nothing → `recent_stderr` lacks the token → RED |
| 5 | Shutdown/Drop over TCP | Rust in-proc | **H-SHUTDOWN** | TcpTransport to in-test peer that echoes then blocks, wrapped in `watchdog`; `shutdown()` → assert peer sees the `Shutdown` frame **then** EOF & reader breaks with **NO `ProcessCrashed`**; `Drop` → `reader.join()` returns <1 s | in-test peer | revert **H-SHUTDOWN** `Shutdown::Write` → peer never EOFs → shutdown blocks (watchdog RED) / reports `ProcessCrashed` |
| 5r | Crash-reap over TCP | Rust in-proc | **H-CRASH-REAP** | in-test peer connects, then closes ONLY the socket **while the "child" stays alive** (a `child` handle whose process does not exit); reader hits `Eof` → `handle_crash`; wrapped in `watchdog(2s,…)`; assert it returns (child killed then reaped) rather than blocking on `wait()` | in-test peer + a still-alive child handle | revert **H-CRASH-REAP** (`wait()` with no prior `kill()`) → `handle_crash` blocks on the live child → watchdog RED |
| 6a | stdout garbage harmless | Integration, deno-gated | transport move (**D-MAIN**) | deno child serves protocol over TCP while writing a **known non-JSON marker** to **stdout**; assert **(i)** round-trip completes, no kill **and (ii)** the marker was observed on the stdout drain | real deno fixture | revert **D-MAIN** (wire `runHost` back to `Deno.stdin/stdout`) → stdout garbage corrupts frames. **Exercised-guard: (ii) proves the garbage was actually emitted (else vacuous under both states — sibling trap)** |
| 6b | Malformed **socket** frame fatal — **Phase 4** | Rust in-proc | **H-MALFORMED** | in-test peer sends ONE non-JSON line **on the socket**; assert immediate "channel compromised" kill + broadcast (not tolerated). **Lands in Phase 4 only**: `reader_loop` is transport-agnostic, so this is a global switch that must arrive with stdio's deletion (and with the deletion of the two leniency unit tests at `:2088`/`:2174`, which it is mutually exclusive with) | in-test peer | revert **H-MALFORMED** (re-add leniency) → single malformed line tolerated |
| 7 | `console.log` harmless | Integration, deno-gated | transport move + **H-STDOUT** | fixture engine calls `console.log("MARK")` in execute; render over TCP; assert **(i)** render succeeds **and (ii)** "MARK" observed on the stdout drain | real deno fixture engine | revert **D-MAIN** → console.log corrupts frames. **Exercised-guard: (ii) proves the engine actually logged (else vacuous)** |
| 8 | `connectControl` round-trip | **Deno-native (`.deno-test.ts`, CI-only)** | **D-CONNECT** | stand up a **real** `Deno.listen({ port: 0 })`; feed the token on a real stdin (or the module's injectable token source); call `connectControl`; assert **first bytes on the socket == `token+"\n"` (order-checked)**, then a Request round-trips via real `readFrames`/`writeFrame` over the real `Deno.Conn` | none (real loopback socket) — do **not** mock `Deno.Conn` | revert **D-CONNECT** `writeAll(token+"\n")` → "first bytes==token" RED; revert `{reader: conn.readable}` → round-trip RED. **Runs only in `ts-test-suite` CI, not `cargo xtask verify`** |
| 9 | E2E Julia over TCP | Full binary | whole stack | `cargo run --bin q2 -- render <julia fixture>`; assert success, **byte-parity vs stdio baseline**, and a tracing marker confirms TCP was used | none (real binary) | revert **H-SPAWN** (omit token-to-stdin write, or omit `--control`) → child never dials → ensure_started times out → render fails. **Exercised-guard: assert the `engine-host connected over loopback TCP` marker (emitted at handshake commit, step 7) — proves TCP was used, not a fallback** |
| 10 | Large-payload deadlock-freedom | Rust in-proc | continuous-drain over TCP (H-FRAME + demux) | **shrink the in-test peer's `SO_RCVBUF` (and the sender socket's `SO_SNDBUF`) to a small known size via `socket2` (`SockRef::set_recv_buffer_size`/`set_send_buffer_size`)**, then round-trip an Execute-shaped frame whose input **comfortably exceeds that shrunk combined buffer** (e.g. ~256 KB against buffers pinned to a few KB — a deterministic block, not reliant on Linux autotune, which would make a fixed payload against *default* buffers vacuous) **while the peer withholds reads until the sender has attempted the full write**; wrapped in `watchdog`; assert it completes | in-test peer | a `send` that holds the write lock across a socket-full blocking write (or buffers whole msg before the reader drains) → deadlock → watchdog RED |
| — | `cargo xtask verify` (gate) | gate | **WASM-GATE** | run verify | — | un-gate `ts_process.rs` / leak `std::net` into wasm → wasm build breaks |

### Refactor-induced vacuity guards (do NOT skip)

- **#3** is vacuous unless the `token+"\n"`+frame are sent in **one coalesced
  write** — sent as two writes, the kernel socket buffer retains the frame and a
  *fresh* reader passes anyway, hiding the very bug the test exists to catch. The
  coalesced write is the discriminator. (The real host sends no eager frame, so
  this hazard is exercised by the **in-test peer**, which is why #3 stays at the
  Rust in-proc tier and does not depend on the bundle.)
- **#2c** has **no revert hunk** (the listener-close is structural — moved by
  value). It is an invariant assertion, not a fail-on-revert seam; do not fake a
  hunk to "bind" it. It reddens only under a refactor that lets the listener
  outlive the handshake.
- **#4** must assert the error arrives **≪ the accept deadline** — asserting only
  "an error occurs" passes via the deadline path even if `try_wait` never fires —
  **and** must assert the error **carries the exact token on `recent_stderr`**: that
  one assertion is the H-DRAIN discriminator (drains started after accept → empty
  ring → RED) *and* the H-SPAWN(a) discriminator (no token→stdin write → child echoes
  nothing → RED). Both require #4's injected `init` to call the **real
  `spawn_into_tcp`** (where H-DRAIN and H-SPAWN(a) live) with a child that echoes its
  first stdin line to stderr — a hand-rolled bind+spawn would exercise neither hunk.
- **#5**'s discriminator is **"no `ProcessCrashed`"**, not "shutdown returned" —
  a missing half-close can still return (via the `Drop` kill) while wrongly
  routing through the crash path; only the crash-vs-graceful surface distinguishes.
- **#5r**'s discriminator is **"the still-alive child does not block `wait()`"** —
  the peer must keep the child process alive while closing only the socket;
  otherwise (child exits) a no-kill `wait()` returns anyway and the test is vacuous.
- **#6a** needs the same exercised-guard as #7(ii): assert the non-JSON marker was
  actually observed on the stdout drain. Without it, a fixture that silently skips
  the garbage write passes under **both** the correct-TCP and the reverted
  stdout-channel states — the test catches nothing (sibling trap).
- **#7**'s **(ii)** is the exercised-guard — without it the test passes when the
  engine never logs (sibling trap). Observe it via `set_global_default` (decided —
  see Rust side), **not** `with_default` (which cannot see the drain thread).
- **#10** has two vacuity conditions, both required: (1) the socket buffers must be
  **pinned small via `socket2` and the payload sized comfortably above them** (a
  fixed payload against *default* buffers can be vacuous — Linux autotunes loopback
  buffers to multiple MB — so shrink `SO_RCVBUF`/`SO_SNDBUF` to a few KB and send
  e.g. ~256 KB; then `send` deterministically blocks with no autotune dependence),
  and (2) the **peer must
  withhold reads** until the sender has attempted the full write — a fast-draining
  peer completes even a buggy whole-message-buffer `send`, hiding the deadlock.

### Missing-test pass (reasoned, per the skill)

- **Added** because the raw change-list didn't name them: #6b (socket-malformed
  *fatality*, distinct from stdout garbage — **lands in Phase 4**, since it is a
  global `reader_loop` switch), #10 (large-payload deadlock-freedom — the plan
  calls the continuous-drain property load-bearing, not an optimization), and the
  exercised-guards on **#6a(ii)** and **#7(ii)** (both prove the garbage/log was
  actually emitted, else vacuous — sibling trap).
- **Accepted-untested (rationale, not silently omitted):**
  - **Constant-timeness of the token compare.** Row #2(b) asserts only that a
    wrong token is *rejected*. Timing behavior is hygiene, not load-bearing (the
    listener accepts once, then closes), and is impractical to assert reliably.
  - **`set_nodelay` on the socket** (part of H-COMMIT). A latency optimization
    with no observable correctness surface — no seam asserts it; deliberately
    untested. (H-COMMIT's `try_clone` split is exercised implicitly by #1/#3/#5;
    only its listener-close is directly asserted, via #2c.)
  - **`main.ts` (D-MAIN) has no unit test** — it is excluded from
    `tsconfig.json`/vitest. Covered by integration rows #6a/#7 and E2E row #9 only.
  - **Bind→dial race** (a hostile local process connecting before the child): v1
    relies on the 122-bit uuid token's unguessability as sole defense, **delivered
    on stdin so a different-uid racer cannot read it** (see "Token delivery"); the
    optional pid/peer-loopback check is deferred. No deterministic way to *win* the
    race in a unit test, so there is nothing to bind — the contract is token
    entropy + private delivery.
  - **`stdout_loop` non-blocking while the child spews during teardown**:
    partially covered by #5 (Drop-join) and #7 (drain). A dedicated
    spew-during-shutdown test is accepted-untested; the `Drop` SIGKILL backstop
    bounds it. Flag for addition if parallel-Pass-2 teardown ever races.
  - **Respawn gets a fresh token/port.** After `reset_after_crash` clears `write`,
    the next `ensure_started` re-runs `init` → new `bind` → new `uuid` token. Not
    directly asserted (uuid uniqueness + a fresh bind are trusted); the crash/respawn
    machinery itself is already covered by the existing generation-guard tests.
    Logged, not tested.
  - **Concurrent `ensure_started` during the up-to-10 s TCP handshake.** The coarse
    init lock (`reader` mutex) is now held across the blocking accept, so a second
    concurrent `ensure_started` (or a `reset_after_crash`) blocks up to the deadline
    then no-ops on the double-check. The *lock discipline* is unchanged from the
    stdio path (only the hold *duration* grew), and `test_race_free_ensure_started`
    already binds the spawn-once contract — so the new long-hold behavior is
    accepted-untested (asserting "blocks then no-ops" deterministically is
    impractical). Logged so the duration change isn't mistaken for covered.
  - **`pc_c_a` (migrated >1 MB frame probe) is a property test**, like #10 — it
    asserts a large single-line frame round-trips, with no named-hunk revert. Not a
    fail-on-revert seam; kept as a standing property guard.
  - **Windows** is a *platform gate*, not a seam — the same seam tests run under
    Windows CI (Phase 3). Logged so "green on macOS" isn't mistaken for "covered."

### Probe-file disposition (frozen-assertion change)

`ts_process_framing_probe.rs` has **three** probes, disposed of **differently**:

- `pc_c_b_foreign_line_is_malformed` / `pc_c_b_prime_interleaved_bytes_corrupt_frame`
  assert **stdout-contamination tolerance** — a scenario that becomes **impossible**
  post-swap (stdout is no longer the channel). **Delete** them at Phase 3 (not
  migrate — migrating would assert tolerance of a thing that can't happen =
  vacuous), replaced by #6a (stdout garbage harmless) + #6b (socket-malformed
  fatal). Vacuity-checked: #6a's "succeeds despite stdout garbage" strictly differs
  from the pre-swap state (would corrupt).
- `pc_c_a_large_single_line_frame_parses` is **NOT obsolete** — it asserts a live,
  transport-relevant property (a **>1 MB single-line frame** round-trips through
  `read_line`'s unbounded growth **intact**), which survives the swap and is **not**
  covered by seam #10: `pc_c_a` uses a **>1 MB** frame to prove `read_line`'s
  unbounded growth returns the payload byte-intact (*framing correctness*), whereas
  #10 — now a **small** payload against deliberately-shrunk socket buffers — asserts
  *deadlock-freedom* under back-pressure. Different sizes, different assertions, so
  #10 does not subsume it.
  **Migrate it to the TCP path** (drive `TcpReadHalf` over a real in-test socket
  emitting a >1 MB newline-framed frame) — do not delete. All three share the
  `spawn_deno_eval`→`spawn_into`/`StdioReadHalf` helper, so once `spawn_into`/
  `StdioReadHalf` are deleted in Phase 4 the whole file stops compiling; the
  migration of `pc_c_a` must land **with** that deletion. (This corrects the
  earlier plan, which wrongly assumed the entire file was disposable at Phase 3.)

## Checklist

Design ratification is **complete** — handshake, token shape, malformed policy,
stdout drain, and rollout are all decided above (2026-07-08). The Phase-0 spike
is **done** (PASS). Remaining is the staged build.

### Phase 0 — spike (de-risk before TDD)
- [x] Prototype the dial-back end-to-end on macOS: q2 binds, spawns `deno …
      --control/--token`, child dials back and round-trips over TCP using the
      **real `framing.ts`** primitives. **PASS 2026-07-08** (deno 2.9.0;
      ~75 ms cold / ~27 ms warm; child-death + wrong-token failure modes clean;
      reader-handoff byte-loss hazard folded into step 6 — **but note (2026-07-22)
      the spike's throwaway child sent an eager frame and never blocked its pipes;
      the real host does neither**, which is why the drain-before-accept and
      crash-reap requirements were added after the spike, not during it). Files
      were throwaway (scratch dir); repo untouched.

The **Test Seam Spec is frozen** (2026-07-08) — every test bound to a named
revert hunk.

> **The fail-on-revert cycle** (per test, in this order): write the test → it is
> **RED from feature-absence** → implement its hunk → **GREEN** → revert the hunk
> → confirm **RED** (this proves binding) → reapply. You cannot revert a hunk
> before it exists; the revert step comes *after* green, not before.

### Phase 1 — build both transports; **production stays stdio** (plan1a-host)
- [x] Seam rows **#1–#5, #2c, #5r, #10** (all in-proc, in-test peers — no Deno side
      needed), each through the fail-on-revert cycle above (#2c is an invariant,
      no revert hunk — see vacuity guards). *(Rust TDD note: a seam naming
      `TcpTransport`/`accept_and_handshake` cannot compile until the item-2/3
      skeletons exist — so land the type/fn **skeletons** first (`todo!()` bodies),
      then write each seam so it is **RED on its assertion**, not on a missing
      symbol, then implement the hunk. #10 also needs `socket2` added as a
      `quarto-core` **dev-dependency**.)*
- [x] `TcpTransport`/`TcpReadHalf` (H-FRAME/H-READER) + `accept_and_handshake`
      (H-ACCEPT/H-TOKEN; listener closed structurally by move-by-value) in
      `ts_process.rs`.
- [x] `spawn_into_tcp`: all-three-piped spawn → **write `<tok>\n` to child stdin**
      (H-SPAWN's delivery half, bound by seam #4) → **spawn stderr+stdout drains BEFORE accept**
      (H-DRAIN) → `accept_and_handshake` → on error, kill+join own drains and
      enrich with `recent_stderr`.
- [x] Grow the `init`-closure return to the `StartedDrains` enum (`Stdio(ChildStderr)`
      vs `Tcp { stderr, stdout }`); add the `stdout_reader` field + both join sites
      (`shutdown()`, `Drop`); `stdout_loop` (H-STDOUT) **TCP-path only**;
      shutdown/Drop parity (H-SHUTDOWN — `Shutdown` frame then `Shutdown::Write`).
- [x] **H-CRASH-REAP (global):** add `child.kill()` before `wait()` in
      `handle_crash`. Verify no existing stdio test reddens (it is harmless there).
- [x] **Do NOT flip production.** `ensure_started` still spawns without
      `--control`; the TCP path is exercised only by the tests above. Every
      existing deno-gated e2e test must stay green at this HEAD.
- [x] **Not in this phase:** H-MALFORMED and seam #6b (global switch — Phase 4).

> **Phase 1 status (2026-07-22 impl):** COMPLETE — commits `5acbef437`
> (skeletons + StartedDrains wiring), `d9eff087f` (#1 H-FRAME), `0e6e61b50`
> (accept_and_handshake #2/#2c/#3), `9b99c917b` (spawn_into_tcp #4),
> `68f39d1b2` (#5 H-SHUTDOWN + #5r H-CRASH-REAP), `bda7c1bd2` (#10 deadlock).
> All seam reverts re-verified cold by the orchestrator; two vacuity defects
> found and fixed during verification (#2c weakened to a round-trip check →
> tightened to assert connect() refused; #10 socket buffers set post-connect
> → vacuous on macOS, fixed to set SO_*BUF before connect/listen + 8x-measured
> payload). `cargo nextest run -p quarto-core` = 2803 passed, 34 skipped. Full
> `cargo xtask verify` then surfaced two `-D warnings` failures that plain
> build/nextest miss (`StartedDrains::{None,Tcp}` dead in the non-test lib build
> until the Phase-3 flip → `#[allow(dead_code)]` + staging comment;
> `local_addr().map(..).unwrap_or(0)` → `map_or`); after those, full verify is
> green at HEAD (workspace nextest 10752 tests + ts-packages + hub-client WASM
> build/tests — WASM-GATE holds). Production stays stdio; no `--control` yet.

### Phase 2 — Deno dial-back (Plan 1b) — still no production flip
- [x] Seam row **#8** (`connectControl` over a **real** loopback socket in a
      `.deno-test.ts`) via the fail-on-revert cycle. **Not a vitest test** — see
      Coverage honesty. Added a `deno test` step to the `ts-test-suite` CI
      workflow (not `cargo xtask verify`, per Coverage honesty) — CI-gated,
      as documented.
- [x] Write `connectControl()` in a **new Deno-only module `control-transport.ts`**
      (from scratch; D-CONNECT) — **added to `tsconfig.json`'s `exclude`** (NOT in
      `framing.ts`); it reads the token from **stdin** (only `--control` from
      `Deno.args`), dials, `setNoDelay`, `writeAll` the token pre-line on the
      socket. `main.ts` channel selection wires it into `runHost` (D-MAIN). (No
      vitest for either module — both reference `Deno.*`.)
- [x] Rebuild the embedded bundle (`cargo xtask build-engine-host-bundle`) +
      `cargo build`. The bundle now *understands* `--control` but never gets it.

> **Phase 2 status (2026-07-23 impl):** COMPLETE — commit `3e45f8c1a`.
> `connectControl` (D-CONNECT) written from scratch in a new Deno-only
> `control-transport.ts`, excluded from `tsconfig.json`; `main.ts` (D-MAIN)
> selects the TCP branch only when `--control` is present in argv, stdio
> otherwise. Seam #8 (`control-transport.deno-test.ts`) drives a real
> `Deno.listen({ port: 0 })` — no mock `Deno.Conn` — and both fail-on-revert
> bindings were demonstrated locally: reverting the token pre-line write
> reddens the "first bytes == token" assertion (bounded timeout, ~3s), and
> reverting `{ reader: conn.readable }` (an empty/fresh `ReadableStream`
> instead) reddens the round-trip assertion. `deno test --allow-all
> --sloppy-imports` is the flag set that works (the module's `types.ts`
> import pulls in `@quarto/types`'s `.js` internal specifiers, same reason
> `wire-parity.deno-test.ts` needs it). Also end-to-end smoke-tested the
> rebuilt bundle directly (`deno run … dist/engine-host-deno.js --control
> 127.0.0.1:<port>` against a real `nc -l` listener): the token was
> observed as the first line on the wire. `npm run test -w
> @quarto/engine-host-deno` (115 vitest tests) and `npm run typecheck -w
> @quarto/engine-host-deno` stay green — confirms `control-transport.ts` is
> excluded from the tsc/vitest graph, not silently pulled in. `cargo build
> --bin q2` re-embeds the rebuilt bundle. Production stays stdio; no
> `--control` yet (Phase 3).

### Phase 3 — flip production to TCP, then validate (incl. Windows)
- [x] **FIRST — capture the stdio byte-parity baseline, BEFORE the flip.** DONE on the
      pre-flip HEAD (`aa8c314f1`): rendered a minimal `engine: julia` doc (`1 + 1`,
      `daemon: false`) via `cargo run --bin q2 -- render` with
      `QUARTO_JULIA_PROJECT=$HOME/Library/Caches/quarto/julia`. Golden saved to scratch
      (`seam9-stdio-baseline.html`, sha256 `809099c1…`; contains `cell-output` +
      `<code>2</code>` — julia executed over stdio). Confirmed DETERMINISTIC (identical
      sha on re-render), so byte-parity is a sound seam-#9 assertion.
- [x] **Flip:** DONE (commit `ee5c312c5`). `ensure_started`'s init closure now binds an
      ephemeral loopback `TcpListener`, generates a uuid token, passes
      `--control 127.0.0.1:<port>` on argv, and routes through `spawn_into_tcp` (token
      delivered on stdin). `StartedDrains::Tcp` is now the live production variant;
      dead-code comment updated (`::None`/`::Stdio` now the test-only variants).
      **Event-collision fix (commit `3cfe190a7`, per Gordon's AskUserQuestion decision):**
      each spawn now fires two `target:"engine_host"` events (`"engine-host spawned"` +
      the `"engine-host connected over loopback TCP"` marker); the 3 pre-existing
      count-by-target tests (echo `j9`, julia `j6`×2) were fixed to count only the
      `"engine-host spawned"` **message** (via a `MsgVisitor`) — production marker
      target kept as `"engine_host"` (plan step-7 literal preserved).
- [x] Integration seam rows **#6a** (stdout garbage harmless) + **#7**
      (console.log harmless, with the exercised-guard) green. DONE — commit
      `7245b8595` (impl by sonnet; both fail-on-revert bindings COLD-VERIFIED by
      the orchestrator — see the Phase-3 status note). Tests
      `p3_6a_stdout_garbage_harmless` + `p3_7_console_log_harmless` in
      `echo_engine_e2e.rs`; sentinel-gated 20-line stdout bursts in
      `echo-engine.ts` (`QUARTO_ECHO_STDOUT_GARBAGE` / `QUARTO_ECHO_CONSOLE_LOG`,
      sized above `MAX_CONSECUTIVE_MALFORMED_LINES=5` so a stdio revert
      escalates→kills), observed on the `stdout_loop` drain via
      `set_global_default`.
- [x] Delete the two obsolete `ts_process_framing_probe.rs` stdout-contamination
      probes (`pc_c_b*`). DONE — commit `21df9d612` (also dropped the now-unused
      `RecvError` import and cleared the stale `#[allow(dead_code)]`/"not yet
      called" comment on `stdout_loop`, live production since the flip).
      **`pc_c_a` left in place** — it still uses `spawn_into` and is migrated at
      Phase 4 (see Probe-file disposition).
- [x] E2E seam row **#9**: DONE (orchestrator, manual + recorded). Re-rendered the SAME
      julia project over the flipped TCP path (`RUST_LOG=engine_host=info`):
      `RENDER_EXIT=0`; the TCP marker fired —
      `INFO engine_host: engine-host connected over loopback TCP port=62760` (proves TCP
      was used, not a stdio fallback); output **byte-identical** to the stdio golden
      (same sha256 `809099c1…`). Revert binding cold-verified: removing the `--control`
      args → rebuild → render FAILS in 9s (`engine-host child exited before dialing back
      over loopback TCP`) → restore → green.
- [ ] **Windows CI** exercises the loopback + accept-poll + token path. **REMAINING**
      (cannot validate on macOS — the `set_nonblocking(false)` accepted-socket trap is
      Windows-only; runs when CI picks up the branch).
- [x] `cargo xtask verify` green (WASM-GATE: `ts_process.rs` stays wasm-gated).
      DONE at Phase-3 HEAD `21df9d612`: full `cargo xtask verify` (Rust build +
      workspace nextest + ts-packages + hub-client build/tests + WASM) →
      **"✓ All verification steps passed!"**, exit 0. WASM-GATE holds (`stdout_loop`
      is not flagged dead — it is live production; the only "never used" warnings
      are pre-existing `pass2_*` helpers, untouched by this phase). Log:
      `.superpowers/sdd/p3c-verify.log`.

> **Phase 3 status (2026-07-23): COMPLETE except the Windows platform gate.** The
> production flip (`ee5c312c5`), event-collision fix (`3cfe190a7`), byte-parity
> baseline (P3a), E2E seam #9, seams #6a/#7 (`7245b8595`), and the `pc_c_b*` probe
> deletion + `stdout_loop` cleanup (`21df9d612`) are all DONE and orchestrator-verified.
> Full `cargo xtask verify` at HEAD `21df9d612` = green ("All verification steps
> passed!"; WASM-GATE holds).
>
> **Seams #6a/#7 fail-on-revert — both bindings COLD-VERIFIED by the orchestrator, and
> the plan's stated revert was corrected.** The frozen seam table lists the #6a/#7
> revert as "D-MAIN (rewire `main.ts` back to stdio)". In the POST-FLIP world that is
> **not** a valid discriminating revert: reverting only `main.ts`→stdio while the Rust
> side still dials TCP produces a transport MISMATCH ("child exited before dialing back
> over loopback TCP"), which reddens **every** echo test vacuously — regardless of the
> garbage. The correct discriminating revert is the **whole stdio world**:
> `git checkout 3e45f8c1a -- crates/quarto-core/src/engine/ts_process.rs` (revert the
> Rust flip; the dual-branch `main.ts` then self-selects stdio, so **no bundle rebuild
> is needed**). Verified:
> - **Binding A (assertion (i) ↔ the flip):** under the stdio revert, the control test
>   `p3_1a_language_claim_executes_echo` **PASSED** (the stdio world renders normally),
>   while `p3_6a`/`p3_7` **FAILED** with the garbage-attributable error *"engine-host
>   protocol error: 6 consecutive non-JSON lines on stdout — channel considered
>   compromised … (last line: ECHO_STDOUT_GARBAGE_MARKER not-json line 5)"*. Control
>   green + garbage red in the SAME reverted world ⇒ the RED is attributable to the
>   garbage, not the revert. Restored → green.
> - **Binding B (exercised-guard (ii) ↔ H-STDOUT / `stdout_loop`):** neutralizing
>   `stdout_loop`'s `info!` forward reddened both tests exactly at assertion (ii) (marker
>   never observed on the drain; only the two `engine_host` lifecycle markers captured)
>   while (i) still passed (render succeeded over TCP). Restored → green. Tree clean
>   after each revert.
>
> **Remaining for Phase 3:** Windows CI only (the `set_nonblocking(false)`
> accepted-socket trap is Windows-specific; fires when CI picks up the branch).

> **Phase 4 status (2026-07-23): COMPLETE.** Task 4-A (seam #6b + H-MALFORMED,
> commit `bd1af5c55`), Task 4-B (delete stdio transport + all consumers on
> loopback TCP, commit `a66b7f12b`), Task 4-C (retire the stdout-contract docs,
> commit `a10e64174`) all landed. Final full `cargo xtask verify` GREEN at HEAD
> `a10e64174` (workspace nextest 10751 passed / 198 skipped; ts-packages +
> hub-client WASM build/tests green; WASM-GATE holds). Windows CI is the only
> remaining gate.

### Phase 4 — hard-swap cutover (delete stdio + make malformed fatal)
- [x] Seam row **#6b** (malformed socket frame fatal) via the fail-on-revert cycle.
      DONE — commit `bd1af5c55` (`test_malformed_frame_is_fatal`, MockTransport +
      one malformed line → `Err(Other)` + `shutting_down`). Binding COLD-VERIFIED
      by the orchestrator: re-adding the log-and-skip `continue` → the in-flight
      request hangs → `watchdog` DEADLOCK at 10.4s → RED; restored → green.
- [x] H-MALFORMED: delete the `MAX_CONSECUTIVE_MALFORMED_LINES` leniency **and**
      the two `#[cfg(test)]` tests that bind it
      (`test_stray_lines_below_bound_are_skipped_not_fatal`,
      `test_malformed_beyond_bound_escalates_distinct_from_crash`). DONE — commit
      `bd1af5c55`. `reader_loop`'s `Malformed` arm is now immediately fatal (a
      single malformed frame on the private control socket → broadcast+kill); the
      counter + constant are gone (only prose-comment mentions remain); message
      reworded to name the control socket. `ts_process` module 40/40 green;
      `echo_engine_e2e` unchanged 13/13.
- [x] **Migrated `pc_c_a` to the TCP path** — commit `a66b7f12b`. Moved
      in-crate as `tests::test_large_single_line_frame_parses_over_tcp` (a >1 MB
      frame over `accept_and_handshake` + a dialer thread; recv-before-join since
      a >1 MB write blocks the dialer until the reader drains). Standing property
      test, no named revert hunk (like #10). `ts_process_framing_probe.rs` deleted
      (its last probe migrated in-crate; `TcpReadHalf`'s field + `accept_and_handshake`
      are private, so an external integration test cannot construct the read half).
- [x] Deleted `StdioWriteHalf`/`StdioReadHalf`/`spawn_into` — commit `a66b7f12b`.
      - Production (`ensure_started`) was already on `spawn_into_tcp` (Phase 3).
      - `TsEngineHost::start_with_command` (`#[cfg(test)]`) ported to the
        loopback-TCP handshake (bind listener + uuid token + append
        `--control 127.0.0.1:<port>` + `spawn_into_tcp`). Its **7** call sites in
        `ts_process.rs` + `registry.rs` now use the new `#[cfg(test)]`
        `deno_dialback_child` helper (a `deno run <tempfile>` child that reads the
        token off stdin, `Deno.connect`s, presents the token pre-line, then runs a
        per-test body). `deno eval` does NOT forward `--control` to `Deno.args`
        (verified empirically) — hence `deno run <tempfile>`, with the tempfile
        kept alive until `start_with_command` returns. The five previously-non-deno
        children (sh/sleep) became deno dial-back children + gained an
        `is_available()` gate and 30s watchdog; CI always installs deno for this
        suite so no coverage is lost. `behave_engine_e2e.rs` needed no work
        (production `ensure_started`, flipped at Phase 3).
      - `StartedDrains::Stdio` removed; `::None` now `#[cfg(test)]`-gated, so the
        non-test lib build has only the live `::Tcp` variant and needs no
        `#[allow(dead_code)]` — the `-D warnings` leg is green.
- [x] Delete the stdout-contract language across plan1a-host / plan1b /
      `engine-host-concurrency.md` — commit `a10e64174` (Task 4-C). The
      grand-plan overview needed no change (verified: no engine-host
      stdout/protocol-channel content there). engine-host-concurrency.md's
      "Deferred: Phase 1.6" section renamed + marked LANDED; the
      console.log-corrupts / stdout-is-the-channel contract retired across all
      three docs (past-tense framing for the historical v1 design).
- [x] Re-run `cargo xtask verify` (final gate) — **GREEN** at HEAD `a10e64174`
      ("All verification steps passed!", exit 0): workspace nextest 10751
      passed / 198 skipped; ts-packages built; hub-client WASM build + all
      vitest suites green (WASM-GATE holds — `ts_process.rs` is
      `cfg(not wasm32)`, so the WASM leg is structurally unaffected).
      **Windows CI is the only remaining gate** (platform gate, fires on CI).

## Open questions (remaining)

None blocking. Everything below is a tunable or a Phase-4 confirmation, not a
fork.

- **Accept deadline value** — settled at ~10 s (spike measured ~75 ms cold /
  ~27 ms warm, so ~130× headroom). Injectable so tests use a short deadline.
  Held under the coarse init lock (so a hung child stalls concurrent spawns for up
  to the deadline — acceptable; normal path is ~75 ms). Revisit only if a
  cold-cache CI box ever trips it.
- **Phase-4 `start_with_command`** — the *only* open question is porting its ~8
  in-crate call sites to the TCP handshake (they spawn stdio-only children).
  `behave_engine_e2e` is **not** a consumer (resolved 2026-07-22 — it uses the
  production `ensure_started` path, so it flips automatically at Phase 3). Not a
  design fork.
- **Local coverage for D-CONNECT** — decide whether to add a `deno test` step to
  `cargo xtask verify` (the deno-test tier is otherwise CI-only). Not blocking;
  a convenience call.
- **Sandbox-denied environments** — after Phase 4 there is no stdio fallback, so a
  seccomp'd / hardened-container / App-Sandbox context that forbids loopback
  `bind` would break the engine host outright (today it needs no sockets). No
  known such deployment target for q2; logged so it isn't a surprise. Not blocking.

*Resolved 2026-07-08:* token = transport **pre-line**, not a protocol frame (this
is what preserves "no protocol-type change"); rollout = **staged hard-swap** with
the production flip pinned to **Phase 3** (cannot precede the bundle); malformed
-fatal is a **global** switch, so it lands at the **Phase-4** cutover with stdio's
deletion; token uses **`uuid`** (existing dep, `daemon.rs` precedent) so there is
**no new crate**; Plan 5 pooling needs no transport-lifecycle change since the
persistent `TcpStream` outlives renders like a persistent pipe.

*Resolved 2026-07-22:* token **delivered on stdin** (not argv — closes the
different-uid `ps`/`cmdline` leak); `handle_crash` **kills before wait**
(socket-EOF ≠ process-exit); drains spawned **before** the accept (deadlock +
stderr-for-error); `connectControl` in a **new Deno-only module**, D-CONNECT via
**`.deno-test.ts` (CI-only)**; `pc_c_a` **migrated** to TCP; single 1.6 security
property clarified (**channel integrity, not extension sandbox**; `--allow-net`
narrowing is a later, enabled-but-separate change).

## References

> **Line numbers throughout this plan are pinned to a base branch tip (~`afaed2c96`);
> sibling plans (plan6, plan1c3) shift some. Resolve every citation by *symbol name*
> (grep), not by line — the symbols were all verified present in two independent
> code-review passes.**

- `claude-notes/designs/engine-host-concurrency.md:195` — canonical deferred note
- `claude-notes/plans/2026-04-16-plan1a-host.md:1193`, `:212`, `:571`, `:1448`
- `claude-notes/plans/2026-04-16-plan1a-protocol.md:388` (Phase 1.5 origin of the numbering)
- `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md:1473-1477` (Phase 1.6 doc comment; `connectControl` written from scratch)
- `crates/quarto-core/src/engine/ts_process.rs` — `EngineTransport`/`EngineReadHalf` (`:169-193`), `spawn_into` (`:319-361`), demux/crash (`:1053-1229`), malformed (`:435-440,1087-1164`), stderr_loop (`:1237-1267`)
- `ts-packages/quarto-engine-host-deno/src/{main,framing,host,deno-host}.ts` (reader = `ReadableStream<Uint8Array>`; `FrameWriter` only; `runHost` is reactive — no eager frame; `connectControl` goes in a NEW Deno-only `control-transport.ts`, NOT `framing.ts`)
- `crates/quarto-core/src/engine/jupyter/daemon.rs:111` — per-session `uuid` key precedent (note: port allocation is delegated to `runtimelib::peek_ports`; it does not itself hold a bound listener)
- `ts-packages/quarto-hub-mcp/src/auth/loopback.ts:169-179` (constant-time compare) + `:209-239` (settle-once teardown) precedents
- `crates/quarto/src/commands/preview.rs:273-337` — connect-*retry* probe; its doc comment documents tokio's bind-backlog property (weaker precedent than the `std::net` `bind→listen` guarantee we actually rely on)
- `crates/quarto-core/tests/integration/ts_process_framing_probe.rs` (the leak probes to invert)
- `~/src/quarto-julia-engine/src/julia-engine.ts` (worked example; TCP+key precedent)
