---
title: 'Force-reconnect the sync WebSocket on wake and network change'
date: 2026-09-28
---

This plan deliberately avoids heartbeats: it is client-only, has no hub or
protocol change, and covers the common triggers at the cost of the
rare ones (see "What this does not cover").

## Context / problem

hub-client's live updates (files sidebar, editor content, presence) ride a
single automerge sync WebSocket (`/ws`). When that connection drops
*silently* — a half-open TCP connection with no close frame — neither side
learns of it:

- Upstream `BrowserWebSocketClientAdapter` (automerge-repo-network-websocket
  2.6.0-alpha.5) reconnects only on a `close` event, which a half-open
  connection never delivers.
- The automerge sync protocol only sends bytes when a document changes, so an
  idle-but-dead connection produces no traffic and no error.
- Result: "stale until I refresh." The Online badge (`SyncStatusBadge.tsx`)
  also stays on "Online", because `peer-disconnected` fires only from
  `onClose` (and from `disconnect()`, which a silent drop never reaches).
- Worse, the badge actively lies on network change: `client.ts:415-428`
  already listens for `online` and fires `onConnectionChange(true)` but
  never reconnects, so after a network change the badge shows "Online"
  over a possibly half-open dead socket. This plan's `online` trigger
  converts that lie into an honest Offline → Online cycle.

The most common causes of a silent drop are **laptop sleep/wake** and a
**network change** (Wi-Fi switch, VPN up/down, offline → online). Both are
observable from the browser without any server cooperation.

The preview SPA already disconnects its sync connection on `pagehide`
(`q2-preview-spa/src/PreviewApp.tsx:745`, bd-jit6pdwq Phase 3 — a socket stuck in CONNECTING
otherwise occupies Firefox's browser-wide per-IP handshake queue). That
does not overlap this plan's triggers: `pagehide` is the page going away,
not a hidden-but-live tab, and `disconnect()` tears the triggers down with
it. Sleep-while-visible and network changes hit the SPA the same way they
hit hub-client, so both benefit from the force-reconnect.

## Approach

When the browser signals that the connection has probably died,
force-reconnect: close the socket and run upstream `onClose` directly. The
existing path does the rest: `onClose` emits `peer-disconnected` (the badge
flips to Offline), schedules `this.connect(...)` after `retryInterval`, and
the reconnect re-syncs every open document (the badge flips back). That
recovery path is already exercised by every hub restart.

`socket.close()` alone is not enough. It only starts the closing handshake
(readyState → CLOSING), and the `close` event waits for the server's close
frame or the browser's closing-handshake timeout (Firefox
`network.websocket.timeout.close` defaults to 20 s; Chromium's is believed
to be up to ~60 s, unverified). On a truly black-holed connection that
timeout is the only way out, so waiting for `close` would delay recovery by
up to a minute in exactly the case this plan targets.

A false positive costs one reconnect + re-sync of already-in-sync documents
(a few small messages per open doc; session-cookie auth is a local check,
`context.rs:890`). That is strictly cheaper than today's recovery — a full
page refresh — so triggers are tuned to favour recall over precision.

## Work items

Test specifications first (TDD); implementation follows.

### Tests (write first, watch fail)

- [x] `StoppableWebSocketClientAdapter.test.ts` (fake timers + stubbed
  globals; see "Testability"):
  - wake gap > `WAKE_GAP_THRESHOLD` on an OPEN socket ⇒ force-reconnect
  - normal ticks, and a throttled-tab gap of ~60 s ⇒ no reconnect
  - `online` event, and `navigator.connection` `change`, on an OPEN
    socket ⇒ force-reconnect
  - socket CONNECTING / CLOSING / CLOSED at trigger time ⇒ no-op
  - two triggers back to back (e.g. `online` + wake gap) ⇒ exactly one
    reconnect
  - `close()` never delivers a `close` event (black-holed socket) ⇒ the
    reconnect is still scheduled after `retryInterval`, and a late `close`
    from the old socket does not schedule a second one
  - `disconnect()` ⇒ listeners and interval removed; later triggers are no-ops
  - no `window` global (Node) ⇒ no listeners, no interval, no throw
- [x] Online-indicator chain: trigger ⇒ `peer-disconnected`;
  sync-client level: `onConnectionChange(false)` then `(true)` on reconnect
  (adapter-level assertions in `StoppableWebSocketClientAdapter.test.ts`;
  client-level end-to-end against a real in-process hub in
  `client.force-reconnect.test.ts`)

### Implementation

- [x] Triggers in `StoppableWebSocketClientAdapter` (below)
- [x] Diagnostics: `recordConnectionEvent('ws-force-reconnect', reason)`
  and a `syncLog` line, so `ConnectionStatusDialog` / `quartoDebug` show why
  a reconnect happened
- [x] Fix the stale doc comment on `buildWsAdapter` (`client.ts:145`) in
  passing: it still claims "the upstream browser adapter is used unchanged",
  but non-auth connections have used the Stoppable subclass since
  bd-jit6pdwq

### Verification

- [x] End-to-end in a real browser (per CLAUDE.md, in-process tests alone
  do not count): a throwaway Playwright spec (not committed) drove the real
  app against the real e2e hub — badge "Synced" → synthetic
  `window.dispatchEvent(new Event('online'))` → badge "Offline" → badge
  "Synced" after the 5 s retryInterval, no page refresh (passed in 8.3 s).
  The physical variant remains for the user: sleep the laptop for > 2 min
  with a second client editing, wake, and confirm the sidebar/editor catch
  up without a refresh; toggle Wi-Fi and confirm the same
  (`npm run local-prod`).
- [x] Kept the hub-client e2e suite green — 77 passed, 1 flaky (unrelated
  `first-run.spec.ts` boot-URL test, passed on retry)
- [x] `cargo xtask verify` (full — hub-client and ts-packages affected);
  needed `brew install typst` first (CI's macOS leg installs it; this
  machine lacked it and the typst Rust test failed before any TS leg ran)
- [x] hub-client two-commit changelog workflow (user-facing: live updates
  now recover after sleep / network change) — `9fff3e98` + changelog commit

## Client change — `StoppableWebSocketClientAdapter`

`ts-packages/quarto-sync-client/src/StoppableWebSocketClientAdapter.ts`
already overrides `onError` / `connect` / `disconnect`. It is the adapter
`buildWsAdapter` (`client.ts:151`) uses for every non-auth connection, which
covers hub-client and the preview SPA. It is also used from Node in the
vitest suite, so every browser API must be feature-detected.

### Triggers

1. **Wake detection (timer drift).** A `setInterval` every
   `WAKE_CHECK_INTERVAL` compares `Date.now()` with the previous tick. A gap
   greater than `WAKE_GAP_THRESHOLD` means the process was suspended (sleep,
   or a frozen background tab), so force-reconnect. Uses wall-clock
   `Date.now()` deliberately: `performance.now()` pauses during sleep on some
   platforms, which would hide the gap.
2. **Network back online.** `window` `online` event ⇒ force-reconnect. Also
   `navigator.connection` `change` where it exists (Chromium only), which
   catches some network switches that never toggle `navigator.onLine`.
   `change` also fires on signal-quality fluctuations
   (effectiveType/downlink shifts on a moving phone), not just switches:
   a spurious but cheap reconnect, accepted — the API offers no reliable
   "same network" signal to filter on.

No `visibilitychange` trigger: a tab that was merely hidden has a live socket,
and a frozen tab (Safari/iOS, Chrome tab freezing) already shows up as a wake
gap because its timers did not run.

### Force-reconnect

`#forceReconnect(reason)`, synchronous:

```ts
#forceReconnect(reason: string): void {
  const socket = this.socket;
  if (!socket || socket.readyState !== WebSocket.OPEN) return;
  syncLog(`WebSocket force-reconnect: ${reason}`);
  recordConnectionEvent('ws-force-reconnect', reason);
  socket.removeEventListener('close', this.onClose); // ignore its late close
  socket.close();
  this.onClose(); // emits peer-disconnected, schedules connect() after retryInterval
}
```

- `onClose` is a public arrow property on the upstream adapter, so calling it
  directly is supported. The reconnecting `connect()` strips the old socket's
  remaining listeners before creating the new one.
- The `readyState === OPEN` guard does the coalescing: after the first
  force-reconnect the socket is CLOSING, so a second trigger in the same
  burst (e.g. `online` and a wake gap together on resume) is a no-op. A
  socket that is already CONNECTING or CLOSED means the retry path is
  already running.
- No jitter. Wake events are per-machine, so they are not correlated. A
  network blip that fires `online` in many clients at once produces the same
  herd as a hub restart, where every client reconnects after the same
  `retryInterval`, and that path already works.
- Recorded choice: the reconnect waits `retryInterval` (5 s by default)
  because it reuses upstream `onClose` wholesale — the same path exercised
  by every hub restart. A false positive (e.g. a frozen-tab wake gap on a
  healthy socket) therefore shows Offline for up to ~5 s on tab return.
  Calling `this.connect(this.peerId, this.peerMetadata)` directly would
  recover instantly, but diverges from the tested path; not worth it for a
  5 s blip.

### Lifecycle

- `connect()` override: after `super.connect(...)`, attach the triggers once
  (idempotent — `connect()` runs again on every reconnect via the parent's
  `onClose`). Skip entirely when `typeof window === 'undefined'`. Read
  `window` / `navigator.connection` here, at attach time, not at module
  load.
- `disconnect()` override: clear the interval and remove the listeners.
  `#stopped` already prevents any reconnect.
- Listener callbacks are instance arrow properties so the same reference
  can be removed.

### Constants

- `WAKE_CHECK_INTERVAL`: 10 s.
- `WAKE_GAP_THRESHOLD`: 90 s. Must stay above background-tab timer
  throttling: Chrome's intensive throttling runs chained timers in hidden tabs
  about once a minute, so a 60 s gap is normal and must not trigger. A
  90 s threshold means a sleep shorter than ~90 s is missed. Such a short
  sleep rarely kills the connection anyway, and if it does, the next
  `online` event or a later wake still catches it.

### Testability

Tests run in Node (isomorphic-ws), where `window` and `navigator.connection`
do not exist. No production seam is needed, and the constructor signature
stays `(url, retryInterval)` as upstream:

- Clock: vitest fake timers already fake `Date.now()`. Simulate a sleep with
  `vi.setSystemTime(Date.now() + 120_000)`, then
  `vi.advanceTimersByTime(WAKE_CHECK_INTERVAL)`.
- Events: `vi.stubGlobal('window', new EventTarget())` and
  `vi.stubGlobal('navigator', { connection: new EventTarget() })` before
  `connect()`, then dispatch `online` / `change`. Call `vi.unstubAllGlobals()`
  in `afterEach`.
- Node-absent path: the default, unstubbed environment.
- Socket state: stub `this.socket` with a fake whose `readyState` and
  `close()` the test controls, so a black-holed socket (no `close` event
  ever) can be modelled.

## Online indicator

No UI change. The force-reconnect runs the existing chain: `onClose` →
`peer-disconnected` (guarded by `remotePeerId`, set during the handshake) →
`createSyncClient`\'s `onConnectionChange(false)` → `setIsOnline(false)` →
badge shows Offline; the reconnect flips it back. The badge is still wrong
*during* an undetected half-open (see below) — only a heartbeat can fix that.

Interaction with the existing `online` listener (`client.ts:415-428`): it
fires `onConnectionChange(true)` immediately and optimistically, so on this
plan's `online` trigger the badge goes Online → Offline → Online across the
reconnect. That is more honest than today (Online displayed over a
possibly-dead socket), and the optimistic flip is redundant once the
reconnect path works — but leave it; touching the badge flow is out of
scope for this plan.

## What this does not cover

Accepted trade-offs of avoiding a heartbeat:

- **Silent drops on an awake machine** — NAT/VPN/middlebox idle timeouts, or
  a network change that fires neither `online` nor `connection.change`. The
  client stays stale until the user refreshes, and the badge says "Online".
- **Proxies with idle timeouts** (e.g. nginx's 60 s `proxy_read_timeout`)
  close idle sockets. That is not silent — the client gets a close event and
  reconnects — but it causes periodic reconnects. Fix at the proxy config if
  seen.
- **Other adapters.** `projectSetService.ts:327` uses the raw upstream
  `BrowserWebSocketClientAdapter`, and `NodeWebSocketClientAdapter` (MCP /
  authenticated Node clients) is separate. Neither gains these triggers.
  Switching `projectSetService` to the Stoppable subclass is a possible
  follow-up.
