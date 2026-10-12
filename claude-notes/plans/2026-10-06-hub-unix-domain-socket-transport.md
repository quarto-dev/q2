---
title: 'Plan: Unix domain socket transport for the Quarto hub sync server'
date: 2026-10-06
description: 'Runs q2 hub exclusively over a filesystem Unix domain socket for the trusted-proxy deployment, as an opt-in alternative to the TCP+WebSocket transport. (Hub-to-hub UDS peering was descoped.)'
---

# Plan: Unix domain socket transport for the Quarto hub sync server

Date: 2026-10-06
Status: Complete (all four phases done or explicitly descoped).
**The client/peering work was descoped on 2026-10-12** (see the
decision note after Phase 2): UDS mode is server-side only —
no shipped UDS client, no `--peer unix:///…`, no announce-policy
change. The shipped feature is the UDS listener for the trusted-proxy
shape (a same-host TLS-terminating proxy fronts the socket); clients
never dial UDS.

Runs `q2 hub` exclusively over a filesystem Unix domain socket (UDS) for
hub-to-hub peering, as an explicit alternative to the existing TCP+WebSocket
transport (default unchanged: TCP+WS only; never both at once).
**Scope: Linux + macOS, filesystem sockets** — no abstract namespace
(rejected, see Security analysis), no Windows (`cfg(unix)`-only feature).

## Overview

Adding a UDS transport is **architecturally easy** — every seam we need
already exists:

- **Server:** samod's `AcceptorHandle` exposes generic `accept(Transport)`,
  `accept_tokio_io(io)`, and `accept_tungstenite(ws)`; axum 0.8 even implements
  `Listener for tokio::net::UnixListener`, so the *entire existing HTTP router*
  (including `/ws` and `/health`) can be served over a UDS unchanged.
- **Rust dialer:** the `BearerDialer` precedent shows a custom samod `Dialer`
  is ~100 lines; the `UnixDialer` used for hub↔hub peering is smaller, and
  (unlike Node) it can verify the server uid via `peer_cred()` after connect.
- **Node clients: untouched.** TS clients never dial UDS — see Client
  side for the rationale.

The hard parts are **not** the transport: they are (1) the **auth model**
(filesystem permissions are the kernel-enforced floor, plus a `SO_PEERCRED`
uid check at accept as defense-in-depth), and (2) **socket-path lifecycle**
(stale files, `sun_path` limits, where the socket lives).
**Scope is Linux + macOS** — both platforms fully supported by tokio,
Node, and axum, and both are dev/CI machines, so the feature is
end-to-end testable locally. Recommended shape: serve the full axum
router over a UDS listener (WebSocket-over-UDS, not a new wire protocol)
as an **exclusive, opt-in alternative** to the TCP listener — exactly one
inbound transport per hub; the default stays exactly as today (TCP+WS
only). UDS connections are unauthenticated local-trusted (filesystem
permissions + a peer-uid check are the access control). **As shipped,
no UDS client exists** — the `UnixDialer`/`--peer unix:///…` peering
work was descoped (see the decision note after Phase 2); `q2 mcp`, hub-client,
and the preview SPA stay on TCP+WS. The deployment shape is a same-host
TLS-terminating reverse proxy fronting the socket (group permissions
plus a uid allowlist, see Auth model); access control lives at the proxy.

## Checklist

Test specifications land first (TDD); each implementation item follows
its failing test.

### Phase 1 — Tests (write first, watch fail)

Parity strategy (see Wire-parity contract): **the same test bodies run
over both transports** — fixtures get a transport knob, not mirrored
copies (mirrors drift).

- [x] Rust (`cfg(unix)`): `TestHubBuilder` gains a `Transport::{Tcp, Unix}` arm (the fixture already builds the router before binding the listener); a new WS-handshake + document-sync test is written once and runs over both transports from a tempdir socket (keep paths short for `sun_path`). UDS-mode HTTP probes use hyper or raw-socket HTTP — reqwest cannot dial a unix socket; WS-level tests use tokio-tungstenite; the UDS sync arm dials via the test-only `UnixWsDialer` in support.rs
- [x] Rust: stale-socket reclaim (dead file → probe → unlink → bind); live-socket refusal ("a hub is already running")
- [x] Rust: permission bits (`0700` dir, `0600` socket); socket file removed on graceful shutdown
- [x] Rust: `peer_cred()` accept check — same uid accepted; mismatched-uid rejected via a `setuid`/`nobody` helper (CI-only); with `--socket-allow-uid`, the allowlisted uid is accepted and others are still rejected
- [x] Rust: trusted-proxy permissions — with `--socket-group`, parent dir lands `0750` and socket `0660`, group-owned by that group (startup fails loudly if the hub cannot chgrp); without it, `0700`/`0600` unchanged
- [x] Rust: CLI validation — bare `--socket-path` resolves to the convention path in project and standalone modes; `--socket-path` + `--oidc-client-id` errors (hub-enforced auth over UDS out of scope for v1); `--socket-path` + `--behind-tls-proxy` accepted (inert without OIDC on any transport); `--socket-path` on Windows errors (runs everywhere); no TCP listener in UDS mode (connect to host:port refused)
- [x] TS: hub-mcp `e2e-auth.test.ts` sync-flow bodies extracted into `e2e-sync-flows.ts` (keeps the suite lean). No TS UDS tests — TS clients never dial UDS

### Phase 2 — Server (`crates/quarto-hub`)

- [x] `--socket-path [path]` CLI flag (optional value) + `HubConfig.socket_path: Option<PathBuf>`; bare flag resolved to the convention path during config assembly; plus `--socket-group <group>` and repeatable `--socket-allow-uid <uid>` for the trusted-proxy shape
- [x] Listener selection: `UnixListener` behind the peer-uid-checking `Listener` newtype (accept iff the peer uid is the hub's own or is in `--socket-allow-uid`); `axum::serve` over it; TCP bind skipped; "listening" log line reports the socket path
- [x] Socket lifecycle: connect-probe-then-unlink; parent dir `0700` + socket `0600` set explicitly by default, `0750` + `0660` group-owned by `--socket-group` when given; unlink on graceful shutdown
- [x] `sun_path` guard: hashed `${TMPDIR}` fallback + `hub.sock.path` pointer file (`0600`)
- [x] `validate_tls_config` treats UDS as secure-local; `--socket-path` + `--oidc-client-id` fails CLI validation (hub-enforced auth over UDS out of scope for v1); `--behind-tls-proxy` stays accepted — inert without OIDC on any transport
- [x] Audit/access-policy identity for UDS peers keyed by peer uid — the hub's own uid logs as "local" (direct), an allowlisted uid as "proxied" — with the uid logged
- [x] CI: run the cross-uid accept tests — one leg (or a `sudo` step) with `Q2_TEST_SETUID=1` so `peer_cred_other_uid_rejected_unless_allowlisted` exercises its helper; it skips loudly everywhere else

**Client/peering work — descoped (2026-10-12, bd-9mpnaslv).** There is
no shipped UDS client and no `--peer unix:///…`. Implementation
surfaced that hub↔hub peering cannot sync documents at all under the
hub's `NeverAnnounce` repo policy: samod's `should_announce` gates not
only announcements but also `Repo::find`\'s outbound requests
(`NotSentDueToAnnouncePolicy` in the request phase), so a peered
connection handshakes and then carries nothing — over UDS *and* over
TCP (TCP peering has been equally inert since #36). Making peering
flow would require an announce-policy exemption for peer connections,
and because samod peer IDs are unauthenticated (the trait docs warn
the network layer must authenticate them), that exemption is only
sound where the transport authenticates the peer. Rather than grow
that surface for a feature whose only consumer was the peering flag
itself, the work was reverted in full; `peer.rs` keeps only a guard
rejecting `unix:///` peer URLs with a clear warning (a tungstenite
dial would otherwise retry silently forever). TS clients never dial
UDS either: no `socketPath` in sync-client, no `unix:///` in
hub-mcp's `ConnectionManager`, no client discovery — `q2 mcp` stays on
TCP+WS. If UDS peering is ever revived, it needs a peer-authenticated
request channel (e.g. the samod fork could split announce/request
gating).

### Phase 3 — Docs and end-to-end

(No client discovery: the `hub.sock.path` pointer the hub writes under
the `sun_path` guard is operator-facing documentation of the fallback
path.)

- [x] `docs/` page for UDS mode (`docs/guides/hub/unix-socket.qmd`): the trusted-proxy deployment (nginx recipe with `proxy_pass http://unix:…`, WS upgrade headers, `--socket-group`/`--socket-allow-uid`), the convention paths and the `hub.sock.path` pointer semantics, the access-control model; the page and the `--socket-path` help text state that browsers and `q2 mcp` cannot connect in UDS mode (MCP stays on TCP+WS) except through an operator-configured proxy
- [x] End-to-end: a real hub binary serves HTTP and sync over its socket and binds no TCP listener; invocation + observed output recorded in this file
- [x] `cargo xtask verify` green — with two recorded caveats (2026-10-12):
  the hub-build leg was driven by rebuilding the WASM bundle first
  (`npm run build:wasm -w hub-client`; the tree's bundle was stale
  relative to main's native goldens, which fails the `test:wasm`
  golden-parity tests regardless of this branch), and the Rust-test
  leg was run separately (`cargo nextest run --workspace`: 16113
  passed, 2 failed — `knitr_doc_echo_false_hides_source_keeps_output`
  and `knitr_cell_echo_false_still_hides_source`, which fail
  identically on pristine HEAD and are tracked in bd-nn2fou8h), with
  `verify --skip-hub-build --skip-rust-tests` covering every other leg.

## End-to-end record (2026-10-12)

Real `hub` binary (debug build at 495a4fad7 + Phase 1/2/4 changes),
macOS arm64, scratch project with one `index.qmd`:

```bash
hub --project …/e2e/proj --socket-path …/e2e/A.sock
```

Observed (inspected):

- `curl --unix-socket A.sock http://localhost/health` →
  `{"status":"ok","project_root":"…/e2e/proj","qmd_file_count":1,"index_document_id":"eumVJjSdZY8VjhMgt2kNFK6Qv2k"}`
  — the full HTTP router answers over the socket.
- `nc -z 127.0.0.1 3000` → refused (exit 1): no TCP listener in UDS mode.
- Socket file `srw-------` (0600) in its `0700` parent dir.
- Sync over the listener: the transport-parity test runs the same
  WS-handshake + document-sync body over TCP and over a UDS fixture
  (two in-process client repos, content round-trip asserted):

  ```
  $ cargo nextest run -p quarto-hub --test integration uds_client
  PASS uds_client::ws_handshake_and_doc_sync_over_tcp
  PASS uds_client::ws_handshake_and_doc_sync_over_unix_socket
  ```

  plus the real-binary subprocess suites in `uds_server.rs`
  (stale-socket reclaim, live-socket refusal, permission bits,
  graceful-shutdown cleanup, peer-uid accept checks, CLI validation,
  no-TCP-listener).

## Motivation

Why UDS is worth having for same-machine hub↔hub peering (TCP+WS stays
for remote/browser, and for `q2 mcp`):

1. **Security: closes the drive-by-local-exploitation class.** A no-auth hub
   on `127.0.0.1:<port>` is connectable by *any* local process — including a
   web page open in a browser (`ws://127.0.0.1:3000` is reachable from
   JavaScript; the hub's `check_ws_origin` only runs for Cookie auth, and a
   no-auth hub does no origin check at all). A UDS in a `0700` directory is
   unreachable from browsers and from other uids, full stop.
2. **Zero network exposure.** A UDS-mode hub binds no TCP socket at all:
   nothing to port-scan, no loopback interface to reach, no possibility of
   a cross-origin or remote connection of any kind. This is the strongest
   available posture for a local-only sync hub.
3. **No port coordination.** `q2 hub` per project currently needs a free port
   and peers need to be configured with it. A well-known socket path under
   `<project>/.quarto/hub/` is deterministic — no collisions, multiple
   concurrent hubs for free.
4. **Kernel-enforced access control.** Filesystem permissions (`0600`
   socket in a `0700` dir, ≈ same-uid-only — the same trust level as
   reading the user's keyring) are enforced by the kernel even if the
   application forgets every check; a `SO_PEERCRED`/`getpeereid` uid check
   at accept adds defense-in-depth and audit logging.
5. **Testable on dev machines.** Filesystem sockets work identically on
   Linux and macOS — unit tests and the full two-hub peering e2e run on
   developer laptops, not just Linux CI.

Non-goals / unaffected: hub-client and the preview SPA are browsers — they
**cannot** use UDS and stay on TCP+WS forever. Local `q2 mcp` also stays on
TCP+WS — TS clients get no UDS dial (see Client side). Remote MCP (CAP-16
Streamable HTTP) is orthogonal.

## Current architecture (as found)

### Server: `crates/quarto-hub`

- Axum 0.8 + tokio. `server.rs` binds
  `TcpListener::bind(format!("{}:{}", config.host, config.port))` and hands
  the router to `axum::serve`. `HubConfig` (`context.rs`) carries
  `host`/`port`; CLI flags `--host`/`--port` in `main.rs`.
- WS path: `ws_handler` (`server.rs`) extracts a Cookie or Bearer
  credential, runs `check_ws_origin` (Cookie only, secure mode), validates
  the JWT (`authenticate_credential`), then `ws.on_upgrade(handle_websocket)`
  → `ctx.acceptor().accept_axum(socket)`.
- Auth is checked **once, at upgrade time**, inside the axum handler — i.e.
  *before* samod sees anything. That means a UDS listener can carry its own
  auth policy without touching samod.
- Peers (`--peer`) dial out with samod's `dial_websocket` (`peer.rs`).

### samod (quarto-dev fork, `access-policy` branch) — the key enabler

`samod::Transport::new(stream, sink)` is a fully generic byte stream/sink.
`AcceptorHandle` public API:

| Method | Input | Framing |
|---|---|---|
| `accept(Transport)` | any `Stream<Vec<u8>>` + `Sink<Vec<u8>>` | caller's choice |
| `accept_tokio_io(io)` | any `AsyncRead + AsyncWrite` | length-delimited (tokio-util `LengthDelimitedCodec`) |
| `accept_tungstenite(ws)` | any tungstenite WS stream/sink | WebSocket |
| `accept_axum(ws)` | axum `WebSocket` | WebSocket (current path) |

Dial side: the `Dialer` trait (`connect() -> Result<Transport, DialError>`),
with built-ins `dial_websocket` and `TcpDialer` (length-delimited). The
repo already ships a custom dialer: **`BearerDialer`**
(`crates/quarto-hub-provider/src/dialer.rs`) — reimplements the tungstenite
client connect to add an `Authorization` header, ~120 lines, and maps frames
with the same `ws_to_bytes` semantics samod uses internally.

### axum 0.8 supports UDS serving out of the box

`axum-0.8.9/src/serve/listener.rs:46`: `impl Listener for
tokio::net::UnixListener`. So `axum::serve(unix_listener, router)` serves the
**entire existing router** — `/ws`, `/health`, `/auth`, static — over a UDS
with zero router changes.

### Node clients

- `ts-packages/quarto-sync-client`: `buildWsAdapter` (`client.ts`) picks
  `StoppableWebSocketClientAdapter` (browser) or `NodeWebSocketClientAdapter`
  (Node, Bearer via headers). There's also a module-level
  `setNetworkAdapterWrapper()` seam.
- `NodeWebSocketClientAdapter` constructs `new WebSocket(url, [], { headers })`
  through a `WebSocketFactory` test seam. TS clients get no UDS dial in
  this plan (see Client side); recorded here for reference in case that
  is ever revisited: the `ws` package (8.21.0) has **no working
  `socketPath` client option** — `initAsClient` resets it to `undefined`
  *after* spreading user options (`websocket.js:681`), so
  `new WebSocket(url, { socketPath })` silently dials TCP
  (`ECONNREFUSED localhost:80`). What works is the native
  **`ws+unix://<socket path>:<request path>`** URL spelling (parsed at
  `websocket.js:807-812` into `opts.socketPath` + `opts.path`), or a
  custom `createConnection` passed through options.
- `ts-packages/quarto-hub-mcp`: `ConnectionManager` takes `--server` /
  `QUARTO_HUB_SERVER` (a `ws://`/`wss://` URL), probes `/health` (converted
  via `toHttpUrl`) to classify `no-auth` vs `requires-auth`, enforces
  loopback rules (`isLoopbackHost`), then `createSyncClient` with a
  `getBearer` callback.

### Platform facts

- **Scope: Linux + macOS, filesystem sockets.** tokio's
  `UnixListener`/`UnixStream` are `#[cfg(all(unix, feature = "net"))]` —
  both platforms covered. Windows has no tokio UDS support; the CLI flag
  fails fast there with a clear error.
- **Peer credentials:** tokio's `UnixStream::peer_cred()` works on Linux
  (`SO_PEERCRED`) **and** macOS (`getpeereid`, see `impl_macos` in
  `tokio-1.53.1/src/net/unix/ucred.rs:54-62`) — the same accept-time uid
  check compiles on both.
- **Node:** `net.connect({ path })` and `ws`\'s `socketPath` support
  filesystem sockets on Linux and macOS alike (this is Node's classic,
  fully-supported UDS path — no version caveats).
- **axum:** `impl Listener for tokio::net::UnixListener` serves the full
  router over a filesystem socket unchanged.
- **`sun_path` limit: 104 bytes on macOS** (108 on Linux). Deep project
  paths (`<project>/.quarto/hub/hub.sock`) can exceed it — needs a length
  guard with a short fallback path (see Server side).

## Design options

### Option A — Full axum router over UDS (WebSocket-over-UDS) — RECOMMENDED

A UDS listener **replacing** the TCP listener for that hub process (chosen
per invocation, never both at once); the router, `ws_handler`, auth
plumbing, and samod accept path are all reused verbatim. Wire protocol
stays HTTP+WS — the identical wire format, so clients differ only in how
the byte stream is obtained.

- Server: bind `tokio::net::UnixListener` on the socket path and
  `axum::serve(listener, router)` instead of the TCP listener. The only new
  hub code is listener selection, socket-file lifecycle, and the
  accept-time peer-uid check (see Auth model).
- Rust dialer (the only shipped one): `tokio_tungstenite::client_async(request,
  UnixStream::connect(path)?)` — a ~60-line `UnixDialer` modeled on
  `BearerDialer`, minus the header injection, plus a `peer_cred()` server-uid
  check after connect. Used by `--peer unix:///…` (hub↔hub) and reused by
  quarto-hub-provider.
- Node clients: no UDS capability — TS clients never dial UDS (see
  Client side).
- Works with `curl --unix-socket <path>` for debugging; `/health` and
  future HTTP endpoints come along for free.

### Option B — Raw length-delimited sync over UDS (no HTTP)

Dedicated accept loop: `UnixListener::accept()` → `acceptor.accept_tokio_io(stream)`.
Leaner (no HTTP/WS framing), but:

- TS side must implement the 4-byte framing itself (`net.Socket` +
  encode/decode + a new automerge-repo `NetworkAdapter`) — a new wire
  protocol to test and maintain in TypeScript, where today there is none.
- No `/health`, no HTTP probing → hub-mcp's auth-mode classification needs a
  different mechanism.
- Saves ~nothing at runtime; sync traffic is dominated by document bytes.

Rejected: doubles the client-side protocol surface for no real gain. Kept
here because `accept_tokio_io` makes it tempting; it's the wrong trade.

### Option C — Additive: UDS listener alongside TCP

Rejected: two simultaneous inbound transports complicate the auth policy
(a connection's trust domain would have to be inferred per-listener
forever, not chosen once at startup), the "listening" surface, and the
operator's mental model. Note this is *not* a rejection of exclusive-UDS
mode: browsers can't dial UDS, so a hub serving hub-client/preview must
run in TCP mode — which is precisely why the default never changes and
UDS is an explicit per-invocation choice.

## Recommended design (Option A)

### Server side

1. **CLI/config:** `--socket-path [path]` on `q2 hub` (and
   `socket_path: Option<PathBuf>` on `HubConfig`) meaning **listen
   exclusively on this UDS — bind no TCP listener**. Semantics:
   - no `--socket-path` → TCP+WS only — **exactly as today; the default
     never changes**;
   - `--socket-path` (no value) → UDS only, at the **convention path**
     (the default-location rule below) — a deterministic path operators
     can put straight into `--peer unix:///…`;
   - `--socket-path <path>` → UDS only at `<path>`.
   In all UDS cases the TCP bind is skipped entirely. The bare flag cannot
   resolve at CLI parse time (clap's `default_missing_value` is static,
   and the convention path depends on project-root discovery), so parsing
   keeps a "use the convention" sentinel and resolution to a concrete path
   happens during config assembly; `HubConfig.socket_path` is a concrete
   `Option<PathBuf>` by the time the server sees it.
   Two companion flags cover the trusted-proxy shape (see Auth model):
   `--socket-group <group>` relaxes the filesystem floor to group
   access, and repeatable `--socket-allow-uid <uid>` extends the
   accept-time peer-uid allowlist beyond the hub's own uid.
   There is deliberately **no additive mode** (TCP+UDS simultaneously):
   exactly one inbound listener exists per hub, keeping the auth policy,
   the listening surface, and the operator's mental model unambiguous.
   Default socket location in project mode:
   `<project>/.quarto/hub/hub.sock`; standalone: `<data-dir>/hub.sock`.
   UDS mode means browsers (hub-client, preview SPA) and `q2 mcp`
   **cannot** connect — correct for a peering-only hub, and worth stating
   in the flag's help text. Outbound peering (`--peer`) is independent of the
   inbound transport: it dials `ws://`/`wss://` as today, and a
   `unix:///` peer URL selects the `UnixDialer` (see Client side) — so
   same-machine hubs can peer over UDS in v1.
2. **Listener:** when `socket_path` is set, bind
   `UnixListener::bind(path)?` and `axum::serve(listener, router)` instead
   of the `TcpListener::bind` in `server.rs` — one serve loop, one
   listener, sharing the existing shutdown watch channel. Because
   `axum::serve` owns the accept loop, the accept-time peer-uid check (see
   Auth model) lives in a newtype wrapping the `UnixListener` that
   implements axum's public `Listener` trait: `accept()` runs
   `peer_cred()` and accepts iff the peer uid is the hub's own or is
   listed in `--socket-allow-uid`; mismatches are dropped and logged, so
   only allowlisted streams ever reach axum. The "listening" log line
   reports the socket path in UDS mode.
3. **Socket lifecycle:**
   - Before bind: if the path exists, connect-probe it; if a live hub
     answers, fail with "a hub is already running for this project"; if
     the connection is refused (stale file), `unlink` and proceed.
   - Create the parent dir `0700` and the socket `0600`, both set
     explicitly (do not trust umask). Trusted-proxy mode
     (`--socket-group <group>`): dir `0750`, socket `0660`, both chgrp'd
     to `<group>` — fail loudly at startup if the hub process cannot set
     that group (it must be a member).
   - The hub never chmod/chgrp's a directory it did not create (a literal
     "always chmod the parent" rule would chmod `/tmp` for
     `--socket-path /tmp/x.sock`, and chgrp'ing a pre-existing data dir
     would grant the proxy group read access to the automerge store).
     Under `--socket-group`, a pre-existing parent must already be
     group-traversable (`o+x`, or matching group with `g+x`), or startup
     fails loudly naming the dir and the remedy.
   - Remove the file on graceful shutdown (the existing shutdown watch
     channel gives the hook); a stale file after a crash is reclaimed by
     the probe-then-unlink rule above.
4. **sun_path guard:** if `path.as_os_str().len() > 100`, fall back to a
   short hashed path (e.g. `${TMPDIR:-/tmp}/q2-hub-<sha256(canonical
   project root)[:16]>.sock`) and record the actual path in
   `<project>/.quarto/hub/hub.sock.path` (mode `0600`) for operators.
   Note the hub canonicalizes the project root, so on macOS a project
   under a symlinked tempdir (`/tmp` → `/private/tmp`) can be longer
   than spelled and trip the guard — the guard working as designed.
5. **Path convention (operator-facing):** the socket lives at
   `<project>/.quarto/hub/hub.sock` (standalone: `<data-dir>/hub.sock`),
   with `hub.sock.path` recording the fallback when the `sun_path` guard
   fires — operators use these when configuring `--peer unix:///…` and
   proxies. There is no client auto-discovery.

### Auth model — no auth on UDS (A1)

**UDS connections carry no token.** The socket's
filesystem permissions are the kernel-enforced access-control floor —
`0600` socket in a `0700` dir ≈ same-uid-only, the same trust level as
reading the user's keyring — and a peer-uid check at accept time adds
defense-in-depth: `UnixStream::peer_cred()` (Linux `SO_PEERCRED`, macOS
`getpeereid` — both exposed by tokio) verifies the peer's uid equals the
hub's uid; mismatches are dropped and logged into the audit policy.
Connections arrive with no credential and no Origin header, and none is
required — peering config stays a single `--peer unix:///…` flag with no
token plumbing.

Implementation: because the listener choice is process-wide (exclusive
mode), no per-connection marking is needed. `axum::serve` owns the accept
loop, so uid equality is enforced by the `Listener`-trait newtype around
`UnixListener` described under Server side — `accept()` verifies the
peer's uid via `peer_cred()` before axum ever sees the stream. The hub
context behaves exactly like today's no-auth hub (`email = None` /
`"local"`); `ws_handler` is untouched. A UDS connection presenting a
Bearer can still be validated normally (harmless; no client needs to).

Considered and rejected — **A2 (UDS still requires Bearer):** defense in
depth and a single auth code path, but it bolts token plumbing onto
same-machine peering to defend against only same-uid attackers — who can
read the user's keyring and process memory anyway. Filesystem permissions
plus the peer-uid check already exclude every other uid.

Trusted-proxy shape — a TLS-terminating reverse proxy in front: a
same-host proxy forwarding to the socket (nginx `proxy_pass
http://unix:…`) is the classic app-server deployment and is explicitly
supported. The hub's UDS leg stays unauthenticated and the proxy is the
trust boundary. Two relaxations, both operator opt-in, make the socket
reachable by the proxy: `--socket-group` grants group access (dir
`0750`, socket `0660`) so a different-uid proxy can connect at all, and
`--socket-allow-uid` adds the proxy's uid to the accept-time allowlist.
The peer-uid check then doubles as an audit label: the hub's own uid
means a direct same-uid peer (hub↔hub), an allowlisted uid means a
proxied remote client — recorded distinctly in the access
policy. Running the proxy workers as the hub's uid also works and needs
no flags, but grants the worker read access to everything the user
owns; the group+allowlist path is the documented one. Share URLs need
no hub-side support here: they are built entirely client-side from the
browser's own connection URL, so they carry the proxy's public address
automatically.

Interaction with the TLS gate: `auth::validate_tls_config` currently demands
TLS or `--allow-insecure-auth` whenever OIDC is enabled. In UDS mode there
is no network transport to secure — filesystem permissions are the control,
strictly stronger than `--allow-insecure-auth` on loopback. The gate should
treat UDS mode as satisfying the secure-local-transport condition.
`--oidc-client-id` combined with `--socket-path` fails CLI validation —
hub-enforced auth over UDS is out of scope for v1; a hub that needs OIDC
runs TCP behind its proxy, as today. `--behind-tls-proxy` needs no rule:
it is consumed only by `validate_tls_config` and is inert without OIDC on
any transport, so the trusted-proxy deployment passes nothing special.

Keep the existing conflicting-credentials and Origin rules for TCP
untouched. The audit/access-policy code (`AuditAccessPolicy`, peer+email
logging) defines the UDS identity by peer uid — "local" for the hub's
own uid, "proxied" for an allowlisted uid — with the uid logged
alongside.

### Client side

*(This section is the as-planned design record. The dialer described
here was descoped on 2026-10-12 — see the decision note after Phase 2;
no shipped UDS client exists.)*

**The only UDS client is the Rust hub↔hub dialer.** TS clients get no
UDS capability: no `unix:///` in hub-mcp's `ConnectionManager`, no
`socketPath` in sync-client, no client discovery — `q2 mcp` keeps its
current TCP+WS posture. Rationale: Node's
`net.Socket` cannot do `SO_PEERCRED`, so a TS dialer could not verify the
server's uid the way the Rust dialer does, and every TS dial path is a
new surface for steering a document-syncing client to an attacker-owned
socket.

1. **URL scheme (peering):** Docker-style `unix:///abs/path/hub.sock`.
   Operator-supplied via `--peer`; curl equivalent for debugging:
   `curl --unix-socket <path>`.
2. **`UnixDialer` (Rust), in quarto-hub:** a ~60-line dialer modeled on
   `BearerDialer` (`tokio_tungstenite::client_async` over
   `UnixStream::connect(path)?`; no header injection — UDS is
   unauthenticated). After connect, verify the *server's* uid via
   `UnixStream::peer_cred()` and drop mismatches (cheap hardening; works
   on Linux and macOS). It lives in **quarto-hub**, not quarto-hub-provider:
   provider depends on hub, so a hub→provider dependency would cycle.
   `peer.rs` uses it for `--peer unix:///…`; quarto-hub-provider reuses
   it from hub for `q2 provide-hub --server unix:///…`.
3. **Peer dialing (`--peer`):** `peer.rs` selects the dialer by URL
   scheme — `ws://`/`wss://` → samod's `dial_websocket` (as today),
   `unix:///` → `UnixDialer` — enabling same-machine hub↔hub peering
   over UDS.

### Wire-parity contract

The guarantee that UDS transmits exactly the data TCP does rests on
construction, verified against the source: the router is
built before and independently of the listener (`run_server_with` in
`server.rs`; the only TCP coupling is the `TcpListener::bind` and a log
line); nothing in the router observes the transport (no
`ConnectInfo`/`peer_addr` extractors anywhere in the crate); the only
`Host`-keyed behavior is `check_ws_origin`, inert when `auth_config` is
`None` — the UDS shape; and `handle_websocket` hands every accepted
socket to the same `acceptor().accept_axum` path. Above the accept
layer there is no code that can observe the difference.

**Intentional wire deltas (the whitelist).** Exactly two, both in the
client's HTTP upgrade request: `Host` is `localhost` (from the
`ws://localhost/ws` translation) instead of `127.0.0.1:<port>`, and no
`Authorization` header is sent (A1). Everything after `101 Switching
Protocols` is shared code and cannot diverge.

**The right invariant.** Raw byte-identity is unachievable even between
two TCP runs — client→server frames carry per-frame random masks and
automerge peer IDs are random per `Repo`. Parity means *identical
decoded protocol sequence and identical converged document state*. With
pinned peer IDs in tests the decoded sequence becomes deterministic and
comparable across transports.

**How it is enforced:**

1. *Transport-matrix tests* (Phase 1): the same sync-behavior test
   bodies run against both transports, so any behavioral delta fails
   one arm.
2. *Capture-golden* (Phase 1): a raw server bound to a UDS records the
   client upgrade request verbatim; assert byte-equality with the
   TCP-captured request modulo the whitelist. This pins the only
   client-side construction site, the `UnixDialer`\'s request builder.
3. *Invariant guard:* nothing may add `ConnectInfo`/`peer_addr`
   extractors to the hub router — that is the property parity by
   construction stands on. Documented on `build_router_with_state`;
   add an xtask lint if the rule ever needs teeth.

### Platform scope — Linux + macOS

- UDS mode exists on **Linux and macOS** — everywhere tokio's
  `cfg(all(unix, feature = "net"))` holds. `--socket-path` on Windows fails
  fast at CLI validation with a clear error pointing at TCP. No Windows
  fallback (named pipes etc.): out of scope for now, revisit if demanded.
- Both platforms are fully testable in dev and CI: unit tests gate on
  `cfg(unix)`; the e2e verification runs on developer machines.

### Security analysis

| Threat | TCP loopback today | + UDS (A1) |
|---|---|---|
| Drive-by from browser JS to no-auth local hub | **Open** (no Origin check on no-auth path) | Closed — browsers can't reach UDS |
| Other-uid local process connects | Open on TCP | Closed — kernel DAC (`0600`/`0700`) + `peer_cred()` check at accept; attempt logged |
| Client steered to an attacker-owned fake hub (impersonation; sync exfiltrates documents) | Open only if the attacker controls `--server`/`QUARTO_HUB_SERVER` — the server location never comes from the project tree | Closed — TS clients have no UDS dial, and the one shipped dialer (the Rust `UnixDialer` behind `--peer`) verifies the server uid via `peer_cred()` after connect; same-uid residual out of scope (see "Same-uid malicious process") |
| Same-uid malicious process | Open (can ptrace/read keyring anyway) | Same — UDS doesn't worsen it |
| Port squatting / race on discovery | Possible | Closed — path convention in a private dir, no ports |
| Stale-socket confusion | n/a | Mitigated: connect-probe before unlink; cleanup on shutdown |
| Socket path discoverable by other uids | n/a | Hidden inside the `0700` project dir |
| Any network-stack exposure at all | Loopback reachable by local processes | **Exclusive UDS: none** — no TCP socket exists to reach by any means |
| Hub fronted by a local TLS proxy (trusted-proxy shape) | Proxy→loopback-TCP; hub exactly as reachable as the proxy makes it | Same — exposure is delegated to proxy config; the socket keeps kernel DAC, but a public proxy with no edge auth means a world-writable hub, and the no-auth path runs no Origin check, so the drive-by class reappears at the proxy boundary — enforce Origin allowlist / auth at the edge |

Remote clients cannot connect *directly* in UDS mode — but a same-host
TLS-terminating proxy forwarding to the socket bridges them. That is
the classic app-server deployment and it is supported via the
trusted-proxy relaxation (see Auth model); access control then lives at
the proxy, and the hub's exposure is exactly the proxy's exposure.
Hub-enforced auth over UDS is out of scope for v1, so `--socket-path`
combined with `--oidc-client-id` still fails CLI validation (a hub that
needs OIDC runs TCP behind its proxy, as today); `--behind-tls-proxy`
is accepted and inert without OIDC. `--socket-path` is always explicit —
**UDS is opt-in only, never default-on**, in project mode or anywhere
else. UDS mode pairs naturally with A1: with no TCP
listener and kernel DAC plus a uid check at accept, there is no untrusted
reachability left, so skipping token validation costs nothing.

#### Abstract sockets — considered and rejected

Linux abstract-namespace sockets lose to filesystem sockets on every
dimension that matters here:

| Dimension | Filesystem socket (`0700` dir) | Abstract socket |
|---|---|---|
| Access-control floor | Kernel-enforced DAC — even if the app forgets every check, other uids can't connect | **None** — any uid in the netns can connect; security exists only via an application-enforced `SO_PEERCRED` check (single point of failure) |
| Name squatting | Other uids can't pre-create the path in a private dir | Any uid in the netns can bind the name first → hub fails loudly (`EADDRINUSE`) but clients may silently connect to the squatter |
| Container scoping | Mount namespace — not mounted = unreachable; fine-grained in k8s pods | Network namespace — shared by all containers in a pod and by host-network containers |
| Discoverability | Hidden inside a `0700` dir | Listed in `/proc/net/unix`, readable by all — names are public, not secret |
| Lifecycle attacks | Stale-file unlink bugs, symlink races, umask mistakes (`0666` sockets) | None — the name dies with the listener |
| Same-uid attacker | Game over either way | Same |

Rationale: filesystem sockets provide a kernel-enforced DAC floor and
eliminate the squatter-impersonation residual (which Node clients cannot
defend against — `net.Socket` doesn't expose `SO_PEERCRED`),
*and* they run identically on macOS, making the feature end-to-end
testable on developer machines (abstract sockets are Linux-only). The
cost — the stale-file/permission lifecycle class — is small and handled by
the documented lifecycle rules (probe-then-unlink, explicit `0600`/`0700`,
cleanup on shutdown).

### Testing plan

Parity approach: see Wire-parity contract. The same test bodies run
over both transports via fixture transport knobs — parity is tested,
not argued — and a capture-golden pins the one legitimately-divergent
seam (the client upgrade request).

- Rust (TDD per repo rules; UDS tests gate on `cfg(unix)`):
  `TestHubBuilder` gains a `Transport::{Tcp, Unix}` arm; the WS
  handshake + document-sync bodies run against both, over a tempdir
  socket (keep paths short for `sun_path`). UDS-mode HTTP probes use
  hyper or raw-socket HTTP (reqwest cannot dial a unix socket); WS
  tests use tokio-tungstenite — the `UnixDialer`\'s own path, so the
  dialer is covered for free. Capture-golden: a raw UDS server records
  the `UnixDialer` upgrade request verbatim; assert byte-equality with
  the TCP capture modulo the whitelist (`Host` value, absent
  `Authorization`). Stale-socket reclaim (create dead file → hub
  unlinks and binds); live-socket refusal (second hub fails with
  "already running"); permission bits (`0700` dir, `0600` socket;
  `0750`/`0660` group-owned under `--socket-group`); file removed on
  graceful shutdown. `peer_cred()` check: same-uid accepted;
  `--socket-allow-uid` extends the accept allowlist; mismatched-uid
  rejection needs a `setuid`/`nobody` helper — a CI-only test. CLI
  validation: bare `--socket-path` resolves to the convention path in
  project and standalone modes; `--socket-path` combined with
  `--oidc-client-id` errors; `--behind-tls-proxy` accepted (inert
  without OIDC); `--socket-path` on Windows errors (that test runs
  everywhere). UDS mode: assert
  connecting to the configured TCP host:port is refused (no TCP
  listener exists). `--peer unix:///…`: two hubs sync a document over
  UDS, exercising scheme-based dialer selection.
- TS: no UDS suites (TS clients never dial UDS); `e2e-auth.test.ts` runs
  its sync-flow bodies (`e2e-sync-flows.ts`) over TCP as today.
- End-to-end (repo rule): two real hubs — one `--socket-path`, one
  `--peer unix:///…` — sync a document, output inspected. Runs on the
  macOS dev machines as well as Linux CI. No browser
  can join a UDS hub — that is the security premise, not a test gap;
  interactive inspection is `curl --unix-socket` for HTTP endpoints and
  `socat -v UNIX-LISTEN:…,fork UNIX-CONNECT:…` as a tee for the stream
  (compare the unmasked server→client direction).
- `cargo xtask verify` unaffected structurally (hub-build leg unchanged;
  no new TS suites to wire).

### Effort estimate

| Piece | Size |
|---|---|
| Hub: CLI flags, config, listener selection, socket lifecycle, `peer_cred` allowlist check | ~1 day + tests |
| `UnixDialer` (in quarto-hub) + `--peer` scheme selection + provider reuse | hours |
| Docs (`docs/`), e2e verification | half day |

### Prior art in repo

- `claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`
  evaluated UDS for the *engine-host* protocol and chose loopback TCP —
  rationale was subprocess-control-specific (blocking-std primitives,
  Windows std support), and does **not** transfer to a long-lived server
  listener with async IO. No conflict.
- `BearerDialer` (`crates/quarto-hub-provider/src/dialer.rs`) — the custom
  `Dialer` template.
- Related strands: bd-q96fhj3q (BearerDialer error mapping). CAP-16
  Streamable HTTP is the remote MCP counterpart; UDS is the local one.
