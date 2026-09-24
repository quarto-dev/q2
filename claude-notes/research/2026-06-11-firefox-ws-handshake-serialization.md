# Firefox `q2 preview` peer-connection timeout: root cause

**Date:** 2026-06-11
**Strand:** bd-jit6pdwq (q2 preview: Firefox flaky 'Document automerge:<id> is unavailable' on cold start)
**Symptom:** sporadic, Firefox-only `Peer connection failed, continuing in
offline mode: Error: Timeout waiting for peer connection`, followed by a
permanent `Document … is unavailable` boot error in the preview SPA.

## TL;DR

Firefox serializes WebSocket *opening handshakes* per host, browser-wide:
only one WebSocket to `127.0.0.1` may be in the CONNECTING state at a
time, across **all tabs**. If any tab holds a WebSocket handshake that
hangs (TCP accepted, HTTP 101 never sent — e.g. a wedged/suspended
`q2 preview` server, or any unresponsive localhost endpoint), every other
localhost WebSocket handshake in the browser queues behind it for up to
Firefox's `network.websocket.timeout.open` (20 s default) per attempt.
The preview SPA budgets only 5 s for the samod `peer` event, so it falls
into "offline mode", `repo.find()` then marks the (ephemeral, never
cached) index doc unavailable, and the boot fails **permanently** — even
though the WebSocket typically connects fine a few seconds later.

Chromium does not serialize handshakes this way and is immune.

## Evidence (all scripts in `claude-notes/tmp/`)

1. **Server exonerated** (`peer-handshake-probe.mjs`): 10/10 Node
   connections to the live failing server (port 59776) reached `peer` in
   0.5–8 ms. The Rust hub answers WS upgrades instantly.

2. **Clean Firefox exonerated** (`firefox-repro.mjs`): 15/15 cold loads
   of the preview SPA in a fresh Playwright Firefox connected in \~260 ms.
   The bug does not reproduce in an idle browser → environmental trigger.

3. **Reproduction** (`firefox-serialization-test.mjs` +
   `blackhole-server.mjs`): one tab holding a WebSocket to a localhost
   port that accepts TCP but never answers the upgrade ("blackhole",
   simulating a wedged/suspended server). A second tab then loads the
   preview SPA:

   - **Firefox**: `Peer connection failed, continuing in offline mode`
     at t=5.29 s — the exact production failure. Retries at +5 s and
     +10 s also starved.
   - **Chromium** (control): `Peer connected - online mode` at t=59 ms
     under identical conditions.
   - **Firefox + stale tab on a *closed* port** (control): peer at
     430 ms. Fast TCP-reset failures do *not* starve the queue; a
     genuinely hung handshake is required.

   The per-host serialization lives in Firefox's `nsWSAdmissionManager`
   (`netwerk/protocol/websocket/WebSocketChannel.cpp`).

## Source-level confirmation that the key excludes the port

Verified against mozilla-central master (2026-06-11),
`netwerk/protocol/websocket/WebSocketChannel.cpp`:

- `nsWSAdmissionManager::ConditionallyConnect` defers a handshake when
  `IndexOf(ws->mAddress, ws->mOriginSuffix) >= 0` — the queue lookup
  compares **resolved IP address + origin attributes only**; `nsOpenConn`
  carries no port or path. Code comment: "If there is already another WS
  channel connecting to this IP address, defer BeginOpen and mark as
  waiting in queue."
- `mAddress` is the **DNS-resolved IP** (`GetNextAddrAsString`), so
  `localhost` and `127.0.0.1` share one key.
- The **port-aware** structure is the separate `FailDelayManager`
  (reconnect backoff), keyed by `(address, path, port)` — which is why
  the closed-port control didn't interfere cross-port but the hung
  handshake did.
- This implements RFC 6455 §4.1 literally: "If multiple connections to
  the same IP address are attempted simultaneously, the client MUST
  serialize them so that there is no more than one connection at a
  time." The spec's serialization clause is per-IP, portless. Chromium
  interprets it loosely (our control: 59 ms with a pending handshake).
- Caveat: `mOriginSuffix` carries container-tab / private-browsing
  identity, so handshakes in a Firefox container or private window do
  **not** share the slot with normal tabs. (Diagnostic implication: the
  bug "disappearing" in a private window is consistent with this root
  cause, not evidence against it.)

## Why this user, why sporadic

The workflow accumulates localhost WebSocket clients: every `q2 preview`
session opens a tab whose automerge-repo `WebSocketClientAdapter` retries
`ws://127.0.0.1:<port>/ws` every 5 s, **forever**, after the server goes
away. Q1 (`quarto preview`) live-reload tabs do the same on their ports.
Any one of these endpoints entering an accept-but-don't-respond state
(server wedged, process suspended, kernel listen-backlog of a dying
process, single-threaded server busy) turns its tab into a perpetual
slot-holder: each retry occupies the browser-wide handshake slot for up
to 20 s. While that state persists, *every* new preview tab fails its
5 s peer wait; close the offending tab (or the endpoint recovers) and
previews work again. Hence "sporadic", and "reproduces every time" on a
bad day (bd-jit6pdwq).

## Aggravating design factors found along the way

These make a transient handshake delay into a hard, unrecoverable failure:

1. **5 s peer budget == 5 s adapter retry interval.**
   `PreviewApp.tsx` passes `peerTimeoutMs: 5000`;
   `WebSocketClientAdapter`'s `retryInterval` default is also 5000 ms.
   Any first-attempt failure loses the race by construction — recovery
   at t≥5 s can never beat the deadline at t=5 s.

2. **No recovery after the timeout.** After `waitForPeer` rejects,
   `connect()` proceeds to `findDoc()`; `networkSubsystem.whenReady()`
   force-resolves 1 s after adapter creation even when unconnected, so
   `handle.request()` runs against zero peers → handle resolves
   UNAVAILABLE → `connect()` throws → SPA sets `boot: 'error'`
   permanently. The adapter often connects seconds later (we observed
   this), but nothing retries the boot.

3. **IndexedDB gates the WebSocket `join`.** automerge-repo's
   `NetworkSubsystem` defers `adapter.connect()` (which creates the
   WebSocket and sends `join`) until `storageSubsystem.id()` — an
   IndexedDB open+read — resolves (verified empirically: a 6 s storage
   delay produces the identical timeout; `slow-storage-probe.mjs`).
   The preview SPA gets IndexedDB unconditionally from
   `buildStorageAdapter()`, yet its docs are ephemeral — the cache can
   never hit, and each preview port pollutes the Firefox profile with a
   new origin's database. Any IDB slowness (Firefox is notorious)
   silently eats the peer budget. Not the trigger observed here, but a
   second independent path to the same failure.

4. **Stale preview tabs retry forever** (no backoff cap, no give-up),
   which is what keeps a hung endpoint's slot occupied perpetually and
   adds localhost WS churn generally.

## Fix directions (not implemented in this session)

- **Make boot resilient instead of deadline-bound** (primary): for the
  preview SPA, offline mode is meaningless — don't hard-fail on the 5 s
  peer timeout. Wait for the `peer` event (with status UI), or retry
  `findDoc` when a peer connects after a failed boot. automerge-repo
  handles can recover from UNAVAILABLE via `progress`/re-request.
- **De-align the deadlines**: if a finite budget is kept, make it ≫ the
  adapter retry interval (e.g. 15–30 s), or pass a smaller
  `retryInterval` to the adapter.
- **Memory storage for the preview SPA**: thread a storage option
  through `createSyncClient()` so preview uses `MemoryStorageAdapter`;
  removes the IDB gate on `join` and stops per-port profile pollution.
- **Cap stale-tab retries**: exponential backoff and/or a "server gone —
  reload when ready" terminal state in the SPA, so dead preview tabs
  stop hammering localhost. (Both kindness to Firefox's handshake queue
  and battery.)
- **User-side mitigation meanwhile**: close old preview tabs; if a
  preview suddenly shows the offline error, suspect some localhost tab
  whose server is hung (check DevTools Network → WS for a pending
  handshake) rather than the new preview itself.

## Loose end (resolved)

The user's 59776 preview server went down mid-investigation; the user
confirmed they closed it themselves. No spontaneous server death
observed; a sibling instance survived identical probing.
