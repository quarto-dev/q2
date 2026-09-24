# Fix: q2 preview Firefox peer-connection timeout (bd-jit6pdwq)

**Strand:** bd-jit6pdwq
**Research:** `claude-notes/research/2026-06-11-firefox-ws-handshake-serialization.md`
**Repro scripts:** `claude-notes/tmp/` (blackhole-server, firefox-serialization-test, …)

## Overview

Firefox serializes WebSocket opening handshakes per resolved IP address,
browser-wide (RFC 6455 §4.1, `nsWSAdmissionManager`). A hung handshake to
*any* `127.0.0.1` port (wedged/suspended local server in any tab) starves
all localhost WS handshakes for up to 20 s per attempt. The preview SPA
budgets 5 s for the samod `peer` event, then "falls back" to an offline
mode that is meaningless for preview (docs are ephemeral, never cached),
and the boot fails permanently — even though the socket typically
connects seconds later.

### Design principles

1. **HTTP for liveness, patience for WebSocket.** The preview server's
   `/health` endpoint is reachable over plain HTTP, which Firefox's WS
   admission queue does not affect. The SPA already fetches it at boot
   (`fetchIndexDocId`). So: never infer "server gone" from WS handshake
   latency; infer it from `/health`. While `/health` answers, keep
   waiting/retrying the WebSocket — however long it takes.
2. **No behavior change for hub-client.** All new capabilities are
   opt-in options on `createSyncClient`/`connect` with defaults that
   preserve today's semantics (IndexedDB storage, 1 ms peer probe,
   offline fallback). Hub-client's offline-first flow is intentional
   and stays.
3. **No regression in other browsers.** Every change is pure
   robustness: an indefinite peer wait resolves in \~60 ms when the
   handshake is fast (Chrome today, clean Firefox today); memory
   storage is strictly faster than IndexedDB; health-gated reconnects
   only kick in when the connection actually drops.
4. **Reduce the SPA's own contribution to the problem.** Stale preview
   tabs currently retry `ws://127.0.0.1:<dead-port>/ws` every 5 s
   forever, and automerge-repo's retry abandons CONNECTING sockets
   without closing them — each abandoned socket occupies Firefox's
   per-IP queue until the 20 s open-timeout. Health-gating reconnects
   removes that churn at the source.

### Why not "just raise the timeout"

The adapter's silent retry interval (5 s) equals the SPA's peer budget
(5 s), so any first-attempt failure is fatal by construction; raising
the budget to 15 s only moves the cliff. The hung-slot condition can
persist for minutes (a perpetually-retrying stale tab holds the slot
\~100 % of the time), so any finite WS deadline picks a wrong answer.
The right deadline lives on `/health`, not on the socket.

## Phases & work items

### Phase 1 — quarto-sync-client: options + tests (TDD)

Write the tests first against the existing vitest harness
(`ts-packages/quarto-sync-client/src/client.test.ts`,
`ts-packages/sync-test-harness` for in-memory repo pairs).

- [x] Tests: `connect()` options-bag back-compat — existing positional
      call sites (hub-client `App.tsx`, preview-runtime) behave
      identically (probe default 1 ms, IndexedDB-by-default in
      browser-shaped env, offline fallback preserved).
      (`client.connect-options.test.ts`; watched 6/12 fail pre-impl.)
- [x] Tests: `peerTimeoutMs: Infinity` waits for the `peer` event
      without scheduling a timer (no `setTimeout(…, Infinity)`
      coercion bug — skip the timer when `!Number.isFinite`), resolves
      when a late peer arrives (60 s simulated), and never enters
      the offline branch. Plus a finite-budget control test.
- [x] Tests: `storage: 'memory'` uses `MemoryStorageAdapter` (no
      `indexedDB` global touched even when one exists); default
      remains IndexedDB.
- [x] Tests: `retryIntervalMs` is forwarded to
      `BrowserWebSocketClientAdapter`\'s constructor.
- [x] Tests: `findDoc` unavailable-recovery — first `repo.find()`
      rejects "unavailable", a peer then connects, retry succeeds;
      no retry with zero peers (offline fast-fail); bounded attempts;
      non-"unavailable" errors rethrown immediately.
- [x] Implement: 6th param of `connect()` widened to
      `number | ConnectOptions` (auth stays positional; TypeError on
      auth passed both ways). `CreateProjectOptions` gains `storage`,
      `retryIntervalMs`, `findDocRetry`.
- [x] Implement: `buildStorageAdapter(kind)` in `storage-adapter.ts`.
- [x] Implement: `waitForPeer` infinite-budget branch; `findDoc`
      retry-on-unavailable (peer-gated via `state.connectedPeers`,
      tracked from repo creation in both connect and
      createNewProject); reset on disconnect.
- [x] `npm run test` in quarto-sync-client (148 passed); hub-client
      `npm run build` (tsc -b project references + vite) green. Full
      `build:all` deferred to Phase 5 (WASM leg untouched by
      TS-only changes).

### Phase 2 — preview-runtime + PreviewApp: resilient boot (TDD)

Tests first in `q2-preview-spa/src/PreviewApp.integration.test.tsx`
(mock `connect` already exists there) plus a small unit suite for the
boot controller if extracted.

- [x] Tests: boot uses `storage: 'memory'` and `peerTimeoutMs:
      Infinity`, and leaves the adapter `retryIntervalMs` at its
      default (assert via the connect mock's args). Do NOT shorten
      the retry interval — see Details.
- [x] Tests: boot retry loop — `connect` rejects once (simulated
      unavailable race), `/health` mock still OK → retries with
      backoff and succeeds; UI stays in "connecting" state, not
      `boot: 'error'`. (Old "connection error ⇒ terminal overlay"
      test rewritten to this contract; watched 3 new tests fail
      pre-impl.) Plus: `bootController.test.ts` — 10 browser-free
      specs covering backoff growth/cap, health-strikes confirmation,
      transient-blip tolerance, hang watchdog, no-attempt-cap, and
      cancellation (these are the durable regression net promised in
      the e2e demotion policy).
- [x] Tests: `/health` unreachable (server killed) → terminal
      `boot: 'error'` ("preview server is not responding"); initial
      /health-dead (docId fetch) still fails fast.
- [x] Tests: slow-connect status UI — Firefox hint after threshold;
      retry attempt/last-error line; no hint/retry-line initially
      (`BootLoadingScreen.integration.test.tsx`, threshold injected
      via `hintAfterMs` prop instead of fake timers).
- [x] Tests: unmount during pending boot — no setState-after-unmount
      (clean console.error), late rejection swallowed; bootController
      cancellation covered unit-side (stops watchdog polling too).
- [x] Implement: `bootController.ts` (`bootWithRetry()`, generic,
      dependency-free): per-attempt health watchdog raced against
      connect (sleep-first so fast attempts cost zero probes),
      post-failure health confirmation (3 strikes, 500 ms spacing),
      capped exponential backoff (1 s → 10 s), `ServerGoneError`
      terminal, cooperative cancellation.
- [x] Implement: thread `ConnectOptions` through `preview-runtime`\'s
      `connect()` (+ re-export from quarto-sync-client index, which
      was missing); doc comments updated.
- [x] Implement: `BootLoadingScreen` component (initializing copy,
      retry visibility, Firefox hint after 8 s); PreviewApp's
      `onError` now ignores connection errors while `boot ===
      'loading'` (the boot controller owns them — otherwise each
      retryable failure would flash the terminal overlay).
- [x] q2-preview-spa: unit 18 + integration 62 green; preview-runtime
      74 green; quarto-sync-client 148 green; hub-client build +
      575 tests green; SPA `tsc -b` + `vite build` green.

### Phase 3 — steady-state reconnect hygiene (stale tabs)

- [x] Tests: on `onConnectionChange(false)` after an established
      session, the SPA calls `disconnect()` then reconnects under
      health arbitration; reconnecting banner while in flight;
      server-gone banner when `/health` dies (content stays up, no
      terminal overlay); automatic recovery when `/health` returns
      (4 new PreviewApp integration tests, watched failing first).
      Unit side: 4 `superviseReconnect` specs in
      `bootController.test.ts` (passthrough, gone→recover, capped
      gone-phase poll backoff 1 s→30 s, cancellation).
- [x] Tests: `pagehide` handler calls `disconnect()` (aborts any
      CONNECTING socket on tab close/navigation).
- [x] Implement: `superviseReconnect()` in bootController (reuses
      `bootWithRetry`; `ServerGoneError` becomes a *state*, not a
      verdict — HTTP-only polling while gone, so stale tabs make zero
      WS attempts); supervision loop + connection banner + pagehide
      handler in `PreviewApp.tsx`; `onError` also gated on
      `connection === 'connected'` so reconnect-phase failures don't
      flash the terminal overlay. `BootStatus` is now a union with
      `{ phase: 'server-gone' }`.
- [x] Suites: spa unit 22 + integration 66 green; `tsc -b` + vite
      build green.
- [x] Manual check (done in Phase 5 end-to-end, scripted against real
      Firefox + real binary): kill the server under an open preview
      tab → WS attempts stop (0 in a 12 s window) and the banner
      appears; restart server → tab recovers without reload. See
      Phase 5 for the two bugs this check surfaced and their fixes.

### Phase 4 — Firefox regression e2e (gated, slow, demotable)

- [x] Ported into `q2-preview-spa/e2e/firefox-ws-queue.spec.ts` +
      `e2e/helpers/blackhole.ts`, using the existing
      `previewServer.ts` harness (real `q2 preview` binary). New
      `firefox-ws-queue` Playwright project runs only this spec; the
      chromium project runs the same spec as the unserialized
      control. Two findings baked into the spec: (a) tab A must be a
      real `http://127.0.0.1` page — sockets opened from `about:blank`
      don't enter Firefox's admission queue (the spec uses the
      server's `/health` page); (b) a precondition assertion pins tab
      A's probe socket in `readyState === CONNECTING` so the test
      can't pass vacuously if the blackhole isn't holding the slot.
- [x] State-based, not latency-based: asserts rendered content
      appears and the terminal overlay never does.
- [x] Pinned pref — **revised from the original 3 s**: the open
      timeout must EXCEED the legacy 5 s peer budget or the pre-fix
      bug can't reproduce (a 3 s release beats the old deadline and
      masks it). Pinned at 8 s, documented in spec + config.
- [x] Watch-it-fail: against the Jun-10 binary (pre-fix embedded
      SPA): fails with the exact production failure ("Render Error —
      Document automerge:<id> is unavailable"). After
      `cargo xtask build-q2-preview-spa` + `cargo build -p quarto
      --bin q2`: passes in 9.1 s (one 8 s queue-release cycle +
      render). Chromium control + full chromium e2e suite: 31 passed;
      3 pre-existing failures in `edit-cell-sizing.spec.ts` traced to
      bd-ov4gqk3m's read-only gating (helper never passes
      `--allow-edit`) — filed as bd-dsgrew1x (discovered-from
      bd-jit6pdwq), predates this branch.
- [ ] **Demotion policy** (agreed 2026-06-11): this spec is never
      PR-blocking (lives behind `--e2e`). If it flakes twice, demote
      it to a documented manual harness (promote the scripts out of
      `claude-notes/tmp/`) and let the Phase 1/2 vitest guards be the
      durable regression net. Known benign decay: if Firefox ever
      relaxes per-IP handshake serialization, the blackhole stops
      mattering and the spec degrades into a plain boot test (passes
      vacuously) — acceptable.

### Phase 5 — verification + bookkeeping

**The end-to-end checks found two real bugs the unit/integration
suites missed** (vindicating the CLAUDE.md e2e policy):

1. **Zombie adapters.** With the server dead, the SPA still made WS
   attempts every \~3 s ("zero WS churn" check failed). Two causes,
   both fixed:
   - the boot controller abandoned in-flight attempts without
     tearing down their transport → new `teardown` option, called on
     both server-gone exits (3 new bootController specs);
   - upstream `BrowserWebSocketClientAdapter.disconnect()` clears the
     retry interval but NOT the onClose-scheduled reconnect
     `setTimeout` — a discarded adapter resurrects itself. Fixed via
     `StoppableWebSocketClientAdapter` (terminal disconnect; the
     local-subclass fallback this plan reserved). Control test
     documents the upstream bug and detects when upstream fixes it.
     Same gate added to our `NodeWebSocketClientAdapter` (same bug).
     Upstream PR target: automerge/automerge-repo,
     `WebSocketClientAdapter.ts` `disconnect()`.
2. **Stale doc id across server restart.** The kill/restart check
   never recovered: q2 preview keeps samod state in a per-process
   temp dir, so a same-port restart serves a NEW index document and
   reconnecting with the boot-time id retries "unavailable" forever.
   Fixed: the SPA re-fetches the doc id from `/health` on every
   connect attempt (health-gating already guarantees /health answers
   first). New integration test pins it.

Also fixed in passing: quarto-sync-client's tsconfig compiled
`*.test.ts` into `dist/`, where vitest's default glob double-ran
stale copies (now excluded from the build).

- [x] `cargo xtask verify` (full) — green at Phase 4 HEAD and re-run
      green at final HEAD.
- [x] End-to-end evidence (real `target/debug/q2` + real Firefox,
      scripts in `claude-notes/tmp/`):
      - `verify-kill-restart.mjs`: render → SIGKILL server → banner
        "Reconnecting to the preview server…" with content retained →
        restart on same port (new doc id `35mH6uaQ…`) → banner
        cleared \~1 s later, heading re-rendered, no reload. PASS.
      - `verify-no-ws-churn.mjs`: 1 WS attempt at boot; **0** WS
        attempts during a 12 s window with the server dead (HTTP
        /health polling only). PASS.
      - `e2e/firefox-ws-queue.spec.ts`: pre-fix binary fails with the
        production error; post-fix passes in 9.1 s. Chromium control
        green.
- [x] Final suite counts: quarto-sync-client 84, preview-runtime 74,
      spa unit 25, spa integration 67, hub-client 575 — all green;
      `tsc -b` + vite builds green.
- [x] hub-client changelog: not needed — no `hub-client/` files
      changed (verified via git diff).
- [ ] braid: final comment + close decision (with user, post-review).

## Details & decisions

- **Options bag, not more positionals.** `connect()` already takes 7
  positional params; the Phase 1 additions go in a trailing options
  object. Hub-client call sites untouched.
- **`Infinity` peer wait is preview-only.** Hub-client keeps its 1 ms
  probe → IndexedDB-first behavior. The sync client stays
  policy-free; the *SPA* owns the "HTTP decides liveness" loop.
- **Why memory storage for preview.** (a) removes the IndexedDB open
  from the path that gates sending the WS `join` (independently
  verified failure mode — automerge-repo defers `adapter.connect()`
  on `storageSubsystem.id()`); (b) preview origins are
  port-randomized, so the cache can never hit and every session
  permanently adds an origin database to the user's profile.
- **Adapter retry interval: keep default (or raise), never shorten.**
  Both numbers are under our control (`peerTimeoutMs` is our literal in
  `PreviewApp.tsx`; `retryInterval` is the second ctor arg of
  `BrowserWebSocketClientAdapter`, built in `client.ts` `buildWsAdapter`).
  But shortening the retry is *counterproductive in Firefox*: the
  adapter's retry fires while the previous socket is still CONNECTING
  and replaces it without `close()`, and abandoned CONNECTING sockets
  keep occupying Firefox's per-IP admission queue until the open
  timeout. Faster retries ⇒ more queue pollution under exactly the
  failure condition we're fixing. With `peerTimeoutMs: Infinity` the
  old de-alignment concern (5 s budget == 5 s retry) is moot; the
  SPA's health-gated loop is the reconnect driver and the adapter's
  internal retry is a background fallback we want quiet. The
  `retryIntervalMs` option (Phase 1) exists so preview can *raise* it
  if Phase 3 testing shows residual churn.
- **findDoc retry stays bounded.** Unbounded retry could mask real
  "doc id mismatch" bugs; 3 attempts gated on peer presence is enough
  to cover the cold-start sync race already documented in
  `client.ts`.
- **Not in scope:** patching/vendoring
  `BrowserWebSocketClientAdapter`\'s abandon-without-close retry
  behavior. Health-gating makes the SPA stop driving that loop;
  upstreaming a `socket.close()`-before-replace fix is a separate
  nice-to-have. Target for that PR: `automerge/automerge-repo` on
  GitHub, `packages/automerge-repo-network-websocket/src/
  WebSocketClientAdapter.ts` (we consume it as
  `@automerge/automerge-repo-network-websocket` 2.5.6). Local
  fallback if upstream stalls: subclass the browser adapter in
  `quarto-sync-client` (precedent: our `NodeWebSocketClientAdapter`).
  File a follow-up strand if Phase 3 testing shows abandoned
  CONNECTING sockets still piling up.
- **UX in non-Firefox browsers:** unchanged paths everywhere —
  fast-connect case resolves identically; the new states only render
  when connection actually stalls/drops (where today's behavior is a
  permanent error screen).

## Success criteria

1. Firefox + blackhole tab: preview renders (≤ 30 s worst case),
   never the permanent offline error. Chromium: no measurable boot
   regression.
2. Killing the preview server produces the "server stopped" banner and
   zero ongoing WS retries; restarting it recovers the tab without a
   manual reload.
3. All existing quarto-sync-client / hub-client / preview-spa suites
   green; `cargo xtask verify` green.
