# Instant project open from the project list

## Note from Elliot

It should not take time to switch views from the project list to the project view. That is some SaaS stuff. We really only need to load the project automerge document before we can switch views. That will happen instantly if the project is in indexeddb.

## Overview

Opening a project from the projects home blocked the UI on "Connecting
to sync server…". The wait was never the sync server; the open path
serially awaited (1) an HTTP round trip to `/auth/actor` for the
Automerge actor ID, and (2) a 400 ms `waitForPeer` budget that existed
only so the online indicator wouldn't flash Offline→Online. With those
gone, a cached project opens straight from IndexedDB.

Final shape (elliot, 2026-08-28, after three rounds of simplification):
**hub-client-only change, no ts-packages changes, no actor-ID cache.**
Blocking on the `/auth/actor` fetch on every open is fine; loading all
file docs before the switch is fine (IndexedDB, concurrent, fast) — it
also means the editor and preview arrive fully formed, no
streaming/flashing.

## What was done

- App.tsx: peer wait reverted 400 ms → the 1 ms probe (offline-first;
  the Offline→Online indicator flash is accepted — cosmetics lose to
  instant opens). E2E keeps its 15 s budget.
- Projects list: the global "Connecting to sync server…" banner is gone
  (ProjectsHome + classic ProjectSelector); instead an italic
  `opening...` appears beside the clicked entry's name (both card and
  row variants; cleared if the connect errors, unmounted on success).
  The now-unused `isConnecting` state/prop was removed end to end
  (App, ProjectsHome, ProjectSelector, DevHarness fake props).
  ProjectSetSetup keeps its own `isConnecting` prop.
- Editor.tsx: `loading=""` on MonacoEditor — monaco is bundled
  (`monacoSetup.ts`), so the only wait is per-mount editor-instance
  creation (a frame or two); the default "Loading..." text just flashed
  on every open and file switch.

## Explicitly tried and backed out (don't re-add without a new reason)

- localStorage actor-ID cache (`actorIdCache.ts` +
  `resolveActorIdCached`) — the actor ID is a deterministic
  per-(user, project) HMAC and so cacheable, but elliot prefers
  fetching `/auth/actor` on every open (one same-origin round trip)
  over carrying the cache.
- `ConnectOptions.backgroundFileLoad` (connect resolves on index only,
  contents stream in) + a bounded wait for the to-be-opened file —
  elliot preferred loading everything before the switch; IndexedDB
  makes it fast enough.
- Sync-client `setActorId` late binding + `editsLocked` Monaco gate +
  actor generation counter — only needed to avoid blocking on an
  uncached actor fetch; elliot is fine blocking there.
- `peerTimeoutMs: 0` "skip the wait entirely" semantic in the sync
  client — the 1 ms probe from hub-client achieves the same without
  touching ts-packages.
- Blanking the preview-loading placeholder texts ("Loading preview...",
  "Loading q2-preview renderer...") — elliot reverted; keep as is.

## Verification (2026-08-28)

- quarto-sync-client 137 passed (after clearing stale compiled
  `dist/*.test.js` artifacts that broke a bare `npm test` there —
  pre-existing; dist is gitignored and the current tsconfig excludes
  tests, so a clean rebuild fixes it permanently).
- preview-runtime 77 passed; hub-client unit 1064 passed, integration
  114 passed, WASM 133 passed; hub-client strict build (`tsc -b` +
  vite + WASM) clean.
- quarto-hub-mcp: 250 passed, 1 pre-existing e2e-auth failure
  ("banned mid-session" stderr timeout) — fails identically with all
  these changes stashed; not from this work.
- In-browser verification: elliot (done iteratively during the session).
