# Connection indicator: no Offline flash on open (bd-53501yf7)

## Overview

When opening a document in hub-client, the header connection indicator
**always showed "Offline" first, then flipped to "Online"** a moment later.

### Root cause

- `App.tsx` holds a boolean `isOnline`, initialized to `false`.
- The indicator lives in `MinimalHeader`, which is inside `Editor` — and
  `Editor` only mounts **after** `project` is set, i.e. **after**
  `connect()` has already resolved. So the indicator's *first paint* is
  whatever `connect()` decided; it never renders during the connecting
  window, and the initial `false` is never seen.
- Production passed `peerTimeoutMs = undefined` → coerced to **1 ms**
  (`client.ts:814`). The WebSocket almost never completes its handshake in
  1 ms, so `connect()` resolved offline-first and fired
  `onConnectionChange(false)` (`client.ts:946`). The header mounted showing
  **Offline**. The later `peer` network event flipped it to **Online**.

So the *connecting* window was displayed as *offline*, and — because of the
1 ms budget — the initial content was also the (possibly stale) local cache
rather than the live document.

### Fix: widen the initial peer wait (only)

Widen the production peer wait from 1 ms to **400 ms**
(`PRODUCTION_PEER_TIMEOUT_MS` in `App.tsx`). Because `waitForPeer` resolves
the *instant* the peer connects:

- **Common (fast) case:** `connect()` resolves as **Online**, so the header
  mounts already Online, showing the **live** document — adding only the
  real connect latency. No flash.
- **Slow/offline case:** `connect()` resolves offline; the header mounts
  Offline and flips to Online when the peer lands. This is the rare tail —
  and still strictly better than today's *always*-Offline-first, so we
  accept it (no extra machinery).

The E2E path is unchanged at 15000 ms (it starts from empty storage and must
sync every doc; opening before the socket connects makes `loadFileDocuments`
race it and the render-target doc can lose under CI contention).

### Approaches considered and dropped

We first built a three-state `connecting`/`online`/`offline` indicator that
stayed **hidden** during the connecting window and used a grace timer to
concede "Offline" only after a first connect that never landed a peer. That
deterministically hid the slow/offline flash tail but added a pure module, a
grace-timer constant, a `hasEverConnected` ref, and a `beginConnect()` reset
at every connect site. Per the user (2026-07-27), we dropped it: the 400 ms
widen alone fixes the common case, and showing Offline on a genuinely slow
connect is acceptable. Simpler wins.

## Work Items

- [x] Widen production `peerTimeoutMs` 1 ms → 400 ms
      (`PRODUCTION_PEER_TIMEOUT_MS`); update the comment; E2E unchanged.
- [x] `MinimalHeader.test.tsx` (jsdom) — indicator shows Online when
      `isOnline`, Offline otherwise (previously untested).
- [x] `npm run test:ci` (hub-client) green.
- [x] `tsc -b` clean and `npm run build:all` succeeds (WASM + vite).
- [ ] End-to-end: `npm run local-prod` (or a real hub) — open a doc,
      confirm the indicator opens Online (fast connect) rather than
      flashing Offline→Online. **(not yet run — see status note)**
- [x] Update `hub-client/changelog.md` (two-commit workflow).
