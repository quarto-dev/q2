# Project import error UX: "Document … is unavailable"

**Strand:** bd-tux4m6od
**Date started:** 2026-07-29

## Overview

Sharing a project collection on quarto-hub.com from one browser to another
fails with the generic error:

> Document 2Agx7kENjysHSujsVgirvykVKECf is unavailable

This tells the user nothing about the cause (not signed in? sync server
unreachable? document genuinely missing? permission denied?) and offers no
next step. The recent change to how hub-client project collections work is
the suspected trigger, but the immediate goal is twofold:

1. **Diagnose** the actual failure in the production deployment (live
   observation via a DevTools-controlled Chrome session, with the user
   logged in), and
2. **Improve** the error reporting on the project-import path so each
   distinct failure mode produces a specific, actionable message.

## Phases

### Phase 1 — Live diagnosis (production)

- [x] Open a DevTools-controlled Chrome window; user logs in to
      quarto-hub.com and navigates to the failing share flow
- [x] Capture console messages, network requests (auth, /ws websocket,
      sync traffic), and the exact UI state when the error appears
- [x] Record the failing document id, URL shape, and any correlated
      server responses (route is `#/join-collection/<id>?name=…&from=…&server=…`)
- [x] Match the observed error string to its source in hub-client

### Phase 2 — Source-code mapping

- [x] Locate where "Document <id> is unavailable" is produced
      (automerge-repo `Repo.find()`, surfaced raw by
      `JoinCollectionLanding.tsx:74` via `projectSetService.ts:237`)
- [x] Trace the project-collection import path; enumerate distinct
      failure modes (see Phase 1 findings: not-found / auth-expired /
      offline / cold-ws race; permission denial impossible today —
      access policy is audit-only)
- [x] Determine which failure mode the observed production error is
      (transient cold-ws race most consistent with observations;
      fix handles all modes regardless)
- [x] Survey prior art: `quarto-sync-client`'s `findDoc` already
      retries the cold-start unavailable race with peer-gating
      (bd-jit6pdwq) and locks friendlier per-surface messages
      (bd-vm5e5u10, 2026-06-12 incident). Mirror that pattern.

### Phase 3 — Test-first fix (TDD)

**Design** (details in "Fix design" below):

- New `hub-client/src/services/collectionConnectError.ts`:
  `CollectionConnectError` with
  `kind: 'auth-expired' | 'offline' | 'sync-unreachable' | 'not-found' | 'unknown'`,
  user-facing `message`, `docId`, `cause`.
- `projectSetService.acquireServer` gains live peer tracking
  (`peer` / `peer-disconnected` events → count) and
  `whenConnected(timeoutMs)`.
- `connectCollection`: replace bare `repo.find()` with a bounded
  retry loop (mirrors sync-client `findDoc`: per-attempt
  `AbortSignal.timeout`, retry-while-peer-connected, wait-for-peer
  when none yet — this fixes the 1 s forceReady race). On final
  failure, classify: peer connected → `not-found`; no peer →
  probe `fetchAuthMe()` (null → `auth-expired`, ok →
  `sync-unreachable`, throw → `offline`).
- `JoinCollectionLanding` renders the classified message (and a
  sign-in-again action for `auth-expired` if the pre-auth-hash
  restore makes it trivial).

**Work items:**

- [x] Write failing service tests
      (`projectSetService.connect.test.ts`): fake ws adapter +
      loopback pair to a real server-side `Repo`; scenarios:
      (a) race-fix: peer connects after forceReady → join succeeds;
      (b) not-found: connected, server lacks doc;
      (c) auth-expired: no peer, `/auth/me` 401 (auth enabled);
      (c') auth-disabled builds map a 401 probe to sync-unreachable;
      (d) offline: no peer, `/auth/me` network error;
      (e) sync-unreachable: no peer, `/auth/me` ok;
      (f) cache-hit works offline (no regression)
- [x] Write failing message-copy tests
      (`collectionConnectError.test.ts`, wording locked like
      dangling-entries.test.ts)
- [x] Write failing component tests
      (`JoinCollectionLanding.test.tsx`, jsdom): distinct copy per
      kind; auth-expired offers "Sign in again"; fallback for
      unknown errors; button re-enables for retry
- [x] Run tests, verify they fail as expected (red: modules missing)
- [x] Implement `collectionConnectError.ts`
- [x] Implement `projectSetService` changes (live peer tracking
      replaces the latched first-peer promise; `findCollectionDoc`
      with bounded find → wait-for-peer → bounded re-request →
      classify; `ConnectCollectionTuning` for tests)
- [x] Implement `JoinCollectionLanding` changes (JoinError state,
      `onSignInAgain` defaulting to reload — main.tsx's pre-auth-hash
      save/restore returns the user to the join route after re-auth)
- [x] All new tests green (23); full `npm run test:ci` green
      (unit 769 + integration + wasm legs)
- [x] `npm run build:all` green
- [x] End-to-end verify in a real browser session (local-prod,
      2026-07-30): `npm run build:local-prod` + `npm run local-prod`,
      navigated Chrome to
      `http://127.0.0.1:8080/#/join-collection/4TftfZrEQU2XZmf4NHErVEGaP6s6?name=Phantom&…`,
      clicked Join. Observed inline error (screenshot inspected):
      "This collection isn't available on the sync server (document
      4TftfZrEQU2XZmf4NHErVEGaP6s6). The link may be stale, or the
      collection may not have finished syncing from its owner — ask
      them to open Quarto Hub and share it again." — the classified
      not-found copy, replacing the old raw automerge message.
      *Gotcha hit during verification:* the PWA service worker served
      a stale precached bundle on first load (old message reappeared);
      after unregistering the SW + clearing caches the new bundle
      (`main-D4UZon0u.js`) loaded and behaved correctly. Worth
      remembering for any local-prod verification of hub-client
      changes.

### Phase 4 — Wrap-up

- [x] Update hub-client/changelog.md (two-commit workflow:
      d947fab2 fix + a02cde3f changelog, on
      `braid/bd-tux4m6od-join-collection-error-ux`)
- [x] Push branch + open PR: quarto-dev/q2#439
      (`feature/bd-tux4m6od-join-collection-error-ux`); CI watched
      from the session
- [x] CI green: TS Test Suite (run 30569132769) and Test Suite
      (run 30569134654) both pass on PR head 65f52199 — run via
      `workflow_dispatch` because GitHub's webhook event delivery
      for the repo stalled ~17:57Z on 2026-07-30 (PR open/reopen/
      close/synchronize all unprocessed; status page claimed
      operational). The PR checks box shows only Snyk until the
      backlog clears; empty commit a6340044 pushed to fire
      `synchronize` when it does.
- [ ] Close bd-tux4m6od with findings recorded

**Open question (closed unresolved):** whether the original failing
browser hit the 401 (expired session) or the slow-101 (forceReady
race) mode is unknowable now — the tab was lost before its Network
panel could be checked. Moot for the fix: both modes are classified,
and the race mode is eliminated by the wait-for-peer retry.

## Notes / Findings

### Phase 1 findings (2026-07-29, live against production)

**Where the string surfaces.** `JoinCollectionLanding.tsx:74` displays
`err.message` verbatim. The error is thrown at
`hub-client/src/services/projectSetService.ts:237` —
`await server.repo.find(docId)` — and the message text is automerge-repo's
own rejection (`node_modules/@automerge/automerge-repo/dist/Repo.js:545`,
`Document ${id} is unavailable`).

**At least three distinct failure modes converge on this one string:**

1. **Server genuinely lacks the document** (healthy, authenticated ws).
   *Reproduced live in production*: navigated the controlled browser to
   `#/join-collection/3K9ZU74uq8d129Qed8k5W3GtbzKu?...` (valid random
   bs58check id), clicked Join while logged in with a working
   connection → identical error UI; console shows only
   `Join failed: Error: Document 3K9ZU74uq8d129Qed8k5W3GtbzKu is unavailable`,
   no network errors. The hub replied doc-unavailable.
2. **Websocket cannot connect — auth expired.** The `/ws` upgrade
   returns 401 on missing/expired `quarto_hub_token` cookie
   (`crates/quarto-hub/src/server.rs:1489-1535`). The client adapter
   retries forever, but `WebSocketClientAdapter.js:70` **force-marks the
   adapter ready after 1 s**; `Repo.find()` then requests the doc with
   zero connected peers → handle transitions straight to UNAVAILABLE →
   same message. The server's own doc comment says clients are expected
   to "reconnect and re-authenticate when the frontend detects token
   expiry" — the join screen has no such detection.
3. **Slow/cold websocket at click time (transient race).** Same 1 s
   forceReady mechanism: any handshake slower than ~1 s produces the
   error even though the connection succeeds moments later. Matches the
   observed flakiness: the user's click on `Join Personal` failed;
   a retry seconds later (same window, same doc) succeeded.

**Supporting observations:**

- The nicer custom messages in `connectCollection`
  (`projectSetService.ts:239-250`, e.g. "Collection not found in local
  storage. Connect online first to sync.") are **dead code for the
  no-local-cache path**: `find()` throws before that branch is reached.
- The join button's enable-gate (`status === 'connected'`) is satisfied
  by loading the *root* collection from IndexedDB cache — no network
  involved — so the button is clickable while the ws is down/401-ing.
- Once a handle is UNAVAILABLE it is cached (`Repo.js:398-407`); a
  retry can return terminal-unavailable instantly, though the handle
  recovers if the doc later arrives from a peer.
- The server-side access policy is audit-only (`access_policy.rs`,
  always `true`), so "unavailable" is never a disguised permission
  denial today.
- Auth note: an unauthenticated browser never sees the join screen at
  all (it hits the Google sign-in gate first) — verified in an isolated
  browser context.

**Open question:** which mode hit the user's original failing browser —
(2) stale session vs (3) cold-ws race. Discriminator: in the failing
browser's Network tab, the `wss://quarto-hub.com/ws` row shows 401
(mode 2) or 101-but-late (mode 3).
