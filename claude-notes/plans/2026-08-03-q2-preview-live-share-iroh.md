---
title: "q2 preview live share over iroh (`--share` / `--join`)"
date: 2026-08-03
status: planned
branch: feature/preview-live-share (integration line; sub-tasks on braid/<id>-<slug>)
braid: bd-yyoyvx91
design-input: user design doc "Creating q2 preview with iroh" (2026-08-03 session)
verified: "file:line claims checked against ../iroh @ v1.0.3, ../iroh-tickets @ 1.0.0, ../samod, and this tree"
---

# q2 preview live share over iroh

Goal: "VS Code Live Share, but built into `q2`." A host runs
`q2 preview --share` and gets a single join string; a guest runs
`q2 preview --join <string>` on another machine and gets a browser tab showing
the same live preview session — automerge sync, presence, and engine captures
included — over an end-to-end-encrypted iroh P2P connection (relay fallback
when hole-punching fails).

## Explicit decision reversal

The preview epic (`claude-notes/plans/2026-05-11-q2-preview-epic.md:578-580`)
declared "multi-user collaborative preview" out of scope ("real collaboration
belongs in `quarto hub`, not `quarto preview`"). This plan reverses that, per
the design doc: ephemeral, zero-setup collaboration is exactly the
niche `q2 preview` fills that a persistent hub does not. The epic's
never-implemented Phase E `--share <url>` idea (broadcast to a remote sync
server; an unnumbered bullet at :313-315 — there is no "E.3" label) is
superseded: our `--share` takes no value and hosts P2P; joining a hosted hub
is Phase 5 of this plan, from the other direction (`--join <https-url>`).

## Architecture decision: authenticated TCP-over-iroh tunnel

**The joiner is a dumb, token-authenticated TCP proxy. The host's existing
preview HTTP server serves everything — SPA assets, `/health`, `/ws`,
`/api/preview/*`, project artifacts — through iroh QUIC streams.**

```
HOST                                              GUEST
q2 preview --share                                q2 preview --join <ticket>
┌────────────────────────────┐                    ┌──────────────────────────┐
│ preview hub (axum)         │                    │ TcpListener 127.0.0.1:N  │
│   127.0.0.1:PORT           │                    │   1 TCP conn = 1 QUIC    │
│   /health /ws /api /assets │                    │   bi-stream, token-      │
│            ▲               │                    │   prefixed               │
│ TunnelHost │ TcpStream per │   iroh (QUIC,      │            ▲             │
│ (iroh      │ bi-stream     │◄──E2E-encrypted,──►│ TunnelClient             │
│  Router)   │               │   hole-punch or    │ (iroh Endpoint)          │
└────────────┴───────────────┘   n0 relay)        └────────────┴─────────────┘
                                                               ▲
                                                     browser: http://127.0.0.1:N/
```

Why this shape wins (each alternative was seriously considered):

1. **The preview SPA hard-assumes same-origin.** `deriveWsUrl()` derives
   `ws(s)://{location.host}/ws` (`q2-preview-spa/src/PreviewApp.tsx:539-542`)
   and the index doc id comes from same-origin `GET /health`
   (`PreviewApp.tsx:499-514`). A local proxy makes both true by construction —
   zero SPA changes for the MVP.
2. **The boot/reconnect supervisor keeps working.** `bootController.ts`'s
   contract is "HTTP `/health` decides liveness; the WebSocket gets patience"
   (`q2-preview-spa/src/bootController.ts:1-20`). With a tunnel, `/health`
   through the local port genuinely probes the remote host: tunnel dies →
   health strikes → banner → HTTP-only polling → recovery. For free.
3. **Rendering is already client-side.** The SPA renders via WASM in the
   guest's browser; after the one-time asset download, tunnel traffic is just
   automerge sync frames, presence ephemerals, and occasional artifact
   fetches. A "native" samod-over-iroh transport would not move rendering
   anywhere better.
4. **No version skew.** The guest's browser runs the host's SPA build, so SPA
   ↔ sync-protocol ↔ schema versions always match. A joiner that served its
   own SPA would need a compatibility story.
5. **Engine captures, artifacts, re-execute all work.** `/.quarto/project-artifacts/*`
   and `/api/preview/re-execute` read host-local state that automerge does not
   carry; proxying HTTP is the only design in which these work without new
   protocol surface.
6. **No TCP-over-TCP pathology.** The tunnel is a *terminated splice*, not
   encapsulation: the TCP legs are loopback on each end and only QUIC runs on
   the WAN, so exactly one congestion controller governs the wide-area path.

**Scope note: multiple concurrent guests are in scope for v1.** Each guest is
an independent iroh connection into the same Router accept loop, all sharing
the one session token; per-peer distinction exists only in host logs
(`remote_id()`), not in permissions (risk #4). Nothing in the design is
single-guest-shaped; Gate 0 Q1 carries an optional multi-guest sanity check,
and Phase 3's end-to-end check joins two guests at least once.

**Alternative rejected (for v1): samod-native iroh transport.** samod's
`Transport::from_tokio_io` accepts any `AsyncRead + AsyncWrite` (samod
checkout `samod/src/transport.rs:54-81`), `HubContext::acceptor()` is public
(`crates/quarto-hub/src/context.rs:441`), and `BearerDialer`
(`crates/quarto-hub-provider/src/dialer.rs`) is precedent for a custom
`samod::Dialer` — so a first-class `IrohDialer`/iroh acceptor is quite
feasible. The decisive count: **the guest needs five
surfaces from the host — SPA assets, `/health` (index doc id),
`/api/preview/config` (allow-edit), `/ws` (sync + presence), and
`/.quarto/project-artifacts/*` + `/api/preview/re-execute` — and a native
samod transport replaces exactly one of them.** The HTTP tunnel must exist
regardless, so "native transport" means a *second* transport, auth
checkpoint, and reconnect lifecycle alongside it, to save only WebSocket
framing on the smallest, most latency-tolerant traffic class (sync frames;
the heavy traffic — assets, WASM, artifacts — is HTTP either way). Auth
bypass (checks live in the axum `ws_handler`,
`crates/quarto-hub/src/server.rs:1602-1648`; the samod `AccessPolicy` impl —
concrete type `AuditAccessPolicy` — is allow-all, with the unconditional
`true` at `access_policy.rs:62`) is the *cheapest* objection to fix with a new auth
endpoint — the load-bearing ones are the same-origin assumptions, the
no-version-skew guarantee, and the artifacts/re-execute surface, which an
auth endpoint does not touch. For accuracy: the "guest as intermediate samod
peer" variant would *not* break presence — samod gossips ephemerals through
intermediate peers (verified: `samod-core/src/actors/document/document_actor.rs:319-333`
forwards ephemeral messages to all other connections) — but it would break
the boot supervisor's "HTTP `/health` decides liveness" contract (a local
`/health` always answers even when the host is gone) and grow the "separate
auth endpoint" into a full sidecar API for doc id / allow-edit / artifacts /
re-execute. Keep as future work for native-peer use cases (e.g.
`q2 provide-hub` over iroh), where samod-over-iroh replaces the *entire*
surface, not a fifth of it.

## iroh facts this plan depends on (verified against ../iroh @ v1.0.3)

The local checkout is **iroh 1.0.3** — a hard API break from pre-1.0
tutorials. Published on crates.io; MIT OR Apache-2.0; MSRV 1.91 (we're on
nightly-2026-04-28 / 1.97 — fine); edition 2024 (we already use it).

- `Endpoint::builder(preset)` / `Endpoint::bind(preset)` — the preset is
  **mandatory** (`iroh/src/endpoint.rs:952,957`). `presets::N0` = n0 pkarr
  publish/resolve + DNS lookup + default relays + ring crypto
  (`iroh/src/endpoint/presets.rs:113`). Tests use `presets::Minimal` (crypto
  only) + `RelayMode::Disabled` + explicit loopback addrs for hermeticity
  (Minimal-built endpoints already default to `RelayMode::Disabled`; the
  explicit setting is documentation).
- Names are `EndpointId` / `EndpointAddr` / `TransportAddr::{Relay,Ip,Custom}`
  (`iroh-base/src/endpoint_addr.rs:41-54`). `NodeId`/`NodeAddr`/
  `discovery_n0()` no longer exist.
- `Router::builder(ep).accept(ALPN, handler).spawn()` registers ALPNs itself
  (`iroh/src/protocol.rs:502-511`); `ProtocolHandler::accept(Connection)` runs
  on its own task and may be long-lived (`protocol.rs:262-268`). Router is
  abort-on-drop: store it, await `shutdown()`.
- `open_bi`/`accept_bi` yield `(SendStream, RecvStream)`; `RecvStream: AsyncRead`,
  `SendStream: AsyncWrite`, and `SendStream::poll_shutdown` calls `finish()` —
  a QUIC FIN, exactly the half-close mapping the splice needs (via `noq`
  1.1.0, n0's quinn fork; `send_stream.rs:345-347`). Use
  `tokio::io::join(recv, send)` + `tokio::io::copy_bidirectional` for splicing.
- **Stream laziness:** `open_bi()` does not wake the peer's `accept_bi()`
  until the opener writes bytes (`iroh/src/lib.rs:150-158`). Our per-stream
  token prefix satisfies this by construction.
- `endpoint.online().await` **pends forever with no relay reachable**; wrap in
  `tokio::time::timeout(Duration::from_secs(10), …)` with our own constant.
  **Do not use `iroh::NET_REPORT_TIMEOUT`** — that re-export is
  `net_report::TIMEOUT`, a bare `u64 = 5` intended for docs (not a `Duration`,
  not 10 s; passing it to `timeout()` is a compile error). The 10 s budget we
  want matches iroh's *private* `defaults.rs:129` constant.
  Only after it resolves does `endpoint.addr()` contain a relay URL.
- Tickets live in a separate crate, **`iroh-tickets` 1.0.0** (verified against
  ../iroh-tickets). Trait `Ticket` requires `KIND` +
  `encode_bytes`/`decode_bytes` (body format is implementer-supplied; postcard
  is the recommendation we follow); the provided `encode_string`/
  `decode_string` produce lowercase-`KIND`-prefix + BASE32_NOPAD and reject
  foreign kinds. We implement `Ticket` for our own struct rather than
  shipping a bare `EndpointTicket` (KIND `"endpoint"`), because the join
  string must also carry the session token.
- Graceful close: `router.shutdown().await?` alone suffices — it already
  calls `Endpoint::close` (`protocol.rs:421-423`) and returns
  `Result<(), JoinError>` which must be handled (≈3 s worst case); a trailing
  `endpoint.close().await` is an idempotent no-op. `Connection::close()` is
  sync and only queues.
- Default n0 relays need no auth; behind NAT the relay path is the designed
  fallback (worst case: relayed throughput, not failure).
- No in-tree TCP↔QUIC bridge exists; we write the ~30-line splice ourselves.
  `iroh/examples/echo-no-router.rs`, `auth-hook.rs`, `search.rs` are the
  patterns to crib from.

## Security model

- **Join string = capability.** `q2preview…` ticket carries the host's
  `EndpointAddr` **and a random 256-bit session token**. QUIC gives
  confidentiality + host authentication (the joiner dials the pinned
  `EndpointId` from the ticket — MITM-proof); the token authenticates the
  *guest* to the host.
- **Per-stream token prefix.** Every bi-stream starts with the raw 32-byte
  token; the host `read_exact`s it under a timeout and compares with
  `subtle::ConstantTimeEq` before connecting the backing `TcpStream`.
  Stateless, no extra RTT (bytes pipeline behind the token), and it satisfies
  iroh's write-first rule. Mismatch → stream reset + connection close + log.
- **The local HTTP port stays loopback-bound.** `--share` adds no listening
  TCP surface; the only remote path is the authenticated tunnel. (This
  sidesteps the never-implemented `--insecure-allow-network` posture from
  epic Q7, `2026-05-11-q2-preview-epic.md:416-431`.)
- **What the token grants — must be printed at share time:** guests can view
  the project, trigger `POST /api/preview/re-execute` (engine execution on the
  host = code execution by design), and, iff the host passed `--allow-edit`,
  write edits that persist to the host's disk. Per-peer permissions are
  explicitly out of scope for v1 (`--allow-edit` is a process-wide
  `OnceLock<bool>` + one `DiskWritePolicy`; see follow-ups).
- Log each joining peer's `EndpointId` (`conn.remote_id().fmt_short()`) on the
  host for auditability.
- Privacy note: `presets::N0` publishes the host's addresses (including LAN
  IPs) to n0's pkarr/DNS. Acceptable for v1; `AddrFilter::relay_only()` is the
  knob if we want to change that (follow-up).

## CLI surface (decided)

| Flag | Meaning |
|---|---|
| `q2 preview --share` | Host: normal preview + iroh tunnel host; prints ticket + ready-to-paste `q2 preview --join …` line |
| `q2 preview --join <TICKET>` | Guest: local loopback proxy + browser; no local project, no hub, no SPA of its own |
| `q2 preview --ui <viewer\|editor>` | Which embedded frontend the server serves. `viewer` (default): the read-only preview SPA; `editor`: the full hub-client editor. Orthogonal to `--allow-edit` — UI choice never changes the disk write policy |

- **`--ui` replaces the earlier `--with-editor` proposal.**
  An enum because the flag *substitutes* which embedded dist `spa_handler`
  serves (it is not additive); `viewer` rather than `preview` because
  `q2 preview --ui preview` is a tautology — "preview" names the session, not
  the frontend. Crucially, **`--ui editor` does not imply `--allow-edit`**:
  UI and write policy are a real 2×2 — the viewer already has an allow-edit
  inline-edit surface (`PreviewApp.tsx:523-532`), and
  `DiskWritePolicy::ReadOnly` already models "session edits sync live, disk
  stays authoritative" (`crates/quarto-hub/src/sync.rs:35-57`). So
  `--share --ui editor` *without* `--allow-edit` is a deliberate sandbox
  mode: guests get the full editor, their edits drive everyone's live
  session, and nothing persists to the host's disk. (Caveat to document:
  under `ReadOnly` a session edit survives only until the next host-side
  filesystem change converges that doc back to disk content — `sync.rs:41-45`;
  binary docs revert immediately.) Running `--ui editor`
  without `--allow-edit` prints: "session edits are ephemeral — pass
  `--allow-edit` to persist to disk". Symmetrically, `--share` does **not**
  imply `--ui editor` (that would couple Phase 2 to Phase 4 and make the
  heavy editor bundle the default relay-path boot experience); if usage
  later wants editor-by-default when sharing, the mechanism is an
  overridable `--ui auto` default — explicitly a non-goal for v1.
- **`--host` is unavailable for hosting semantics** — it is already the bind
  interface (`crates/quarto/src/main.rs:212-213`). It keeps that meaning
  (and, for `--join`, binds the local proxy listener; default `127.0.0.1`).
- Compositions: `--share --ui editor` (guests get the full editor, served by
  the host; sandbox unless `--allow-edit`); `--share --ui editor --allow-edit`
  (full collaborative editing, persists to host disk); `--share --allow-edit`
  (viewer with inline-edit write-back); `--join --port N`; `--join --no-browser`.
- Conflicts (clap `conflicts_with`): `--join` × {`path`, `--share`,
  `--no-project`, `--allow-edit`, `--ui`, `--data-dir`,
  `--preview-dir`}.
- ALPN: `b"q2/preview-tunnel/0"`. Ticket KIND: `"q2preview"` (KIND string is
  the protocol version tag; breaking change ⇒ new KIND).

## New crate: `crates/quarto-p2p`

Encapsulates all iroh usage; `quarto-preview` and `crates/quarto` see only:

```rust
pub struct PreviewShareTicket { pub addr: EndpointAddr, pub token: [u8; 32] }
  // impl iroh_tickets::Ticket (KIND "q2preview"), Display/FromStr, Debug redacts token

pub struct TunnelHost;    // TunnelHost::spawn(cfg, target: SocketAddr) -> (PreviewShareTicket, TunnelHostHandle)
pub struct TunnelClient;  // TunnelClient::bind(ticket, local: SocketAddr) -> (SocketAddr, TunnelClientHandle)
  // handles: async shutdown(); client: status watch (Connected/Reconnecting) for CLI messaging
```

Native-only (not in the WASM closure — `wasm-quarto-hub-client` depends on
`quarto-core`/`pampa`, not on `quarto-preview`/`quarto-p2p`). Deps: `iroh`
(default features), `iroh-tickets`, `subtle`, `rand`, `tokio` — with
**explicit `io-util` + `net` features**: `io::join`/`copy_bidirectional` are
`io-util`-gated, the workspace default set (`rt-multi-thread`, `macros`)
lacks it, and today it only unifies on via quarto-hub's `full` — `tracing`,
`thiserror`. (Lockfile check: `subtle`, `rand`, and the Phase 1
WS-test dev-dep `tokio-tungstenite` 0.29 are already in `Cargo.lock`
transitively; the genuinely new tree is iroh + iroh-tickets + postcard.) Host side re-dial-free (Router accept loop); client side owns a
re-dial loop with backoff (existing TCP conns die on connection loss; new ones
use the fresh connection; the SPA's health supervisor papers over the gap).
Client registers the ticket's `EndpointAddr` in a `MemoryLookup`
(`iroh/src/address_lookup/memory.rs:75`) so re-dials re-resolve without n0
infra.

---

# Phases

Sequencing: **Gate 0 first — nothing else starts until it returns "go."**
Then 0 → 1 → 2 → 3 are strictly ordered. **Phase 4 (`--ui editor`) is
independent of iroh entirely** and can proceed in parallel with Phase 0/1
once the gate passes — it waits for the gate like everything else, because a
no-go kills the epic and P4 only exists here in service of the share story.
Phase 5 is a spike, gated on 3 + 4.

## Gate 0 — Feasibility spike (go/no-go)

**Nothing below this section starts until this gate returns "go."** The plan
commits to a specific architecture (dumb token-authenticated TCP proxy over
iroh QUIC) and a heavy new dependency before any of it has run end-to-end.
This gate front-loads the bets that would kill or reshape the plan, at
throwaway-spike cost, before we pay for Phase 1's full TDD surface and five
phases of implementation.

**Mechanics.** Time-box: ~2 working days. Spike code lives on the gate
strand's branch (`braid/bd-l4j4ky8k-live-share-feasibility-gate`) and is
**throwaway — it is never merged**; what lands on the integration line is
this section's findings, the measurements, and the recorded decision.
**TDD exemption:** this is an investigation, not a feature — the repo's
TDD rule resumes at Phase 1, which re-implements from scratch under tests
(salvage knowledge, not code). Spike shortcuts allowed: `unwrap()`s, no
re-dial loop, no graceful shutdown, a debug-printed `EndpointAddr` + hex
token instead of the ticket format. Shortcuts NOT allowed: a real
`q2 preview` server as the tunnel target, a real browser as the client,
the real n0 relay for the cross-network leg, and the 32-byte token prefix
on each stream (it doubles as iroh's write-first requirement, so skipping
it would make the spike unrepresentative).

**Feasibility questions (spike):**

- [x] **Q1 — Does the SPA work through the tunnel at all?** One machine:
      `q2 preview` on a fixture project, hand-rolled tunnel host in front of
      it, hand-rolled tunnel client on another port, browser pointed at the
      client port. Pass: SPA boots, `/health` returns the index doc id, the
      document renders (assets + WASM fetched through the tunnel), and a
      host-side file edit propagates live to the browser — i.e. the splice
      carries both plain HTTP and a long-lived WebSocket. Run in **Chrome
      and Firefox** — browser-specific WS behavior has bitten this SPA
      before (`2026-06-11-firefox-ws-peer-timeout-fix.md`), and a
      browser-specific failure is cheaper to learn here than in Phase 2.
      Optional (non-blocking): point a second tunnel client + browser at
      the same host as a multi-guest sanity check — v1 scope is N
      concurrent guests (see the architecture scope note).
- [x] **Q2 — Does it survive a real session?** Keep the Q1 session open
      ≥10 minutes including ≥2 minutes fully idle, then edit again. Pass:
      the edit still propagates. This checks the QUIC keep-alive vs.
      browser-connection-pooling interaction (`endpoint/quic.rs:155-161`,
      5 s keep-alive vs. 30 s idle) in reality, not just in source. Run
      the idle leg in Safari as well (free on this machine) —
      idle-connection handling is exactly where browsers differ.
      Opportunistic observation, not pass/fail: close the host laptop's
      lid mid-session and note what the guest sees on wake — sleep/wake is
      the most common real-world connection death and the observation
      informs Phase 1's re-dial design (the reconnect story itself is
      Phase 1 scope).
- [x] **Q3 — Does the relay path work and feel usable?** Host and guest on
      different networks, real n0 relay. Logistics: network namespaces are
      Linux-only and the dev host is macOS, so this leg is **two physical
      machines** (e.g. one on a phone hotspot); a headless cloud VM guest
      cannot measure browser time-to-first-render. (If a test network
      blocks UDP entirely, iroh falls back to relay-over-TCP-443 — that is
      the designed worst case, not a failure to debug.) Pass: join
      succeeds. Record **three numbers**, not one: (a) time-to-first-render
      on the guest, (b) bytes transferred + effective throughput for the
      asset boot, (c) edit→propagation latency once the session is warm —
      (c) is the steady-state "feels usable" number (sync frames are
      small, so it should be ~relay RTT; confirm). **Known confounder for
      (a):** the preview server serves everything uncompressed (verified:
      no CompressionLayer anywhere in the preview/hub stack —
      quarto-hub's tower-http features are only `trace`/`cors`/
      `set-header`), so the ~38 MB WASM travels at full size; (b) exists
      to make a breach attributable to tunnel vs. payload. Soft threshold:
      < 30 s time-to-first-render on a residential-class connection;
      worse ⇒ conditional go, mitigation ladder in order of cost:
      (1) HTTP compression on the preview server
      (`tower_http::CompressionLayer` or a precompressed `.wasm.br` —
      wasm compresses ~3–5×, no version-skew cost), (2) guest-side asset
      serving (risk #5) promoted from follow-up into Phase 1 scope.
- [x] **Q4 — Is the dependency weight acceptable?** Measure and record
      here: `q2` release binary size and clean-build wall time, before vs.
      after the iroh dep tree. Methodology (so Phase 0's re-measure
      compares like with like): release build of `--bin q2`, `cargo clean`
      before each timed run, baseline and after measured back-to-back on
      the same machine — the bd-xvdop controlled-measurement style — and
      record the exact iroh/iroh-tickets versions measured. Soft
      thresholds: ≤ 15 MB binary growth and ≤ 25 % clean-build slowdown;
      worse ⇒ conditional go, feature-gating decision required before
      Phase 0 starts.

**Static checks (no spike code needed, but gate-blocking):**

- [x] Licensing: iroh is MIT OR Apache-2.0 + one BSD-3 notice
      (`../iroh/iroh/LICENSE-BSD3`) to carry in attribution — confirm
      nothing else in the transitive tree (noq, netwatch, portmapper,
      hickory, reqwest) is problematic. This repo has
      **no cargo-deny infrastructure** (no `deny.toml`, no CI step), so
      this is a one-off inventory on the spike branch (ad-hoc
      `cargo deny check licenses` with a scratch config, or
      `cargo license`), not an existing gate to pass
- [x] n0 usage policy: `--share` defaults to n0's hosted relays and pkarr
      publishing. Licensing covers the code; this covers the **service**:
      confirm n0's terms for third-party production use of the public
      relay/DNS infrastructure (fair-use expectations, rate limits, any
      "run your own relay for shipped products" guidance). Same class of
      due diligence as the license inventory — cheap to check now,
      expensive to discover post-ship
- [x] Windows: the spike crate compiles for Windows (iroh supports it;
      watch `netwatch`/`portmapper`). **Test-suite CI
      has no Windows leg** (`.github/workflows/test-suite.yml` matrix is
      ubuntu + macos; only `release.yml` builds Windows). A manual check
      on a real Windows machine (`cargo xtask test` per
      `claude-notes/instructions/windows-dev.md`) works if one is handy;
      **do not** burn spike time on
      `cargo check --target x86_64-pc-windows-msvc` from macOS — it dies
      in ring's C/asm build script (no MSVC-targeting toolchain) before
      saying anything about iroh. The reliable cheap signal is a
      **throwaway `workflow_dispatch` GH Actions job** on the spike branch
      running `cargo check` on `windows-latest` (release.yml already
      proves those runners build this tree); it is not a PR-CI signal
- [x] WASM closure unaffected: `cargo tree -i iroh` from
      `wasm-quarto-hub-client` must fail

**Decision (the actual gate):**

- [x] Findings recorded above — exact invocations, output snippets, and
      numbers; the repo's end-to-end evidence policy applies to the spike
      too ("output inspected" notes, no success-by-absence-of-errors)
      *(see "Gate 0 findings (spike executed 2026-08-04)" below)*
- [x] Verdict posted as a `braid comment` on the epic (bd-yyoyvx91);
      **user sign-off required** — go / conditional go / no-go is the
      user's call, informed by this data *(findings posted 2026-08-04;
      **user signed off: GO**, 2026-08-04 — recorded on the epic; gate
      strand bd-l4j4ky8k closed; Phase 0 and Phase 4 unblock)*

| Verdict | Meaning |
|---|---|
| **Go** | Q1–Q4 pass, static checks green → Phase 0 unblocks |
| **Conditional go** | Q3 or Q4 breached a soft threshold → user picks the mitigation (feature-gate iroh; add HTTP compression to the preview server; promote guest-side asset serving; LAN-only v1 scope) and this plan is amended before Phase 0 starts |
| **No-go** | Q1/Q2 fail unfixably (the splice can't carry the SPA), or licensing/Windows are hard blockers → the epic closes, or the architecture section is redone from the rejected-alternatives list (guest-side SPA + samod-native transport is the fallback to re-evaluate); Phases 0–5 do not start |

### Gate 0 findings (spike executed 2026-08-04)

**Spike artifacts.** Branch `braid/bd-l4j4ky8k-live-share-feasibility-gate`
(throwaway — never merged), commits `e80cebf8` (tunnel host+client crate
`crates/q2-p2p-spike`) and `9067b008` (Q4 wiring + Windows workflow).
Dependencies measured: **iroh 1.0.3 + iroh-tickets 1.0.0 from crates.io**
(spike client's `--relay-only` uses a custom `PathSelector` behind iroh's
`unstable-custom-transports` feature — zero extra deps). Browser legs driven
by Playwright (Chromium 1223, Firefox 1522, WebKit 2287) with a scratchpad
driver script; one non-obvious detail for Phase 2/3 e2e work: **the preview
SPA renders the document inside an iframe**, so text assertions must scan
`page.frames()`, not the top page.

**Q1 — PASS (Chromium + Firefox, two concurrent guests).**
Invocations: `q2 preview <fixture> --no-browser` (port 49583, 2-page fixture
project) ← `spike-tunnel-host 127.0.0.1:49583` ← two
`spike-tunnel-client <ticket> <token> 9280|9281` ← browsers at
`http://127.0.0.1:928x/?page=index.qmd`.
- `curl http://127.0.0.1:9280/health` through the tunnel returned the
  **identical** payload to direct, including
  `"index_document_id":"4ZLBFnLKivaVWXc9HdF2SCACTn2U"`.
- SPA booted and rendered in both browsers; **~47.5 MB fetched through the
  tunnel per guest** (uncompressed, 38.4 MB of it the WASM — the plan's
  no-compression confounder confirmed); first render 1.7 s (Chromium) /
  1.9 s (Firefox) on the direct path.
- Host-side `sed` edit (`MARKER-0`→`MARKER-1`) propagated live to **both
  guests simultaneously** in **554 ms (Chromium) / 587 ms (Firefox)** —
  multi-guest sanity check passed in the same run. Before/after screenshots
  inspected: rendered document shows the new marker text in both browsers.
- Wrong-token check: client with a zeroed token → host logs
  `BAD TOKEN - dropping stream`, curl fails, zero bytes reach the target.
- Splice carries HTTP + the long-lived `/ws` WebSocket by construction
  (sync frames are what propagated the edits).

**Q2 — PASS (12 min session, fully idle, Chromium + WebKit).**
Fresh sessions on both tunnel clients, then **720 s fully idle** (only the
SPA's own background polling), then another host edit
(`MARKER-1`→`MARKER-2`): propagated in **1.07 s (Chromium) / 1.18 s
(WebKit)**. Post-soak screenshots inspected — rendered `MARKER-2` visible.
The QUIC keep-alive (5 s) vs browser-connection-pooling (30 s idle timeout)
interaction is a non-issue in practice. Caveats: WebKit (Playwright) stands
in for Safari — same engine, not the Safari app; the lid-close observation
was not performed (no way to close the lid programmatically) — fold it into
the real cross-network session below.

**Q3 — PASS. Two measurements: (i) single-machine relay-pinned
approximation, (ii) real cross-network leg via a GH Actions guest (below).**
The spike client's `--relay-only` flag pins path selection to relay paths
(verified: **zero** DIRECT selections in the client log for the whole leg;
selected path stayed `euc1-1.relay.n0.iroh.link`, rtt ~31 ms warm). Fresh
Chromium boot through the relay-pinned tunnel:
- (a) time-to-first-render: **4.07 s** (soft threshold was < 30 s)
- (b) bytes: ~47.5 MB → ≥ **11.7 MB/s** effective through the real n0 relay
  (lower bound; includes render time)
- (c) edit→propagation warm: **1.00 s** (vs ~0.55 s direct — consistent with
  "+relay RTT")
Honest limitation: both endpoints shared this machine's (fast) connection,
so (a)/(b) are not a residential-guest measurement — but the traffic did
transit the real n0 relay, so protocol behavior and rate-limiting posture
are exercised. Also observed (informs Phase 1 status messaging): on loopback
the first selected path after connect is RELAY, upgrading to DIRECT within
~3–6 s — initial connect in 162 ms.

**Q3 cross-network leg — executed and PASSED (2026-08-04, user-approved
push).** The plan assumed a cloud VM guest "cannot measure browser
time-to-first-render" — true for eyeballs, false for a Playwright driver
measuring it in-process, so the two-machine leg was automated: live host on
the dev machine (`q2 preview` + `spike-tunnel-host`, plus a loop bumping a
numbered marker every 20 s with ms timestamps), guest = `ubuntu-latest`
GH runner (Azure network) that builds the spike client, joins via the
ticket (passed through ephemeral repo secrets, deleted after the run),
and boots headless Chromium through the tunnel. Workflow
`spike-q3-guest.yml`, run 30897199010; boot + final screenshots for both
legs downloaded from the artifact and inspected (rendered document at the
last observed marker). Results:
- connect: **0.42 s** cross-network. **Both legs — default and
  relay-pinned — stayed on the relay for their whole run**: hole-punching
  Azure↔residential NAT never yielded a selected direct path, so this
  measured exactly the relay-fallback scenario Q3 exists for.
  Guest→relay rtt ~130 ms (`euc1-1` from Azure); relays probe at
  200/OK in 0.08–0.5 s from the runner.
- (a) time-to-first-render: **13.2 s (default) / 13.3 s (relay-pinned)** —
  soft threshold < 30 s, PASS.
- (b) ~47.5 MB per boot → **~3.7 MB/s sustained through the relay**
  (uncompressed WASM dominates; the HTTP-compression mitigation would cut
  the payload ~3–5×).
- (c) edit→observed propagation over 6 marker bumps: **0.81–2.34 s,
  median ~1.1 s** (includes the driver's 150 ms poll grain and host↔runner
  NTP clock skew).
- Caveats: runner egress is datacenter-class, not residential; the
  Safari-app and lid-close observations still need a human-driven session.
- Reliability note worth keeping: the **first** guest run (30896594167's
  sibling, run 30896593541) failed at `ep.connect` with a bare timeout —
  the host saw no inbound attempt — and was unreproducible 8 minutes later
  (identical code + ticket connected in 0.42 s). A transient
  n0 discovery/relay hiccup of exactly the class Phase 1's re-dial/backoff
  and Phase 3's error UX must absorb (the spike client retries nothing by
  design).

**Q4 — dependency weight (controlled, bd-xvdop style: `cargo clean` before
each timed run, back-to-back on the same machine, release `--bin q2`,
iroh 1.0.3 wired reachably into the binary via an env-gated hook).**
- baseline (branch point, no iroh): **94.9 s** wall, binary **81,814,464 B
  (78.0 MiB)**
- after (iroh dep tree linked): **110.6 s** wall, binary **95,602,544 B
  (91.2 MiB)**
- deltas: **+13.79 MB binary** (threshold ≤ 15 MB — within) and **+16.6 %
  clean-build wall time** (threshold ≤ 25 % — within) → **PASS, no
  feature-gating decision forced**
- reachability verified end-to-end: the measured release binary itself ran
  the tunnel host (`Q2_SPIKE_TUNNEL_TARGET=127.0.0.1:49583
  ./target/release/q2` printed a live TICKET/TOKEN) — the delta is not a
  dead-code artifact

**Static checks.**
- **Licensing — PASS.** One-off inventory of the 141 packages the iroh tree
  adds to `Cargo.lock` (`cargo tree --format "{p}|{l}"` + `cargo metadata`
  for target-specific deps): all permissive. MIT/Apache-2.0 dual dominates;
  BSD-3-Clause: `curve25519-dalek`, `ed25519-dalek` (attribution notices to
  carry, matching iroh's own `LICENSE-BSD3`); BSD-2-Clause: `arrayref`,
  `spez`; **one MPL-2.0: `attohttpc`** (via the portmapper stack; file-level
  weak copyleft — fine as an unmodified linked dep); 3× Unlicense
  (`async_io_stream`, `pharos`, `ws_stream_wasm` — wasm-target-only);
  1× Zlib-or; no GPL/LGPL/AGPL/SSPL anywhere.
- **n0 usage policy — PASS with eyes open.** No dedicated ToS for the preset
  infra exists (itself a finding). DNS-discovery docs explicitly say
  *"You're more than welcome to run production systems using the public
  relays if you find performance acceptable"* — but relays are rate-limited
  (numbers unpublished, *"can change at any time"*, iroh 1.0 post), carry
  *"no guaranteed uptime"*, and the add-a-relay doc says *"production
  deployments should run their own."* Iroh Services ToS (May 2025) reserves
  unilateral termination, caps liability at US$50. Paid offering exists
  (Iroh Services; dedicated relays ~$197/mo, free tier 10 concurrent
  endpoints). Public-relay sunsets are per-protocol-version on an announced
  schedule (v1.0 relays "until End of Life"). Verdict: ephemeral preview
  sharing is well inside tolerated use — relays are handshake/fallback
  paths, not the primary data path — but **make relay/DNS endpoints
  user-configurable** (follow-up strand at epic close) and track the
  per-version relay sunset schedule at iroh upgrades.
- **Windows — PASS.** Throwaway workflow on the spike branch
  (`.github/workflows/spike-windows-check.yml`) pushed with user approval
  as `spike/bd-l4j4ky8k-live-share-feasibility-gate` and run on
  `windows-latest`: `cargo check -p q2-p2p-spike -p quarto` (the iroh
  tunnel crate + the fully wired q2 closure) succeeded in 5m36s — GH run
  30894960520, 2026-08-04. Gotcha for posterity: `workflow_dispatch` only
  registers from the default branch (404 on dispatch), so the workflow
  fires `on: push` to `spike/**` instead.
- **WASM closure — PASS.** From `crates/wasm-quarto-hub-client` (spike
  branch, iroh in the workspace lockfile):
  `cargo tree -i iroh` → `error: package ID specification 'iroh' did not
  match any packages`.
- Bonus datapoint: the spike's bare `EndpointTicket` printed at 151 chars
  (+ 64 hex token chars) — consistent with the plan's 173–235-char estimate
  for the combined `q2preview` ticket.

**Runbook — optional residential-class Q3 re-run (user + one extra
machine/network; the cross-network leg itself already passed via the GH
Actions guest above — this remains only if a residential-uplink
time-to-first-render number is wanted):**
1. On the host machine (this repo, spike branch worktree):
   `cargo run --bin q2 -- preview <project> --no-browser` (note the port),
   then `./target/debug/spike-tunnel-host 127.0.0.1:<port>` (from
   `.worktrees/bd-l4j4ky8k-live-share-feasibility-gate`); copy TICKET +
   TOKEN.
2. On a second physical machine on a different network (e.g. phone
   hotspot), build the spike client (`cargo build -p q2-p2p-spike` on the
   spike branch) and run
   `spike-tunnel-client <TICKET> <TOKEN> 9280 --relay-only` (relay-pinned;
   drop `--relay-only` for a hole-punching run — the client logs which path
   is selected).
3. Browser at `http://127.0.0.1:9280/?page=index.qmd`; stopwatch
   time-to-first-render; edit a file host-side and eyeball propagation.
   Optionally close the host's lid mid-session and note what the guest sees
   on wake (informs Phase 1 re-dial UX).

**Verdict — GO, signed off by the user 2026-08-04** (all four questions
and all four static checks passed; no soft threshold breached). Gate
strand bd-l4j4ky8k closed; Phase 0 (bd-9gam4jqe) and Phase 4
(bd-jt1etjbn) unblock. Optional extras left open: a residential-class Q3
re-run (runbook above) and a human-driven session for the
Safari-app/lid-close observations. Housekeeping done: remote `spike/…`
branch deleted 2026-08-04 (throwaway; existed to run the Windows +
cross-network CI legs), ephemeral SPIKE_* secrets deleted, session token
dead with the host process; the local spike worktree
(`.worktrees/bd-l4j4ky8k-live-share-feasibility-gate`) is kept for
reference until Phase 1 re-implements under tests — its code is still
never to merge.

## Phase 0 — Scaffold

No behavior; keep it short. Dependency due-diligence (size/build-time
measurement, licensing, Windows, WASM closure) lives in Gate 0 — Phase 0
only re-establishes those facts on the real crate wiring, since the gate
proved them on a throwaway branch that never merges.

- [x] Add `crates/quarto-p2p` (lib) to the workspace; deps as above; empty
      public API stubs behind `todo!()` are fine at this point
      *(done 2026-08-04: `PreviewShareTicket`, `TunnelHost`/`TunnelHostHandle`,
      `TunnelClient`/`TunnelClientHandle`, `TunnelStatus`, `TunnelError`;
      all method bodies `todo!("Phase 1 (bd-v8mwzpmi)")`; deps iroh 1.0.3
      (default features), iroh-tickets 1.0, subtle 2, rand 0.9,
      tokio +io-util+net, tracing, thiserror; `[workspace.dependencies.quarto-p2p]`
      entry added for Phase 2's consumer)*
- [x] `cargo build --workspace` green; confirm `cargo xtask verify
      --skip-hub-build` unaffected *(done 2026-08-04: workspace build
      green; `cargo xtask verify --skip-hub-build` → "All verification
      steps passed!", output inspected)*
- [x] Re-confirm the Gate 0 static checks on the real scaffold:
      `cargo tree -i iroh` from `wasm-quarto-hub-client` fails; a Windows
      build compiles the crate (manual or release-workflow leg —
      test-suite CI has no Windows matrix entry); the dep set matches what
      Gate 0 measured (if it drifted — e.g. feature changes — re-measure
      Q4 and update the gate section's numbers)
      *(2026-08-04: **WASM closure PASS** — `cargo tree -i iroh`
      from `crates/wasm-quarto-hub-client` → "package ID specification
      'iroh' did not match any packages"; **dep set PASS** — Cargo.lock
      additions are name+version-identical to the gate branch's lockfile
      (141 external packages; iroh 1.0.3, iroh-tickets 1.0.0; only
      symmetric diff is `quarto-p2p` vs `q2-p2p-spike`), so no Q4
      re-measure; **Windows: gate coverage accepted** — user decision
      2026-08-04: no dedicated Windows leg for Phase 0; the gate proved
      the identical dep set on windows-latest (run 30894960520,
      2026-08-04) and only the ~90-line stub crate is new, so the
      Windows signal rides the release workflow / later CI instead)*

## Phase 1 — `quarto-p2p` core (TDD)

Tests live in `crates/quarto-p2p/tests/integration/` per the integration-test
layout rule (single `main.rs` binary). All tests hermetic: `presets::Minimal`,
`RelayMode::Disabled`, explicit loopback `TransportAddr::Ip` addrs — **no n0
infrastructure in CI**.

**Test specs (write these first, watch them fail):**

*(all landed failing-first in `c146ca6d` — 10/10 FAIL via `todo!()` stubs,
output inspected — then went green with the implementation, 2026-08-05;
suite lives in `crates/quarto-p2p/tests/integration/{ticket,tunnel}.rs`)*

- [x] `ticket::roundtrip` — ticket with relay + ip addrs + token →
      `to_string()` (starts with `q2preview`) → `parse()` → equal
- [x] `ticket::rejects_garbage_and_foreign_kinds` — empty string, random
      base32, a bare iroh `EndpointTicket` string (`endpoint…`) all fail with
      a typed error
- [x] `ticket::debug_redacts_token` — `format!("{ticket:?}")` does not contain
      the token bytes/hex *(also asserts Debug does not embed the full join
      string, which would leak the token via base32)*
- [x] `tunnel::http_roundtrip_loopback` — tiny axum server as target; host
      endpoint + `TunnelHost::spawn`; client endpoint + `TunnelClient::bind`;
      raw HTTP/1.1 GET through the client's local port returns the body;
      repeat over ≥8 **concurrent** connections (concurrent QUIC streams)
- [x] `tunnel::websocket_frames_survive` — target is an axum `/ws` echo;
      `tokio-tungstenite` client through the local port; upgrade + a few
      frames round-trip (proves the splice handles long-lived duplex traffic)
- [x] `tunnel::wrong_token_rejected` — stream with a wrong/short token is
      reset; the target server sees **zero** TCP connections (count accepts)
- [x] `tunnel::client_redials_after_connection_loss` — drop the host-side
      connection; next local TCP conn succeeds after client re-dial
      *(host restarted with fixed secret key + token + UDP port so the
      unchanged ticket stays valid; asserts the status watch flips to
      `Reconnecting` and back to `Connected`)*
- [x] `tunnel::half_close_propagates` — guest-side TCP write-half shutdown
      reaches the target as read-EOF (and the reverse direction), while the
      other direction keeps flowing; guards the splice's EOF ↔
      `SendStream::finish()` mapping, which `websocket_frames_survive`'s
      symmetric traffic does not exercise
- [x] `tunnel::clean_shutdown` — `shutdown()` on both handles completes
      without hangs and unbinds the local port

**Implementation:**

*(implementation complete 2026-08-05; verification: 10/10 crate tests
green, `cargo nextest run --workspace` 10873 passed, `cargo xtask verify
--skip-hub-build` "All verification steps passed!", `cargo tree -i iroh`
from `wasm-quarto-hub-client` still fails — output inspected for all)*

- [x] `ticket.rs` — struct + `iroh_tickets::Ticket` impl + `FromStr`/`Display`
      *(postcard wire format follows iroh-tickets' versioned-enum convention:
      `TicketWireFormat::Variant1 { id, addrs, token }`; manual `Debug`
      redacts the token)*
- [x] `host.rs` — `Endpoint` (preset injectable for tests) + `Router` with a
      `ProtocolHandler` whose `accept()` loops on `accept_bi()`, spawning per
      stream: `read_exact` 32-byte token under a 10 s timeout → constant-time
      compare → `TcpStream::connect(target)` →
      `copy_bidirectional(&mut tokio::io::join(recv, send), &mut tcp)`;
      log `remote_id().fmt_short()` per connection. **Half-close:**
      `copy_bidirectional` propagates read-EOF as `poll_shutdown` on the
      opposite writer; via `tokio::io::join` that must land as
      `SendStream::finish()` (QUIC FIN), and a stream FIN must become TCP
      write-shutdown — verify with `tunnel::half_close_propagates` rather
      than assuming the adapter chain does it
      *(done; preset injection is `TunnelHostConfig { preset, secret_key,
      token, bind_addr }` — the last three exist for the hermetic
      restart-same-identity re-dial test. N0 spawn wraps `online()` in the
      plan's 10 s timeout and warns + degrades to direct/LAN-only on miss)*
- [x] **QUIC keep-alive vs. browser connection pooling:** browsers hold idle
      pooled HTTP/1.1 connections open for minutes; if the iroh connection's
      idle timeout fires in between, the next request on a pooled TCP conn
      fails before the client re-dials. iroh's defaults already cover this —
      keep-alive 5 s vs. 30 s connection idle timeout
      (`iroh/src/endpoint/quic.rs:155-161`, `socket.rs:109`; connection idle
      is the noq default 30 s) — so this item is a verifying test, not new
      config. If we ever do override: the type is `QuicTransportConfig` /
      `QuicTransportConfigBuilder` set via `Builder::transport_config`
      (`endpoint.rs:669`) — **not** `TransportConfig`, which in iroh v1 is an
      unrelated internal socket-transport enum. Do not rely on the SPA's
      health polling to keep the tunnel warm
      *(verified by `tunnel::idle_pooled_conn_survives_quic_keepalive`:
      35 s fully-idle pooled HTTP/1.1 conn through default-config hermetic
      endpoints, then a second request on the same conn succeeds — ~40 s
      runtime by design, the slowest test in the crate)*
- [x] `client.rs` — endpoint + `MemoryLookup` seeded from the ticket;
      `TcpListener` accept loop; per conn: `open_bi()` on the current
      connection (re-dial with expo backoff on failure), write token, splice;
      status watch channel for CLI messaging
      *(re-dial is owned by a supervisor task parked on `conn.closed()` —
      backoff 250 ms → 5 s cap, per-attempt 10 s connect timeout; per-conn
      handlers wait on the status watch, budget 30 s, then drop the TCP
      conn so browser/health-supervisor retries stay cheap. Initial dial
      failure is a `TunnelClient::bind` error by design — Phase 3 wants
      "host unreachable" at join time, not a silent background retry)*
- [x] Shutdown plumbing: `router.shutdown().await?` on the host (it closes
      the endpoint itself; handle the returned `JoinError`); abort accept
      loop + close endpoint on the client
      *(client shutdown awaits the aborted accept-loop task so the local
      port is provably unbound before returning — asserted by
      `tunnel::clean_shutdown`)*

## Phase 2 — `q2 preview --share` (host)

*(implemented 2026-08-05, bd-jhvkwosw; tests landed first and were
observed failing — CLI tests via E0026 missing-field compile errors (the
expected failure mode for a structural clap addition), share-glue tests
4/4 FAIL at runtime on `todo!()` stubs, output inspected — then went
green with the implementation)*

**Tests first:**

- [x] CLI: `--share` parses; `--share --join x` rejected. **New tests, not an
      extension** — `crates/quarto` has no clap parse tests today
      (`preview.rs:625-647` are boot-URL formatting tests; the one existing
      exclusion is a runtime bail at `preview.rs:70`), so build the small
      `try_parse_from` harness this plan's conflict matrices need
      *(`cli_parse_tests` in `crates/quarto/src/main.rs`: parses, defaults
      off, composes with `--allow-edit`, conflicts with `--join` — the
      conflict asserted as `ErrorKind::ArgumentConflict`, so it pins a real
      `conflicts_with`, not an unknown-arg rejection)*
- [x] `quarto-preview` unit: share glue produces a ticket whose tunnel target
      is `127.0.0.1:{config.port}` (the port is resolved CLI-side before the
      server starts, `preview.rs:114-117` — `on_ready` does *not* carry it);
      ticket line printed via an injected writer/callback — do not scrape
      stdout
      *(`crates/quarto-preview/tests/integration/share.rs`, hermetic-iroh:
      the glue test fetches a marker through a `TunnelClient` bound to the
      minted ticket, proving the target; the banner arrives via an injected
      closure. Extra banner tests: capability wording per `--allow-edit`,
      direct/LAN-only notice when the ticket has no relay addr, join line
      always last (copy-paste contract))*

**Implementation:**

- [x] `PreviewArgs::share` + clap flag (`crates/quarto/src/main.rs` Preview
      variant) → `PreviewConfig::share` (`crates/quarto-preview/src/lib.rs`)
      *(also declared `--join <TICKET>` in clap now — hidden
      (`hide = true`) with `conflicts_with = "share"` and a runtime
      "not implemented yet (Phase 3)" bail — because the conflict test
      needs the arg to exist; Phase 3 unhides it, implements the guest
      path, and adds the full conflict matrix)*
- [x] `quarto-preview` → `quarto-p2p` dep; when sharing: generate token, bind
      endpoint (`presets::N0`), `timeout(Duration::from_secs(10), online())`
      (on timeout: proceed, warn "relay unreachable — direct/LAN connections
      only"), `TunnelHost::spawn` targeting `config.port`. **Print timing:**
      the `on_ready` callback receives only `Arc<HubContext>` and fires
      *before* the listener binds (`server.rs:1915` vs. `:1933`) — so a
      ticket printed there precedes accept. That is the same property as the
      existing CLI boot-URL print (`preview.rs:136-141`) and is acceptable
      (a too-fast guest just retries via its health supervisor), but the
      print does not need `on_ready` at all — the ticket's only inputs
      (`config.port`, token, endpoint addr) exist before the server starts:

      ```
      Sharing this preview session (end-to-end encrypted via iroh).
        Anyone with this string can VIEW the project and RE-RUN its code:
        [and EDIT files on this machine — only if --allow-edit]

          q2 preview --join q2preview<...>
      ```

      The ticket string will be long — **measured** with a
      scratch impl against crates.io iroh-base/iroh-tickets 1.0 (postcard
      of `{id, addrs, token}`, KIND `q2preview`, real n0 relay hostname):
      173 chars relay-only, 235 chars typical (relay + 2×IPv4 + IPv6),
      336 chars with 5×IPv6 — i.e. 3–5 wrapped lines at 80 columns. The
      same experiment validated that the planned wire shape round-trips
      through `encode_string`/`decode_string` exactly as the trait docs
      promise. Print the `q2 preview --join …` line on its own line with
      nothing after it, so a triple-click / drag copy survives terminal
      wrapping; the end-to-end check below must include copy-pasting the
      wrapped line from a real terminal
      *(done: `crates/quarto-preview/src/share.rs` —
      `start_share_session(TunnelHostConfig, host, port, allow_edit,
      announce)` → `ShareSession { ticket, handle }` +
      `format_share_banner`; called from `run_with_on_ready` before the
      server starts (with a `port != 0` guard — library callers must
      pre-resolve like the CLI does). Two deviations-with-reasons from the
      sketch: (1) the banner's relay-unreachable warning is driven by
      inspecting the minted ticket via a new
      `PreviewShareTicket::has_relay_addr()` — quarto-p2p's
      `tracing::warn!` is invisible at the CLI's default `quarto=warn`
      filter, so the banner is the user-visible signal; (2) the tunnel
      target is `share_target(host, port)` rather than hardcoded
      `127.0.0.1` so `--share` still works when `--host` binds a concrete
      non-loopback interface — unspecified binds (`0.0.0.0`/`::`) and
      hostnames still map to loopback)*
- [x] Ctrl-C: tunnel shutdown joined into the existing graceful-shutdown path
      (before the `TempDir` drop)
      *(in `run_with_on_ready`: `ShareSession::shutdown()` runs after
      `run_server_with` returns — i.e. after the hub's final filesystem
      sync — and before control returns to the CLI where the ephemeral
      `TempDir` drops; verified live in the e2e below: SIGINT → graceful
      shutdown logs → `EXIT=0`)*
- [x] **End-to-end (mandatory, record invocation + output here):** two
      terminals on one machine — host `--share` in a fixture project, guest
      `--join`; `curl http://127.0.0.1:<guest-port>/health` shows the host's
      `index_document_id`; browser on the guest port renders the document;
      live edit on host propagates
      *(executed 2026-08-05 — see "Phase 2 end-to-end record" below; the
      guest side used `cargo run -p quarto-p2p --example tunnel-client`
      since `--join` itself is Phase 3)*

### Phase 2 end-to-end record (2026-08-05)

All output inspected; the guest was the new `tunnel-client` example
(`crates/quarto-p2p/examples/tunnel-client.rs`) because the real `--join`
lands in Phase 3 — the tunnel path exercised (ticket parse →
`TunnelClient::bind` → local proxy) is exactly what Phase 3 will wrap.

- **Host** (fixture project: `_quarto.yml` + `index.qmd` + `about.qmd`):
  `q2 preview <fixture> --share --no-browser --port 9377` printed the
  boot URL, then the banner:

  ```
  Sharing this preview session (end-to-end encrypted via iroh).
  Anyone with the join string below can VIEW the project and RE-RUN its
  code on this machine:

  q2 preview --join q2previewadtdnwynvuwfau3ybfdlxih7yyexrw5xms6ew6dnoix7g3y…
  ```

  (224-char ticket, within the plan's 173–235 estimate; no direct/LAN
  warning — the n0 relay was reachable.)
- **Guest:** `tunnel-client <ticket> 9280` → "joined shared preview
  session: http://127.0.0.1:9280/", status `Connected`. The ticket was
  copy-pasted from the host's captured stdout and parsed round-trip;
  the interactive triple-click-on-a-wrapped-terminal-line check still
  wants a human eyeball (noted for the Phase 3 e2e, which a user drives).
- **`/health` through the tunnel** returned the byte-identical payload to
  direct, including `"index_document_id":"2GQkn7ADdeaLnaME5mQBo6eFnJvi"`
  and `"qmd_file_count":2`.
- **Browser (Playwright Chromium 1223** at `http://127.0.0.1:9280/?page=index.qmd`,
  frames scanned per the Gate 0 iframe finding): document rendered
  through the tunnel in **1.47 s**; screenshot inspected (`MARKER-0`
  visible).
- **Live edit:** host-side `MARKER-0`→`MARKER-1` write propagated to the
  guest browser in **1.07 s**; post-edit screenshot inspected (rendered
  text shows `MARKER-1`).
- **Ctrl-C:** SIGINT to the host → "Received Ctrl-C, initiating graceful
  shutdown…" → final filesystem sync (3 docs, 0 errors) → process
  `EXIT=0`; the guest's status watch flipped `Connected → Reconnecting`.

## Phase 3 — `q2 preview --join <ticket>` (guest)

*(implemented 2026-08-06, bd-6y0p1bne; tests landed first and were
observed failing — 5/6 new CLI conflict tests FAIL (only the Phase 2
`--share` conflict pre-existed), the two new quarto-p2p status tests
fail as E0432/E0599 compile errors (the expected failure mode for the
`TunnelStatus` shape change, matching Phase 2's E0026 precedent) —
then went green with the implementation)*

**Tests first:**

- [x] CLI conflict matrix: `--join` × each of {path, `--share`,
      `--no-project`, `--allow-edit`, `--ui editor`, `--data-dir`,
      `--preview-dir`} rejected; × {`--port`, `--no-browser`, `--host`} accepted
      *(done except `--ui editor`: the `--ui` flag itself is Phase 4
      (bd-jt1etjbn, not yet implemented), so its `--join` conflict lands
      there with the flag — noted in the Phase 4 items below. Conflicts
      asserted as `ErrorKind::ArgumentConflict`; the accepted set is
      pinned by `preview_join_composes_with_guest_flags`)*
- [x] **The money test** (integration, `crates/quarto-preview` or `quarto-p2p`
      with a dev-dep on `quarto-hub`): start a real preview hub in-process on
      a fixture project (`run_server_with`), `TunnelHost` in front of it,
      `TunnelClient` on a random port (all hermetic-iroh); then through the
      guest port: (a) `GET /health` → 200 with the host's
      `index_document_id`; (b) `repo.dial_websocket("ws://127.0.0.1:{guest}/ws")`
      with an in-memory samod client → load `IndexDocument` → files map
      matches the fixture. This proves automerge-sync-over-tunnel without a
      browser. Patterns: `dial_websocket` per
      `crates/quarto-hub-provider/tests/integration/relay_sync.rs:54` (works
      here because preview's `/ws` takes no credentials — **verified**:
      preview sets `auth_config: None`,
      `quarto-preview/src/lib.rs:407-410`, and `ws_handler` skips
      credential *and* Origin checks entirely when no auth config is set,
      `quarto-hub/src/server.rs:1607,1642-1644`); in-memory repo +
      `IndexDocument::load` per `join.rs:30-56` — but note `join.rs` itself
      dials via `repo.dial(BackoffConfig, BearerDialer)` because
      `dial_websocket` cannot set auth headers (that distinction matters
      again in Phase 5)
      *(landed as `crates/quarto-preview/tests/integration/join_tunnel.rs::guest_syncs_project_through_tunnel`,
      via `quarto_preview::run` (which wraps `run_server_with`) — no new
      dev-dep needed beyond `url` (samod + quarto-hub are already regular
      deps; samod's `tungstenite` feature unifies in via quarto-hub).
      Composition-of-tested-parts, so it passed on first run — that is
      the expected outcome for this proof, unlike the fail-first items
      above. Also asserts tunnel `/health` == direct `/health`)*

**Implementation:**

- [x] `--join <STRING>` arg; guest path in `commands/preview.rs::run` that
      bypasses project resolution/TempDir/hub entirely: parse ticket,
      `TunnelClient::bind(("127.0.0.1"|--host, --port or probed))`, print URL
      *(clap arg unhidden + `conflicts_with_all`; `run_join` in
      `commands/preview.rs`; `--host` resolves via `tokio::net::lookup_host`
      so `localhost` works; explicit `--port` gets the friendly
      `validate_explicit_port` check, otherwise the OS assigns and
      `TunnelClient::bind` reports the bound port back — no pre-probe
      needed since nothing must print before a second server starts)*
- [x] Browser-open readiness = first successful `GET /health` **through the
      tunnel** (extend `wait_until_accepting`, `preview.rs:294-327` — a local
      TCP accept alone would lie when the tunnel is dead)
      *(new `wait_until_healthy` + `health_get_ok` (hand-rolled HTTP/1.1
      GET, no new HTTP-client dep): same backoff shape and
      open-anyway-on-timeout floor as `wait_until_accepting`, 15 s budget,
      5 s per-attempt cap; unit tests cover 200 / non-200-keeps-polling /
      nothing-listening)*
- [x] Status messaging from the client watch channel: "connected via
      <direct|relay>", "reconnecting…". API pinned:
      `Connection::paths()` returns a `PathList` whose `Path` entries
      expose `is_selected()` / `is_relay()` / `is_ip()` / `rtt()`
      (`../iroh/iroh/src/socket/remote_map/remote_state/path_watcher.rs:446-494`);
      `paths_stream()` / `path_events()` give live snapshots for the
      watch channel (`endpoint/connection.rs:1144-1176`). The selected
      path's `is_relay()` is the direct-vs-relay discriminator.
      (`Endpoint::remote_info(EndpointId)` also exists, `endpoint.rs:1623`,
      but the per-connection API is the right one here)
      *(API change in quarto-p2p: `TunnelStatus::Connected` now carries a
      `PathKind` ({Direct, Relay, Unknown}, Display-able), fed by a
      per-connection `paths_stream()` watcher task (used over
      `path_events()` because it yields the current snapshot on first
      poll — no missed initial selection). A `conn_generation` counter
      guards against a dying connection's straggler snapshot overwriting
      the re-dialed connection's kind. `futures` promoted from dev-dep to
      dep for `StreamExt`. Pinned by `tunnel::status_reports_direct_path_kind`)*
- [x] Ctrl-C teardown; clear error UX for: malformed ticket, host unreachable,
      token rejected (host rotated/session ended)
      *(malformed → parse error naming `q2 preview --share` + the
      wrapped-line copy warning, exit 1; unreachable → bounded 10 s dial
      timeout then "could not reach the share host", exit 1; token
      rejected → second quarto-p2p API change: the client supervisor
      inspects `conn.closed()` and maps an `ApplicationClosed` carrying
      the host's `ERROR_CODE_UNAUTHORIZED` (1, now a crate-shared const)
      to a **terminal** `TunnelStatus::Rejected` — no re-dial spin, since
      the same token can never succeed — which the CLI turns into "the
      host rejected this join string… ask for a fresh `--share` string"
      and a non-zero exit. Pinned hermetically by
      `tunnel::rejected_token_flips_status_terminal` (client side; the
      target-sees-zero-TCP-conns half was already pinned by Phase 1's
      `wrong_token_rejected`). Ctrl-C: select on `ctrl_c()` vs the
      status reporter, then `TunnelClientHandle::shutdown()`; measured
      29 ms exit when Connected, 3.0 s when Reconnecting (iroh's
      documented graceful-close budget with a dial in flight —
      accepted))*
- [x] **End-to-end (mandatory, record here):** cross-machine host/guest run
      with the real n0 relay path (netns is Linux-only — same logistics as
      Gate 0 Q3: two physical machines); inspect rendered output in the
      guest browser; note "verified in browser". Join **two guests
      concurrently** at least once (v1 scope is N guests — architecture
      scope note)
      *(both legs done: single-machine 2026-08-06 with two concurrent
      guests, cross-machine 2026-08-06 via a GH-Actions guest — see the
      records below. The only remaining human-eyeball nicety is the
      wrapped-ticket triple-click copy check in a real terminal —
      non-blocking, same status as Phase 2's note)*

### Phase 3 end-to-end record (single-machine legs, 2026-08-06)

All output inspected. Binary: `target/debug/q2` at the Phase 3 tree
(guests use the real `q2 preview --join` — no example shims). Fixture:
`_quarto.yml` + `index.qmd` (`MARKER-0`) + `about.qmd` in a scratchpad
dir.

- **Host:** `q2 preview <fixture> --share --no-browser --port 9377`
  printed the boot URL, then the banner ending in a bare
  `q2 preview --join q2previewadwfpfxpcncsiqj2qf6…` line (203-char
  ticket; relay reachable, so no direct/LAN warning).
- **Two concurrent guests:** `q2 preview --join <ticket> --no-browser
  --port 9280|9281` → each printed
  `→ http://127.0.0.1:928x/` and `● connected via direct connection`.
- **`/health` through both guest ports** returned **byte-identical**
  payloads to direct (`diff` clean), including
  `"index_document_id":"4VQ27RYYrpZ5W3D5Fph6NE5tmYoy"` and
  `"qmd_file_count":2`.
- **Browser (Playwright Chromium** at `…/?page=index.qmd`, frames
  scanned per the Gate 0 iframe finding): document rendered through the
  tunnel on **both** guests in **1.46 s**; screenshots inspected
  (`MARKER-0` visible). Note: guests ran `--no-browser` and Playwright
  drove the pages, so the auto-open itself wasn't exercised — its
  gating helper (`wait_until_healthy`) is unit-tested and the opener is
  the same `open_browser_or_log` host mode uses.
- **Live edit:** host-side `MARKER-0`→`MARKER-1` propagated to the
  already-open guest page in **~0.6 s** (marker visible 2.2 s after the
  edit including a second page's fresh boot + both screenshots); a
  fresh guest boot after the edit rendered `MARKER-1` in 1.45 s.
  Post-edit screenshots inspected on both guests.
- **Error UX, all observed:** `--join not-a-ticket` →
  `invalid join string (wrong prefix, expected q2preview)` + guidance,
  `EXIT=1`; SIGINT of the host flipped the surviving guest to
  `○ connection lost — reconnecting…`; joining the dead host's ticket
  failed after the bounded 10.0 s dial timeout with
  `could not reach the share host …`, `EXIT=1`. Token-rejected is
  covered hermetically (`rejected_token_flips_status_terminal`) — not
  reproducible from the CLI without hand-crafting a wrong-token ticket.
- **Ctrl-C teardown:** guest exit measured **29 ms** in the Connected
  state (port unbound, `Received Ctrl-C, leaving the shared session…`);
  **3.0 s** in the Reconnecting state (iroh close with a dial in
  flight). Host Ctrl-C behavior unchanged from Phase 2's record.
- **Verification at this tree:** `cargo build --workspace` green;
  `cargo nextest run --workspace` **10897 passed**; `cargo xtask verify
  --skip-hub-build` → "All verification steps passed!"; `cargo tree -i
  iroh` from `wasm-quarto-hub-client` still fails (closure clean).

### Phase 3 end-to-end record (cross-machine n0-relay leg, 2026-08-06)

Executed via a **GH-Actions guest**, same logistics as Gate 0 Q3: live
host on the dev machine (`q2 preview <fixture> --share --no-browser`
— fixture with `MARKER-0`, plus a loop bumping `MARKER-N` every 20 s
and logging host bump timestamps in epoch-ms), guest = `ubuntu-latest`
runner (Azure network) that builds **the real `q2` binary** and joins
with **the real `q2 preview --join`** — no spike shims anywhere.
Throwaway workflow `spike-p3-join-guest.yml` + driver
`spike/p3-guest-driver.mjs` on branch `spike/bd-6y0p1bne-p3-cross-e2e`
(commit 07d945a1, never merged); ticket passed through the ephemeral
`SPIKE_P3_TICKET` repo secret. **GH run 31092359776** (job green in
12m13s, most of it the q2 build). All output and both screenshots
downloaded from the `p3-guest-evidence` artifact and inspected.

- **CLI status surface:** each guest printed exactly one
  `● connected via relay` — the Azure↔residential pair never
  hole-punched a direct path (exactly the relay-fallback scenario this
  leg exists for), and zero `reconnecting`/`rejected` events over the
  whole session. This is the leg that exercises the
  `PathKind::Relay` rendering (local runs only ever show `direct
  connection`).
- **`/health` through the tunnel** (both guests, guest2 concurrent
  with guest1): correct payload with the host's
  `"index_document_id":"4JGt98WMiAbWRuCfaDwmp3NmPuga"`,
  `"qmd_file_count":2`.
- **Browser (headless Chromium on the runner):** first render through
  the real n0 relay in **12.7 s**, ~**47.5 MB** fetched (uncompressed
  WASM dominates — same payload confounder as Gate 0; its
  HTTP-compression mitigation remains the first lever). Boot
  screenshot shows `MARKER-33` rendered; final shows `MARKER-37`.
- **Live-edit propagation over 4 marker bumps** (runner-observed ts −
  host bump ts; includes the driver's 150 ms poll grain and
  host↔runner NTP skew): **899 / 979 / 1043 / 1563 ms, median
  ~1.0 s** — consistent with Gate 0's 0.81–2.34 s and the local leg.
- **n0 relays from the runner:** euc1-1 and use1-1 probed 200/OK in
  0.57 s / 0.32 s.
- Caveats unchanged from Gate 0: runner egress is datacenter-class,
  not residential; a human-driven Safari-app/lid-close session remains
  an optional extra.
- **Housekeeping done 2026-08-06:** `SPIKE_P3_TICKET` secret deleted,
  remote `spike/bd-6y0p1bne-p3-cross-e2e` branch deleted (workflow was
  throwaway, never merged; local branch kept for reference), host
  process stopped — the session token died with it.

## Phase 4 — `q2 preview --ui editor` (independent track)

Serve the **full hub-client editor** from the preview server instead of the
read-only SPA. Zero hub-client *source* changes expected — we reuse the
`#/share/` route with a relative `server` (hub-client resolves relative sync
URLs against the page origin: `hub-client/src/utils/routing.ts:51-57`; share
params parsed at `routing.ts:226-239`, all three of `server`/`file`/`name`
required: `App.tsx:418-423`).

**Tests first:**

- [ ] CLI: `--ui viewer` / `--ui editor` parse (clap `ValueEnum`, default
      `viewer`); an unknown value (`--ui monaco`) is rejected with the list
      of valid values; **`--ui` × `--join` rejected** (the one Phase 3
      conflict-matrix entry deferred here because the flag didn't exist
      yet — extend `--join`'s `conflicts_with_all` in `main.rs` and add
      the parse test alongside Phase 3's `assert_join_conflict` helper)
- [ ] Rust unit: `--ui editor` boot URL builder emits
      `http://{host}:{port}/#/share/{indexDocId}?server=%2Fws&file={rel}&name={project}`
      (doc id **without** the `automerge:` prefix — `routing.ts:420`; `file`
      falls back to the first `.qmd` when no initial page was resolved; note
      params ride the **hash fragment**, not the URL query)
- [ ] Rust unit: `--ui editor` leaves the write policy alone: with
      `--allow-edit` → `DiskWritePolicy::WriteBack`; without →
      `DiskWritePolicy::ReadOnly` **and** the ephemeral-session note is
      emitted ("session edits are ephemeral — pass `--allow-edit` to
      persist to disk") — assert via an injected writer/callback, not
      stdout scraping (same style as Phase 2's ticket-line test)
- [ ] Build-time: embed-dir placeholder fallback works on a tree without the
      editor dist (mirror the existing `QUARTO_PREVIEW_EMBED_DIR` placeholder
      test story, `crates/quarto-preview/build.rs:19-43,60-107`)
- [ ] `cd hub-client && npm run build:all` still green (CRITICAL per
      CLAUDE.md); new embed build produces a servable dist

**Implementation:**

- [ ] hub-client: `build:preview-embed` npm script — no `VITE_GOOGLE_CLIENT_ID`
      (auth UI off: `App.tsx:104`), `VITE_DEFAULT_SYNC_SERVER=/ws`, outDir
      `dist-preview-embed/` via `vite build --outDir`. Note: hub-client has
      **no** alternate-config/outDir precedent to copy — the only prior art is
      env-var-at-build-time (`build:local-prod`,
      `scripts/build-local-prod.sh:11`); the script must keep the 4 HTML
      entry points (main, debug, q2-debug, q2-preview) and run after the
      `build:wasm` + `build:sandboxed` pre-steps. **hub-client change ⇒
      two-commit changelog rule**
- [ ] xtask: `build-hub-client-embed` (sibling of `build_q2_preview_spa.rs`)
- [ ] `quarto-preview/build.rs`: second build-script-**emitted** env
      (`cargo:rustc-env=QUARTO_HUB_CLIENT_EMBED_DIR=…` — mirroring how
      `QUARTO_PREVIEW_EMBED_DIR` actually works: build.rs emits it at :30-33,
      nothing reads it from the environment) + `include_dir!` + placeholder
      fallback; runtime: `--ui editor` flips which dir `spa_handler`
      (`lib.rs:490-509`) serves
- [ ] `--ui` flag (clap `ValueEnum` `PreviewUi { Viewer, Editor }`, default
      `Viewer`) → boot URL in share-route form when `editor`. **Structural
      change required:** today the CLI builds *and prints* the boot URL and
      captures it in the browser-open task (`preview.rs:136-141`, :158-180)
      before `quarto_preview::run` is even called (:224), while the index doc
      id only exists server-side (`ctx.index().document_id()`,
      `quarto-hub/src/index.rs:138` — bare id; the share route wants it
      bare), reachable in `on_ready`. So move boot-URL construction into the
      library, or channel the doc id from `on_ready` back to the CLI's
      print + browser-open path — "available in `on_ready`" is not enough by
      itself. No write-policy coupling — without `--allow-edit`, emit the
      ephemeral-session note instead of flipping `DiskWritePolicy`
- [ ] Dedupe the shared `wasm_quarto_hub_client_bg.wasm` across the two
      embeds. **Decided from measured numbers:** the artifact
      is 38,371,765 bytes and **byte-identical** in both dists (sha256
      `a075c962…`), and Vite's content hashing even gives it the same
      hashed filename in both (`wasm_quarto_hub_client_bg-B4wtBy8i.wasm`)
      since the hash is content-derived. hub-client dist ≈ 67 MB,
      q2-preview-spa dist ≈ 45 MB — a naive double-embed adds ~67 MB to
      `q2`, ~38 MB of it pure duplication. So: serve the `.wasm` from a
      single shared embed; the identical content-hashed filename makes
      "strip from one dist, route both asset paths to the shared copy" the
      natural mechanism (exact design in this phase). Still record the
      final binary delta after dedupe
- [ ] Known warts to document in `--help` + here: hub-client persists a
      ProjectEntry + IndexedDB automerge cache per ephemeral session (stale
      entries accumulate across preview restarts — follow-up strand);
      `--share --ui editor` means the *host* picks the UI for all guests
- [ ] End-to-end (mandatory): real browser session — editor loads, file
      sidebar shows the project, Monaco edit persists to host disk (verify
      file content on disk), preview pane updates

## Phase 5 — Spike: `--join https://quarto-hub.com/#/share/…`

Design-doc question: can `--join` also target a hosted hub share URL? The
honest answer is "auth is the hard part" — so this phase is a **spike + design
note**, not committed implementation. Deliverable: a
`claude-notes/plans/2026-XX-XX-join-hosted-hub.md` with a go/no-go and a
working prototype behind no flag stability promise.

Sketch (what research says is feasible):

- Parse the share URL (precedent: `parse_index_doc_id`,
  `crates/quarto/src/commands/provide_hub.rs:63-72`; TS twin
  `ts-packages/quarto-hub-mcp/src/share-url.ts`)
- Serve the Phase-4 hub-client embed locally; browser stays same-origin
- The blocker: hub cookie auth is same-origin with an Origin==Host check on
  `/ws` (`crates/quarto-hub/src/server.rs:1631-1636`), and browsers can't set
  WS headers. So: a local **reverse proxy** that terminates the browser's
  same-origin `/ws` and dials `wss://quarto-hub.com/ws` injecting
  `Authorization: Bearer <Google ID token>` — the hub already accepts Bearer
  on `/ws`, and both the token acquisition (hub-mcp PKCE + keyring,
  `ts-packages/quarto-hub-mcp/src/auth/*`) and the header-injecting dial
  (`BearerDialer`, `crates/quarto-hub-provider/src/dialer.rs`) have working
  precedent. `/auth/actor` (per-project actor id) needs the same proxying
- Open questions for the spike: bearer expiry mid-session (no refresh story on
  `/ws` — the hub validates once at upgrade, `server.rs:1595-1601`); whether
  q2 grows a browserless Google login flow or shells out to `q2`'s existing
  hub auth; identity/presence when several guests proxy through one bearer

- [ ] Spike + design note + go/no-go

---

# Verification (repo policy)

- Every phase: `cargo build --workspace`, `cargo nextest run --workspace`
  (never through `tail`), `cargo xtask verify --skip-hub-build`; **full
  `cargo xtask verify` for Phase 4** (hub-client + embed legs)
- TDD is non-negotiable: each phase's test list lands and **fails** before its
  implementation items. **The one exemption is Gate 0**: it is an
  investigation spike whose code is never merged; Phase 1 re-implements the
  tunnel from scratch, tests-first
- Gate 0's verdict (with the user's sign-off) must be recorded in this file
  and as a braid comment on the epic before any other phase's strand moves
  to `in_progress`
- End-to-end verification per CLAUDE.md: the checked items marked
  *(mandatory, record here)* must capture the exact invocation, an output
  snippet, and an explicit "output inspected" note in this file before the
  phase's strand closes. Unit tests alone do not close a phase
- No pushes without explicit user approval

# Risks / open questions

1. **Dependency weight.** iroh brings noq (quinn fork), netwatch, portmapper,
   hickory, reqwest. Compile time + binary size measured in Gate 0 (Q4);
   the go/conditional-go criterion is explicit there.
   (`fast-apple-datapath` uses private Apple APIs — irrelevant unless we
   ever ship via the Mac App Store.)
2. **n0 infrastructure dependency at runtime.** Relays + pkarr are n0-hosted
   defaults. Ticket embeds direct+relay addrs, so LAN joins survive an n0
   outage; cross-NAT joins do not. Acceptable for v1; self-hosted relay is the
   escape hatch. `IROH_FORCE_STAGING_RELAYS` exists for debugging.
3. **RCE surface honesty.** Sharing = letting guests run the project's code on
   the host (that's what preview re-execution *is*). Mitigated by capability
   ticket + loud print; not mitigable further without per-peer permissions.
4. **Per-peer permissions don't exist.** One `DiskWritePolicy` + one
   process-wide `allowEdit` for all peers. Follow-up strand; likely needs the
   samod fork's `AccessPolicy` hook (it exists for exactly this).
5. **Tunnel latency on the relay path.** Asset boot may be slow relayed;
   sync frames are small. The preview server serves assets **uncompressed**
   (verified: no CompressionLayer in the preview/hub stack;
   quarto-hub's tower-http features are `trace`/`cors`/`set-header` only),
   so the first lever if boot feels bad is HTTP compression
   (`tower_http::CompressionLayer` or precompressed `.wasm.br` — wasm
   compresses ~3–5×, no version-skew cost). Only after that: serving SPA
   assets from a guest-side embed while proxying only `/health|/ws|/api|/.quarto`
   — deliberately deferred (version-skew cost).
6. **Windows** for iroh-dependent tests — Gate 0 checks compile (note:
   test-suite CI is ubuntu + macos only; Windows verification is manual per
   `claude-notes/instructions/windows-dev.md`, or the `release.yml` windows
   leg); if tests misbehave see that file's exclusion guidance.

# Follow-up strands to file when the epic lands

- Per-peer edit permissions (AccessPolicy-based)
- Guest-side asset serving (relay-path boot latency)
- `AddrFilter::relay_only()` privacy knob for `--share`
- hub-client ephemeral-session IndexedDB/ProjectEntry cleanup
- samod-native `IrohDialer` for native peers (`q2 provide-hub` over iroh)

# Braid strands

- Epic: **bd-yyoyvx91**
- Gate 0: bd-l4j4ky8k · Phase 0: bd-9gam4jqe · Phase 1: bd-v8mwzpmi ·
  Phase 2: bd-jhvkwosw · Phase 3: bd-6y0p1bne · Phase 4: bd-jt1etjbn ·
  Phase 5 (spike): bd-ckra329s
- Blocking chain: G0 → P0 → P1 → P2 → P3; P4 blocks only on G0 (parallel
  track once the gate passes — the gate *is* the due diligence P4 waits
  for); P5 blocks on P3 + P4. `braid ready` should show G0 and nothing
  else until it closes (P0 and P4 unblock together when it does). The
  skein matches this chain.

# Reference index

- Preview CLI + flow: `crates/quarto/src/main.rs:195-243`,
  `crates/quarto/src/commands/preview.rs` (`run` :58-225, port probe :229-256,
  browser wait :294-327, boot URL :446-471)
- Preview server: `crates/quarto-preview/src/lib.rs` (embed :38, config
  routes :443-476, spa_handler :490-509, hub config :360-423,
  allow-edit/DiskWritePolicy :414-421)
- Hub seams: `crates/quarto-hub/src/server.rs` (`run_server_with` :1890,
  `extend_router`/`on_ready`/`on_file_changed` :1893-1895; `on_ready` fires
  :1915 *before* the listener binds :1933, ws auth :1602-1648),
  `context.rs` (`acceptor()` :441, repo build :293-316); index doc id via
  `ctx.index().document_id()` (`index.rs:138`, bare id — the SPA prepends
  `automerge:`, `PreviewApp.tsx:511-513`)
- samod transport seam: samod checkout `samod/src/transport.rs:19-81`,
  `acceptor_handle.rs:74-82`; dialer precedent
  `crates/quarto-hub-provider/src/dialer.rs`, joiner precedent `join.rs:30-56`
- SPA same-origin assumptions: `q2-preview-spa/src/PreviewApp.tsx`
  (deriveWsUrl :539-542, fetchIndexDocId :499-514, allow-edit :523-532,
  connect :818-824), `bootController.ts:1-20`
- hub-client share route: `hub-client/src/utils/routing.ts` (:35-57, :99-109,
  :226-239, :347-354, :413-431), consumption `App.tsx:409-463`
- iroh (../iroh @ v1.0.3): `iroh/src/endpoint.rs` (builder :952, connect
  :1052, accept :1165, online :1358, close :1706),
  `endpoint/presets.rs:113`, `protocol.rs` (Router :406-511),
  `endpoint/quic.rs` re-exports; `iroh-tickets` 1.0 (`Ticket` trait,
  BASE32_NOPAD + postcard); examples `echo.rs`, `echo-no-router.rs`,
  `auth-hook.rs`, `transfer.rs` (local-relay dev pattern)
- Prior plans: `2026-05-11-q2-preview-epic.md` (Phase E bullets :311-315
  (unnumbered — no "E.2/E.3" labels exist), out-of-scope
  :572-580, Q7 :416-431), `2026-06-10-q2-preview-edit-writeback.md`,
  `2026-06-11-firefox-ws-peer-timeout-fix.md`
