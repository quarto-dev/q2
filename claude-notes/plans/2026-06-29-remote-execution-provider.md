# Remote code-execution provider for hub sessions

**Strand:** bd-sfet3264 (feature, P1).
**Date:** 2026-06-29.
**Status:** Design / investigation. **Implementation gated on explicit
user approval after this plan is reviewed and iterated.**

## Goal

Let a user who has the `q2` binary "connect" to an existing
collaborative editing session (a hub-client tab, or a project on
`quarto-hub.com`), authenticate, and **announce that this client is
willing to execute code**. When any player in the session asks for a
document's code to be executed, the connected `q2` client runs the
engines (knitr/jupyter — the same machinery `q2 preview` already
uses) and **deposits the execution results into the automerge session
so every player sees the executed output** — not just raw `{r}` /
`{python}` source.

Today only `q2 preview` shows executed output, and only to its own
embedded preview SPA against a *local, ephemeral* hub. hub-client and
quarto-hub.com render **source-only** (no capture consumption). This
plan brings server-side execution to the shared, persistent,
multi-player session.

## The crucial finding: the result transport is already automerge-native

The user's instinct — "refactor the q2 preview execution-result
channel into a mode usable by hub-client" — turns out to be *already
most of the way done*. The execution-**result** path in `q2 preview`
does **not** ride on loopback HTTP and is **not** stored in a VFS
file. It is already a set of automerge documents synced over the same
samod WebSocket as the project files:

1. The server runs engines and produces a `Vec<EngineCapture>`
   (`EngineCapture { engine_name, input_qmd, result }`,
   `crates/quarto-trace/src/lib.rs:186`).
2. It serializes them to gzipped JSON and stores them as a **separate
   samod/automerge binary document** (`create_binary_document(..,
   CAPTURE_MIME_TYPE)`, written by `write_capture_doc` in
   `crates/quarto-preview/src/capture_driver.rs:326` and
   `re_execute.rs:337`). One binary doc per `.qmd` with code cells.
3. It writes a **pointer** into the project IndexDocument's `captures`
   sidecar: `CaptureRef { capture_doc_id, staleness, state,
   last_error }` keyed by the source file's relative path
   (`crates/quarto-hub/src/index.rs:7-17,68-74,272`; TS mirror in
   `ts-packages/quarto-automerge-schema/src/index.ts:40-63`).
4. Both docs sync to the browser over `/ws`. The SPA observes the
   sidecar via `onCapturesChange`
   (`ts-packages/quarto-sync-client/src/client.ts:352-377`), fetches
   the binary doc by id (`getBinaryDocById`, `client.ts:1102`), and
   threads the gzipped JSON into the WASM renderer
   (`render_page_for_preview(path, grammars, captureGzJson)`,
   `q2-preview-spa/src/PreviewApp.tsx:1030-1059`).
5. WASM splices the captured output into the live-edited AST via
   `CaptureSpliceStage` (`crates/quarto-core/src/engine/capture_splice.rs`,
   wired in `crates/wasm-quarto-hub-client/src/lib.rs:1201-1309`).
   Engines never run in the browser; the capture is treated as an
   AST-level "recipe" matched by `(content-hash, occurrence-index)`
   so it survives live prose edits (see
   `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`).

**So "deposit results into automerge in a way visible to other
players" is exactly what the CaptureRef sidecar + capture binary doc
already do.** The hub relays both to every connected peer. The
document-bloat instinct is also already handled at the *file* level:
captures live in their own binary docs, never inside the (frequently
edited) file documents.

What is **not** reusable as-is, and is the real work of this feature:

- **The execution *trigger*.** In `q2 preview` the SPA POSTs
  `/api/preview/re-execute` to the *same loopback process* that owns
  the repo (`crates/quarto-preview/src/re_execute.rs:102`). In a
  shared hub the executor is a *remote peer*; there is no loopback
  HTTP between a player's browser and the volunteer's `q2`. The
  trigger must travel **through automerge** (ephemeral message or a
  persisted request entry).
- **The executor's *role*.** In `q2 preview` the `q2` process *is*
  the hub: it owns the samod `Repo` and accepts inbound connections.
  The new executor must instead be a samod **client peer** that
  *dials out* to a remote hub and `find()`s the existing index doc.
- **Auth.** The remote hub (quarto-hub.com) requires a Bearer JWT on
  the WS upgrade. Today only the TS sync client can attach it; the
  Rust samod dialer cannot (see "Auth" below).
- **hub-client capture consumption.** hub-client currently ignores
  the `captures` sidecar and renders source-only. It must learn to
  consume captures the way q2-preview-spa does.
- **Capture retention.** An ephemeral preview repo can leak orphaned
  capture docs freely. A persistent project cannot — and samod
  exposes **no document-delete API** (`Repo` has only `create`,
  `find`, `dial_websocket`). See "Capture retention" below.

## Current architecture (verified against the code, 2026-06-29)

### Two React apps, shared runtime

| | `hub-client/` | `q2-preview-spa/` |
|---|---|---|
| Role | Full collaborative editor (Monaco, sidebar, presence, auth) | Minimal preview embedded in `q2` binary |
| Hub | persistent `quarto-hub.com` | local ephemeral hub from `q2 preview` |
| Storage | IndexedDB | memory |
| Auth | HttpOnly cookie (browser) | none (loopback) |
| Execution | **none — source-only render** | consumes captures, splices output |
| WASM | `crates/wasm-quarto-hub-client` (shared) | same |
| Shared TS | `@quarto/preview-runtime`, `@quarto/preview-renderer`, `@quarto/quarto-sync-client`, `@quarto/quarto-automerge-schema` | same |

### automerge document model

- `IndexDocument { files: Record<path, docId>, version, identities,
  captures: Record<path, CaptureRef> }`
  (`ts-packages/quarto-automerge-schema/src/index.ts:58-63`;
  `CURRENT_SCHEMA_VERSION = 2`). The "VFS" at the automerge layer is
  just `files: path → docId`; each file is its own document
  (`TextDocumentContent { text }` or `BinaryDocumentContent {
  content, mimeType, hash }`).
- The Rust hub uses **samod** (quarto-dev fork of automerge-repo,
  `samod 0.10`). `samod::Repo` is symmetric: it can `accept` inbound
  *and* `dial_websocket(url, backoff)` outbound
  (`crates/quarto-hub/src/peer.rs:18-50`, used by `q2 hub --peer`).

### Engines are Rust-native (this shapes everything)

`KnitrEngine`/`JupyterEngine` are `#[cfg(not(target_arch =
"wasm32"))]` (`crates/quarto-core/src/engine/registry.rs:52-65`).
`record_capture` (`crates/quarto-core/src/engine/preview_record.rs:130`)
and the disk cache `record_capture_cached`
(`crates/quarto-preview/src/cache.rs:151`) are Rust. **Real execution
can only happen in a native Rust process.** The browser/WASM side only
*replays* captures.

### Auth (TS-only today)

OAuth 2.0 Authorization-Code + PKCE with an RFC 8252 loopback
redirect, against Google; implemented entirely in
`ts-packages/quarto-hub-mcp/src/auth/`. Tokens live in the **OS
keyring** (`@napi-rs/keyring`, service `dev.quarto.hub-mcp`, account
`<issuer>:<client_id>`), **shared across the `q2 mcp` and `npx`
channels**. The TS sync client attaches the Bearer on every (re)connect
via `NodeWebSocketClientAdapter` (`getBearer()` →
`Authorization: Bearer <jwt>`). The Rust samod dialer **cannot set
headers** — joining an *authenticated* hub from Rust needs a custom
`BearerDialer` on samod's public `Dialer` trait (the
`2026-06-11-q2-mcp-hub-auth.md` plan judged this "feasible, no fork
changes" but deliberately chose the TS launcher to avoid a second
auth/threat-model surface).

### `q2 mcp` is the closest precedent

`q2 mcp` is a **thin Rust launcher** (`crates/quarto-mcp-launcher`)
that discovers Node, extracts an embedded esbuild bundle, injects
compiled-in OAuth client id/secret + default server, and `exec`s a
Node process that reuses `@quarto/quarto-sync-client` +
`@quarto/hub-mcp` auth to join a project's automerge session. It does
**not** execute code; it manipulates files.

## Target architecture

```
 ┌─────────────┐         automerge sync over /ws (Bearer auth)        ┌──────────────────────┐
 │ Player A    │◄──────────────── quarto-hub.com ──────────────────►│ q2 execution provider │
 │ (hub-client │       index doc + file docs + capture docs          │  (NEW subcommand)     │
 │  browser)   │                                                     │                       │
 │             │  ──(1) "execute foo.qmd" request──────────────────► │  watches for requests │
 │             │  ◄─(2) capability: "executor online: knitr,jupyter" │  runs engines (Rust)  │
 │ consumes    │  ◄─(3) CaptureRef sidecar update + capture binary ─ │  writes capture doc + │
 │ captures    │        doc (EXISTING transport)                     │  sidecar (EXISTING)   │
 └─────────────┘                                                     └──────────────────────┘
```

Steps (2) and (3) reuse existing machinery. Step (1) and the executor's
client role are new.

### Reuse map

| Concern | Reuse | New |
|---|---|---|
| Serialize execution result | `EngineCapture`, `write_capture_doc`, `create_binary_document` | — |
| Deposit result in automerge | `CaptureRef` sidecar + capture binary doc | retention/GC policy |
| Run engines | `record_capture` / `record_capture_cached`, `EngineRegistry` | engine availability handshake (announce which engines this host has) |
| Join the session | samod `Repo` + `dial_websocket` | `BearerDialer` for auth; client-peer bootstrap (no inbound acceptor) |
| Auth | `q2 mcp` OAuth/keyring (TS) | token bridge to Rust *or* Rust OAuth port (decision below) |
| Trigger execution | — | ephemeral request channel + capability announcement |
| Consume captures in the editor | q2-preview-spa's `onCapturesChange`→`getBinaryDocById`→`render_page_for_preview` | port the same into hub-client |
| Request execution from the editor | q2-preview-spa `StaleCaptureOverlay` POST | replace POST with an automerge ephemeral send |
| Clear results (D6) | Rust `IndexDocument::remove_capture` (map-key delete) | TS `SyncClient.clearCapture`/`clearAllCaptures` + hub-client affordance |

## Decisions locked (2026-06-30)

All of D1–D6 and open questions 6–8 were resolved with the user on
2026-06-30. Per-decision detail is inline below; the short form:

- **D1 — Hybrid (C).** Node owns auth; Rust owns sync + execution +
  capture-writing via a `BearerDialer`.
- **D2 — Hybrid request channel.** Ephemeral "execute now" + ephemeral
  capability beacon; persisted `CaptureRef.state`/`staleness` for
  durable status. Beacon liveness timeout = **1.5 × refresh interval**.
- **D3 — Content-addressed + dedup** now; capture docs **excluded from
  zip export** (treated as cache); a real GC design follows
  immediately after.
- **D4 — Surface to all, allow all.** Providing execution implicitly
  extends it to every player. Optional **owner-only** locked-down mode
  (only the providing user's actor id may request) as a follow-on.
- **D5 — Heartbeat claims + cooperative `--force` takeover.** Stale
  (no-heartbeat) claims auto-reclaim; a live claim needs `--force`, and
  the displaced executor stands back.
- **D6 — Per-doc clear first.** Clear-all deferred. Confirmation UX +
  in-flight race handling settle when the executor lands (Phase 4).
- **Q6 — quarto-hub.com only for v1**; mechanism is engine-agnostic.
- **Q7 — working name `q2 provide-hub`** (avoid "connect": Posit
  Connect collision). Final naming TBD; alternatives below.
- **Q8 — per-doc clearing first.**

## Key design decisions

### D1 — Where does the executor live: native Rust, TS launcher, or hybrid? — DECIDED: hybrid (C)

**Decision (2026-06-30): Option C (hybrid).** Node owns auth only;
Rust owns sync + execution + capture-writing via a `BearerDialer`.

This is the central decision; everything else flows from it.

- **Option A — Native Rust provider.** New `q2` subcommand opens a
  local samod `Repo` (memory/temp storage), dials the remote hub with
  a `BearerDialer`, `find()`s the index doc, watches for requests,
  runs engines in-process, writes capture doc + sidecar with the
  *existing Rust functions verbatim*. Self-contained single binary; no
  Node. Cost: must obtain/refresh the Bearer token in Rust (port the
  OAuth loopback+PKCE+keyring+refresh, or read the existing keyring
  entry — fragile w.r.t. refresh) and implement `BearerDialer`.
- **Option B — TS launcher (like `q2 mcp`).** Node holds the
  authenticated connection (reuse everything), detects requests, then
  shells out to `q2` to run engines and produce a capture, and writes
  the capture doc + sidecar **in TS** (duplicating Rust's
  `write_capture_doc`/`set_capture` against the TS schema). Cost:
  splits capture-writing logic across two languages; the engine
  invocation becomes an awkward subprocess boundary.
- **Option C — Hybrid (recommended starting point).** Node owns
  *auth only* (reuse OAuth/keyring/refresh untouched) and mints/hands
  a fresh Bearer to a Rust process; Rust owns *sync + execution +
  capture-writing* natively via a `BearerDialer`. Minimizes new code:
  no Rust OAuth port, no TS duplication of capture-writing. New Rust:
  `BearerDialer` + request-watch loop + capability announce. New
  glue: token hand-off (env var / fd / short-lived local socket) and
  refresh propagation.

Recommendation: **C** for the first cut — it keeps execution and the
already-working capture transport entirely in Rust while reusing the
entire TS auth surface. Revisit a fully-native A later if the Node
dependency is unwanted.

### D2 — Execution-request channel: ephemeral vs persisted

**Decision (2026-06-30): hybrid.** Ephemeral "execute now" nudge +
ephemeral capability beacon; persisted `CaptureRef.state`/`staleness`
for durable status (as preview already does).

The user specifically asked about automerge **ephemeral messages**.
Findings:

- Ephemeral is **proven** (hub-client presence:
  `handle.broadcast(msg)` + `handle.on('ephemeral-message')`,
  relayed by samod to all peers subscribed to that doc, **including
  the server peer**). It is **per-DocHandle**, **best-effort**, and
  **not persisted**.
- The sync client currently exposes only **per-file** handles
  (`getFileHandle(path)`); the **index** `DocHandle` is internal
  (`state.indexHandle`, `client.ts:174`) and would need a new exposed
  method to broadcast on.

Trade-off:

- **Ephemeral request** ("please execute foo.qmd now"): low latency,
  zero document churn — but **lost if no executor is connected at
  send time** (no durability). Good for "Run" button semantics where
  the user can retry.
- **Persisted request** (write an intent into the index doc, e.g.
  bump a `CaptureRef.requestedAt` or a `requests` map): survives
  executor reconnect, naturally deduped by CRDT — but adds index-doc
  history churn and needs cleanup.

Likely answer: **hybrid** — ephemeral for the live "execute now"
nudge and for **capability/liveness** ("an executor is online with
engines X, Y"), plus the *existing persisted* `CaptureRef.state`
(`idle`/`running`/`error`) and `staleness` for durable status the way
preview already does. Capability announcement is inherently ephemeral
(presence-like): the executor periodically broadcasts
`{ executor: true, engines: [...], actorId }`; editors show "Run"
affordances only while a capability beacon is live.

**Capability beacon liveness (decided 2026-06-30).** The executor
re-broadcasts the beacon every `BEACON_INTERVAL`; an editor marks the
executor offline if no beacon arrives within
`BEACON_TIMEOUT = 1.5 × BEACON_INTERVAL`. The 1.5× factor absorbs CRDT
propagation latency and avoids flicker, while staying tight enough that
a genuinely-disconnected provider disappears quickly (it tolerates a
late beacon, not a fully-dropped one — by design, per the user: a dead
provider should not linger as "online"). Proposed starting values
**`BEACON_INTERVAL = 3 s`, `BEACON_TIMEOUT = 4.5 s`** (both tunable;
the invariant `TIMEOUT = 1.5 × INTERVAL` is the contract, the absolute
numbers are not). The beacon carries `{ actorId, engines: [...],
generation }` so editors can both (a) gate "Run" affordances on
liveness and (b) show *which* engines are serviceable (D-Q6:
engine-agnostic mechanism, engine-specific *availability*). Open: which
DocHandle carries the beacon — the index handle (needs the new exposed
broadcast method) is the natural project-scoped channel; alternatively
a convention on a well-known per-file handle. Lean index-handle.

### D3 — Capture retention in long-lived projects — DECIDED: content-addressed dedup, GC next

**Decision (2026-06-30): content-addressed + dedup now**, with capture
docs **excluded from the project zip export** (treated as cache, never
"real" project content). A proper GC design is the **immediate
follow-on** (the user expects to want it right after) — file it as a
linked strand once this lands.

`q2 preview` creates a **new** capture binary doc on every
re-execute and orphans the previous `DocumentId`
(`re_execute.rs:144-156`). Fine for an ephemeral repo; a persistent
project would accumulate orphans **and samod has no public
document-delete API**. Options:

- **Content-addressed captures.** Key the capture doc by a hash of
  its bytes (or of `input_qmd`); reuse the existing doc when the hash
  matches (dedup), so re-running an unchanged doc creates nothing new.
  Bounds growth to "one live capture per distinct result," but stale
  results still linger.
- **In-place mutation of one capture doc per file.** Keep a single
  `captureDocId` per path and overwrite its `content` on re-execute.
  Avoids orphans, but an automerge binary doc's **history** grows
  with each overwrite (the very bloat we want to avoid). Mitigate
  only if samod gains history compaction.
- **Server-side GC.** quarto-hub.com (the persistent server, which
  *does* own its storage) prunes capture docs not referenced by any
  index `CaptureRef`. Needs a server feature + a safe "unreferenced"
  definition across branches/versions.

Recommendation: start with **content-addressed + dedup** (no
orphan-on-unchanged, simple, client-only), and file a follow-up for
server-side GC of truly-unreferenced capture docs. Confirm with the
user whether capture docs should be **excluded from the project zip
export / treated as cache** so they never become "real" project
content.

**Separate two concerns that this plan previously conflated.** There
is (a) *removing the reference* — taking a document back to its
pre-captures effective state — and (b) *reclaiming storage* — actually
freeing the orphaned binary doc bytes. (a) is a user-facing affordance
(see D6) and is cheap and fully supported. (b) is the hard
server/samod-level GC problem above. The user-facing "clear" solves
(a); it does **not** require (b) to work — a cleared document renders
source-only immediately regardless of whether the bytes are ever
reclaimed.

### D4 — Authorization model (who may execute, who may request) — DECIDED: surface to all, allow all; owner-only as follow-on

Running arbitrary `{r}`/`{python}` from a shared document is **remote
code execution on the volunteer's machine**.

**Decision (2026-06-30):**

- The executor **opts in per project/session** (explicit `provide-hub`
  invocation, never automatic).
- **All players may request execution by default.** Providing
  execution implicitly extends that capability to everyone in the
  session — a user who runs `provide-hub` is understood to be offering
  their machine to the whole room. We **surface to all players** that
  "code from this document runs on `<user>`'s machine" (a visible
  trust banner / indicator, not buried).
- **Optional owner-only locked-down mode (follow-on).** A flag (e.g.
  `--owner-only`) restricts requests to *the providing user's own actor
  id*. **This is knowable:** under auth the hub derives a per-project
  actor id `HMAC-SHA256(server_secret, sub ‖ project_id)`, exposed at
  `GET /auth/actor?project=<id>` (`crates/quarto-hub/src/server.rs:781-806`,
  `auth.rs:729`). It is **stable across the user's devices/sessions**
  and unique per project. So the executor knows its own actor id, every
  request carries the requester's actor id, and owner-only is simply
  `request.actorId == self.actorId`. Same user on a second device still
  matches (same actor id) — the intended semantics.

Default-open is the v1 posture; owner-only is a small additive gate
once the request channel carries actor ids (it does, for D5's claim
model anyway).

### D5 — hub-client capture-consumption UX

hub-client renders source-only today. Bringing in captures means:

- Wire `onCapturesChange` → `getBinaryDocById` →
  `render_page_for_preview(captureGzJson)` into hub-client's
  `ReactPreview` (port from `q2-preview-spa/PreviewApp.tsx`).
- Add a "Run" / "Re-execute" affordance that emits the D2 request and
  reflects `CaptureRef.state` (`running`/`error`) + `staleness` and
  the D2 capability beacon (disabled when no executor is online).
- Decide multi-executor behavior (more than one volunteer online):
  see the claim model below.

**Multi-executor claim model (decided 2026-06-30): heartbeat claims +
cooperative `--force` takeover.** First-claim-wins alone has the
failure the user flagged — if the claiming executor dies mid-run,
everyone else is blocked indefinitely. The fix has two parts:

- **Heartbeat staleness handles the offline case automatically.** A
  claim is a CRDT entry (per-doc or per-request) carrying
  `{ actorId, generation, claimedAt, heartbeatAt }`. The owning
  executor refreshes `heartbeatAt` on the same cadence as the
  capability beacon. Any executor that sees a claim whose `heartbeatAt`
  is older than `CLAIM_TIMEOUT` (reuse the `1.5 × interval` rule)
  treats it as abandoned and may re-claim — **no `--force` needed for
  the common "provider went offline" case.** This directly answers the
  user's "long delays if an executor goes offline" worry.
- **`--force` is the escape hatch for an *alive-but-stuck* executor.**
  A new executor invoked with `--force` writes a claim with a higher
  `generation`. Existing executors **watch their own claim** and, on
  seeing a live claim with a higher generation for the same doc,
  **voluntarily stand back** (abort/skip the run, stop heartbeating
  that claim). This is a *cooperative* yield — safe because every
  executor is our own trusted binary, not a security boundary. A
  malicious/stale peer ignoring the yield is out of scope (the trust
  model is D4: whoever provides execution is trusted by the room).
- **Feasibility:** yes. It is the same shape as the existing
  process-wide `IN_FLIGHT` in-flight guard (`re_execute.rs:49`),
  lifted from one process into a CRDT claim every executor observes,
  plus a generation counter for force and a heartbeat for liveness.
- **Open detail:** claim granularity — per-document (simpler; one
  executor "owns" a doc at a time) vs per-request (finer; lets two
  executors service two different docs concurrently). Lean
  **per-request claim, keyed by (path, request-generation)** so
  independent docs never block each other, with the doc-level beacon
  separate from the per-request claim.

### D6 — User-facing "clear execution results" affordance — DECIDED: per-doc first

**Decision (2026-06-30): ship per-document clear first** (sub-decision
1); project-wide "clear all" deferred. Sub-decisions 2 (confirmation
UX) and 3 (in-flight race) **settle when the executor lands (Phase 4)**
— until then there is no executor to race with, and a hand-injected
capture can simply be cleared. Recommended landing positions when we
get there: confirmation prompt = **yes** (clearing affects all
players); in-flight race = **write-if-not-cleared** on the executor
side (the principled fix), falling back to "accept + document" if it
proves fiddly.

A user must be able to **remove** the executed output from a
document's preview *without replacing it* — returning the document to
its pre-`captures` effective state. This is semantically distinct from
the two existing operations: re-execute *replaces* a capture, and
`staleness` *keeps* the capture but flags it outdated. "Clear" removes
it entirely; the editor then falls back to source-only rendering (the
splice has nothing to apply).

**Data-level mechanism (small, already half-built).** Clearing is a
single automerge **map-key delete** on the IndexDocument's `captures`
map — *not* a document deletion. This sidesteps samod's missing
doc-delete API entirely: we never delete the binary doc, only the
`CaptureRef` reference pointing at it.

- Rust: `IndexDocument::remove_capture(path)` already exists
  (`crates/quarto-hub/src/index.rs:307`, `tx.delete(captures_obj,
  path)`), used only by tests today.
- TS: the sync client currently has **no** capture-mutation API at all
  (it only *reads* via `onCapturesChange`). The one new piece is a
  typed `SyncClient` method, e.g. `clearCapture(path)` /
  `clearAllCaptures()`, that mutates the already-held internal index
  handle: `state.indexHandle.change(d => { delete d.captures?.[path] })`.

**Key property: clearing needs no executor and no server round-trip.**
Unlike re-execute (which requires a native engine), clearing is a pure
CRDT mutation any peer can perform directly — it works even when no
volunteer `q2` is connected. This makes the affordance a low-risk,
executor-independent deliverable that can ship **before** the executor
work (it pairs naturally with Phase 1's capture-consumption port).

**Sub-decisions to settle:**

1. **Granularity.** Per-document "clear results" and a project-wide
   "clear all results"? (Recommend both — per-doc in the preview
   toolbar, clear-all in a project menu.)
2. **Collaborative semantics.** The `captures` sidecar is shared, so
   one user clearing removes executed output **for every player** and
   for every open tab. Likely desired ("clean up the document"), but
   warrants a confirmation prompt naming that it affects collaborators.
3. **In-flight interplay.** If a capture is `state: running` when a
   user clears it, the executor may finish afterward and re-create the
   entry — silently re-adding output the user just removed. Options:
   (a) accept + document the race for v1; (b) clearing also broadcasts
   a cancel on the request channel (D2); (c) the executor re-checks
   the sidecar still wants results before writing (write-if-not-cleared).
   Recommend (c) as the principled fix, (a) acceptable for a first cut.
4. **History caveat (be honest in the UX copy).** Automerge history is
   append-only: the old `captureDocId` remains in the index doc's
   history and the binary-doc bytes remain in storage until D3's
   server GC runs. "Clear" restores the *effective/visible* state, not
   the byte-level history. The user sees a clean document; true
   reclamation is the separate (b) concern in D3.

## Open questions

D1–D6 and Q6–Q8 are **resolved** (see "Decisions locked" above). What
remains genuinely open and needs settling *during* the relevant phase:

1. **Auth bridge (D1=C) [Phase 3]**: token hand-off mechanism from
   Node→Rust — env var (simple, but token in process env), inherited
   fd / pipe, or a short-lived local socket. Plus how refresh
   propagates (Node refreshes; Rust must pick up the new token before
   the old one expires on reconnect). Lean fd/pipe for the secret.
2. **Beacon/claim channel [Phase 2]**: carry the beacon + claim on the
   index `DocHandle` (needs a new exposed broadcast/subscribe method on
   `SyncClient`, since only per-file handles are exposed today) vs a
   convention on a well-known per-file handle. Lean index handle.
3. **Claim granularity [Phase 2/4]**: per-document vs per-request
   (lean per-request keyed by `(path, generation)`; see D5).
4. **Naming (Q7) [Phase 0/3]**: working name `q2 provide-hub`;
   alternatives `q2 provide`, `q2 provide-execution`, `q2 hub-provide`.
   Avoid `connect` (Posit Connect collision). Final pick before the
   subcommand is user-visible.
5. **Clear in-flight race (D6 sub-3) [Phase 4]**: confirm
   write-if-not-cleared vs accept-and-document once the executor exists.

## Phased plan (TDD)

> No implementation starts until the user gives the explicit go-ahead.
> Phases below are a skeleton; each phase ends green and is verified
> end-to-end per CLAUDE.md. **Scope (Q6): quarto-hub.com only for v1;
> whole-project; mechanism engine-agnostic** (verify E2E with whichever
> engine the test host has — knitr per prior precedent).

- **Phase 0 — Decisions.** ✅ D1–D6 + Q6–Q8 resolved 2026-06-30 (see
  "Decisions locked"). Remaining to lock *in-phase*: the items in
  "Open questions" (auth-bridge mechanism, beacon/claim channel + wire
  format, final subcommand name). Working name: **`q2 provide-hub`**.
- **Phase 1 — Editor consumes captures + clear affordance (hub-client).**
  Port capture-consumption from q2-preview-spa into hub-client; verify a
  *hand-injected* capture doc + sidecar renders executed output in a
  real browser session against a local hub. (No executor yet — proves
  the consumption half end-to-end.) **Also lands D6's per-doc "clear
  results":** add the `SyncClient.clearCapture` mutation +
  toolbar affordance; verify clearing a hand-injected capture
  returns the preview to source-only. Clearing is executor-independent,
  so it is fully shippable in this phase. (Clear-all deferred.)
### Phase 1 — detailed checklist (TDD)

Findings that shaped this (verified 2026-06-30):
- The inner WASM helpers `render_single_doc_to_response` /
  `render_project_active_page_to_response` **already accept both
  `captures` and `attribution_json`** and attach both on the same
  `RenderToPreviewAstRenderer` (`lib.rs:1572-1577`). The capture-aware
  entry `render_page_for_preview` hardcodes `attribution=None`; the
  attribution entry `render_page_in_project_with_attribution` hardcodes
  `captures=Vec::new()`. hub-client's main path uses the *latter*, so
  it cannot consume captures today.
- preview-runtime's `setSyncHandlers` **already supports
  `onCapturesChange`** and `getBinaryDocById` is already exported
  (`ts-packages/preview-runtime/src/automergeSync.ts:42,68,116,200`).
  hub-client's `App.tsx` simply never registers the handler.
- `IndexDocument::remove_capture` exists in Rust but there is **no TS
  capture-mutation API** on `SyncClient` yet.

- **1A — WASM: capture-aware attribution entry (Rust).** ✅ done.
  - [x] Added `capture_gz_json: Option<Vec<u8>>` to
        `render_page_in_project_with_attribution`; parses via
        `parse_capture_from`; threads `captures` into both inner helpers
        (replaced the two `Vec::new()` literals). Fixed the internal
        `render_page_in_project` caller (4th `None`).
  - [x] RED→GREEN via WASM vitest
        `hub-client/src/services/captureSplice.wasm.test.ts`: single-file
        q2-preview doc with one `{r}` cell + a hand-built capture whose
        post-engine markdown carries a capture-only marker; asserts the
        marker appears in the rendered AST. RED confirmed on current
        code (3-arg entry ignores the 4th arg → no marker); GREEN after.
  - [x] Coexistence test: capture splices even when an attribution
        payload (`{}`) is also supplied (both attach on the same renderer).
- **1B — TS wrapper + binding type.** ✅ done.
  - [x] Extended the `render_page_in_project_with_attribution` binding
        type and the `renderPageInProjectWithAttribution` wrapper in
        `ts-packages/preview-runtime/src/wasmRenderer.ts` with
        `captureGzJson?: Uint8Array`.
- **1C — hub-client capture state (`App.tsx`).** ✅ done.
  - [x] Added `captures: Record<string, CaptureRef>` state; registered
        `onCapturesChange` in `setSyncHandlers`; passes `captures` to
        `<Editor>`. (Reset relies on `onCapturesChange` firing on
        connect, matching the existing `identities` pattern.)
- **1D — hub-client ReactPreview consumes captures.** ✅ done.
  - [x] Threaded `captures` App→Editor→PreviewRouter→ReactPreview
        (mirroring `identities`). ReactPreview fetches the active doc's
        capture bytes via `getBinaryDocById`, keyed on its `captureDocId`
        (not content), and passes them to both
        `renderPageInProjectWithAttribution` (4th arg) and
        `renderPageForPreview` (slides). `captureBytes` added to the
        render-trigger deps so a freshly-arrived/cleared capture
        re-renders.
  - [x] Integration test `ReactPreview.capture.integration.test.tsx`:
        a capture in props is fetched by id and its exact bytes reach
        the render call as the 4th arg; no-capture ⇒ no fetch, 4th arg
        undefined.
- **1E — D6 clearCapture (sync client).** ✅ done.
  - [x] Added `SyncClient.clearCapture(path)` →
        `indexHandle.change(d => { delete d.captures[path] })`; exported
        through preview-runtime (`automergeSync.ts`).
  - [x] RED→GREEN unit tests in `client.test.ts`: `clearCapture`
        removes the entry leaving siblings intact; no-op (no throw, no
        sibling change) when the path has no capture.
- **1F — hub-client clear affordance (UI).** ✅ done.
  - [x] `ClearCaptureControl` (presentational, injected `onClear`) shown
        in the preview pane only when `captures[activeFile]` exists; a
        two-step inline confirmation naming the collaborator-wide effect
        (chosen over `window.confirm` for stylability + testability).
        Wired into `Editor.tsx` → `clearCapture(path)`; minimal CSS in
        `Editor.css`.
  - [x] RED→GREEN component test
        `ClearCaptureControl.integration.test.tsx` (5 cases: hidden
        without capture / without path; shown with capture; confirm
        calls `onClear(path)`; cancel does not).
- **1G — E2E (Playwright). ⏸ Deferred to Phase 4 (rationale below).**
  - A faithful browser E2E needs a capture to exist in the live
    automerge session. In the target architecture **only the Rust
    executor writes captures** (`set_capture`); the browser solely
    *reads* and *clears* them — there is deliberately no TS
    `setCapture`. Injecting a capture in a Phase-1 Playwright test would
    therefore require throwaway test-only scaffolding (a
    `_seedCaptureForTesting` that fabricates a binary doc + sidecar),
    which `claude-notes/instructions/coding.md` discourages. **Phase 4's
    real executor makes this exact E2E faithful with zero scaffolding**
    (executor writes a real capture → hub-client shows it → clear), so
    the browser E2E is folded into Phase 4.
  - [ ] (Phase 4) Browser E2E: executor writes a capture → hub-client
        iframe shows `.cell-output` → "Clear results" → source-only.
  - Phase-1 verification standing in for it (see "End-to-end evidence
    (Phase 1)" below): the WASM test drives hub-client's **actual**
    render entry with a **real** capture and asserts spliced output;
    the React integration test covers the browser wiring
    (props→fetch→render-call); the component test covers clear.
- **1H — verify.** ✅ TS suites green (hub-client unit 662 / integration
  83 / wasm 124; sync-client 107; preview-runtime 74). Rust workspace
  untouched (only the out-of-workspace `wasm-quarto-hub-client` crate +
  TS changed; `npm run build:wasm` + `npm run typecheck` green). Full
  `cargo xtask verify` to run before the push request.

### End-to-end evidence (Phase 1, 2026-06-30)

Per CLAUDE.md's end-to-end rule. The strongest currently-faithful check
drives hub-client's **actual** render entry
(`render_page_in_project_with_attribution`) through the **real** WASM
pipeline with a **real** gzipped capture, and inspects the output AST.

Invocation:
```
cd hub-client && npx vitest run --config vitest.wasm.config.ts \
  src/services/captureSplice.wasm.test.ts
```
Observed (spliced AST region around the capture-only marker
`SPLICEDOUTPUT42`, console-dumped during a one-off run):
```
…["code-copy-outer-scaffold"],…["code-with-copy"],…"SPLICEDOUTPUT42"],"t":"CodeBlock"…
```
The marker exists **only** in the capture's post-engine markdown, never
in the source doc — so its presence in the rendered AST proves the
capture's engine output was spliced in through the entry hub-client
uses. The no-capture baseline asserts the marker is absent, and a third
case asserts capture+attribution coexist. Browser wiring
(props→`getBinaryDocById`→render-call 4th arg) is covered by
`ReactPreview.capture.integration.test.tsx`; the clear affordance by
`ClearCaptureControl.integration.test.tsx`. The full in-browser E2E is
deferred to Phase 4 (see 1G).

- **Phase 2 — Request + capability channel.** Implement the D2 channel
  (ephemeral capability beacon + execute request).

### Phase 2 — decisions locked (2026-06-30) + checklist (TDD)

Resolved with the user:
- **Q-A — scope:** channel + `SyncClient` API + capability detection +
  stub-responder tests. The user-facing **Run** affordance is deferred
  to **Phase 4** (no executor exists yet, so a Run button would be
  dormant). hub-client *does* consume beacons into a live-executor state
  in this phase.
- **Q-B — claims (D5 heartbeat/`--force`/generation):** deferred to
  **Phase 4** (claims only matter once a real executor can collide).
- **Q-C — carrier:** the **index `DocHandle`** (project-scoped),
  exposed via `SyncClient.getIndexHandle()` (mirrors the existing
  `getFileHandle()` ephemeral surface). Per-file handles would fragment
  the channel.
- **Q-D — wire format** (cross-language contract; Rust executor mirrors
  it in Phase 4): `kind`-discriminated, `exec/`-namespaced JSON on the
  index handle's ephemeral channel:
  - beacon → `{ kind: 'exec/beacon', actorId, engines: string[], generation }`
  - request → `{ kind: 'exec/request', path, requestId, requesterActorId }`
- **Q-E — timing:** `BEACON_INTERVAL_MS = 3000`,
  `BEACON_TIMEOUT_MS = 4500` (the locked `TIMEOUT = 1.5 × INTERVAL`).

- **2A — `SyncClient.getIndexHandle()` + preview-runtime export.** ✅ done.
  - [x] Returns `state.indexHandle` (or null); re-exported from
        `automergeSync.ts`. RED→GREEN unit test: index handle null
        before connect, the handle after.
- **2B — execution-channel wire format + pure helpers.** ✅ done.
  - [x] `hub-client/src/services/executionChannel.ts`: message types +
        `makeBeacon`/`makeExecuteRequest`, `parseExecMessage`
        (validate/discriminate untrusted payloads), `applyBeacon`/
        `pruneExecutors` (live-executor map keyed on actorId, `1.5×`
        staleness). RED→GREEN pure unit tests (13).
- **2C — execution-channel service (stateful).** ✅ done.
  - [x] `createExecutionChannel({ getIndexHandle, onExecutorsChange,
        now, ... })`: subscribes to index ephemeral messages →
        live-executor set + prune timer; `requestExecution(path)`
        broadcasts an `exec/request`. RED→GREEN stub-responder tests (5)
        against a fake DocHandle (records broadcasts; injects messages):
        beacon appears/expires; request shape round-trips through
        `parseExecMessage`; self-beacon ignored; null when not connected.
- **2D — wire into hub-client (capability state, no Run UI).** ✅ done.
  - [x] `useExecutionChannel(isOnline, indexDocId)` hook starts/stops the
        channel with the connection/project and returns `liveExecutors`
        (integration test: beacon→executor, teardown). App holds it and
        passes `executorsOnline` to Editor, which shows a minimal
        read-only "Executor online" bar (no Run button). Minimal CSS.
- **2E — verify.** ✅ hub-client unit 680 / integration 86; sync-client
  108; preview-runtime 74; `tsc -b` + vite build green; typecheck green.
- **Phase 3 — Rust client peer + BearerDialer (D1=C).** New subcommand
  opens a samod client `Repo`, dials a remote hub with a `BearerDialer`
  fed a token from the auth bridge, `find()`s the index doc. Verify it
  joins an authenticated session and reads the file list.
- **Phase 4 — Execute-on-request.** Wire the request channel to
  `record_capture_cached`; write the capture doc + sidecar with the
  existing Rust functions; add the capability beacon. E2E: a browser
  "Run" makes the connected `q2` execute and the executed output
  appears for *all* players.
- **Phase 5 — Retention (D3) + authorization (D4).** Content-addressed
  capture dedup; per-project opt-in + (if chosen) requester gating +
  consent UX. File server-GC follow-up.
- **Phase 6 — Hardening.** Reconnect/refresh, multi-executor claim,
  diagnostics surface, `cargo xtask verify` full green, push approval.

## Key source references

- Capture transport (result side, fully reusable):
  - `crates/quarto-preview/src/capture_driver.rs:57,326` (record + write doc)
  - `crates/quarto-preview/src/re_execute.rs:102,291,337` (trigger + perform)
  - `crates/quarto-core/src/engine/preview_record.rs:130` (`record_capture`)
  - `crates/quarto-preview/src/cache.rs:151` (`record_capture_cached`)
  - `crates/quarto-trace/src/lib.rs:186` (`EngineCapture`)
  - `crates/quarto-hub/src/index.rs:68,272` (`CaptureRef`, `set_capture`)
  - `crates/quarto-hub/src/resource.rs:18,116` (binary doc schema)
  - `crates/quarto-core/src/engine/capture_splice.rs` (replay/splice)
  - `crates/wasm-quarto-hub-client/src/lib.rs:1201-1309` (WASM consume)
- Editor consumption (port target):
  - `q2-preview-spa/src/PreviewApp.tsx:758,1030-1059`
  - `ts-packages/quarto-sync-client/src/client.ts:352-377,1102,174`
  - `q2-preview-spa/src/components/StaleCaptureOverlay.tsx:59-77`
- Clear results (D6):
  - `crates/quarto-hub/src/index.rs:307` (`remove_capture`, map-key delete)
  - `ts-packages/quarto-sync-client/src/client.ts:174` (internal `indexHandle` to expose for mutation)
- Ephemeral channel (request/capability model):
  - `hub-client/src/services/presenceService.ts:333-439`
  - `ts-packages/quarto-sync-client/src/client.ts:1335-1346` (`getFileHandle`)
- samod client peer + auth:
  - `crates/quarto-hub/src/peer.rs:18-50` (`dial_websocket`)
  - `crates/quarto-hub/src/server.rs:385-395,781-806,1051-1057` (Bearer, actor)
  - `ts-packages/quarto-hub-mcp/src/auth/*` (OAuth/keyring/refresh)
  - `ts-packages/quarto-sync-client/src/NodeWebSocketClientAdapter.ts` (Bearer WS)
  - `crates/quarto-mcp-launcher/src/lib.rs` (`q2 mcp` launcher pattern)
- Prior plans:
  - `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md` (auth + launcher; native-Rust findings)
  - `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md` (capture splice)
  - `claude-notes/plans/2026-05-27-multi-engine-execution.md` (Vec<EngineCapture>)
  - `claude-notes/plans/2026-01-06-execution-engine-infrastructure.md` (engine trait)
