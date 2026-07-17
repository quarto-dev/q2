# hub-client: connection-gated auth + local-first documents

**Status:** proposed
**Date:** 2026-07-06
**Part of:** the auth-reshape path — see the umbrella index `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`. Independent of the two sibling plans (Part B PKCE unification; server-minted sliding sessions). Publishing a local project to a hub is the separate adoption follow-on `claude-notes/plans/2026-07-06-hub-client-local-project-adoption.md`.

## Overview

The SPA opens straight into a usable app (project selector + editor) with **no
login gate**. Users create and edit Automerge documents that live entirely in
browser IndexedDB. Authentication is required **only** when the user chooses to
connect to a hub server.

**v1 scope:** local-first documents + gate removal, and a "Connect to a hub"
action that authenticates and **opens or creates hub-side projects** (today's
connect path, HMAC actor from the first change). Local projects stay local.

**Publishing an existing local project up to a hub** — the forward actor-switch
+ display-bridge and the sync-up — is a **separate follow-on plan**
(`2026-07-06-hub-client-local-project-adoption.md`), deferred because it is the
only part that hard-depends on the unsolved offline-doc durability problem
(`bd-10bdjmjb`, D1) and carries all the actor-reconciliation risk. v1 delivers
the headline local-first value with **zero dependency on D1**.

This effort is **pure hub-client / sync-client work — no server-auth changes**,
which is why it ships first.

## Current architecture (map for implementers)

> Line numbers are approximate as of 2026-07-06; grep to confirm before editing.

### Entry gate (the target — small)
- `hub-client/src/App.tsx:82` — `AUTH_ENABLED = !!import.meta.env.VITE_GOOGLE_CLIENT_ID` (module-scope, build-time).
- `hub-client/src/App.tsx:581-587` — loading gate (spinner).
- `hub-client/src/App.tsx:589-596` — **the entry gate**: `if (AUTH_ENABLED && !auth) return <LoginScreen/>`. Nothing renders until this passes.
- `hub-client/src/App.tsx:85-93` — `useAuth()` on mount calls `GET /auth/me` for the whole app. **Already non-blocking at the hook level** — its mount effect `.catch()`es a 401/network failure and just clears `loading` (`useAuth.ts:81-84`); the only thing that blocks first render is the App.tsx gate above.
- `hub-client/src/App.tsx:400-409` — disconnect-on-auth-loss (ties auth lifecycle to project lifecycle); must be scoped to *connected* projects so a local project is never torn down on auth loss.
- When `VITE_GOOGLE_CLIENT_ID` is unset, `AUTH_ENABLED=false` and both gates are skipped today — but that is a *build/deploy* toggle, not a runtime choice. This plan makes "no gate" the default runtime behavior.

### Connection-auth surface (already connection-scoped — reuse as-is)
- Ambient `quarto_hub_token` HttpOnly cookie on the same-origin WS upgrade (`ts-packages/quarto-sync-client/src/client.ts:135-144`; the browser attaches it automatically — no token in the WS URL).
- `resolveActorId` → `GET /auth/actor?project=…` per connect (`hub-client/src/services/authService.ts:82-94`, called at `App.tsx:128-131`).
- `useAuthProbe` watchdog, gated on `AUTH_ENABLED && !!auth && !!project && !isOnline` (`App.tsx:126-130`) — its `enabled` guard is part of the `AUTH_ENABLED` reconciliation in A4.

### Local-first infrastructure (~80% present)
- Document IDs minted **client-side** via `generateAutomergeUrl()` (`client.ts:1529`) — no server needed for identity.
- `IndexedDBStorageAdapter` persists all docs across reloads (`ts-packages/quarto-sync-client/src/storage-adapter.ts:72`).
- `connect` and `createNewProject` already have offline fallbacks (1 ms peer-wait → load/create from IndexedDB) (`client.ts:830-846`, `:1503-1527`).
- A storage-only, no-network Repo is **proven** in the debug harness: `createLocalStorageRepo` = `new Repo({ storage })` (`hub-client/src/debug/services/repo.ts:80`).
- automerge-repo supports attaching a network adapter later (`repo.networkSubsystem.addNetworkAdapter`) — used by the adoption follow-on.

### Local-first blockers (what this plan must change)
- `hub-client/src/services/projectSetService.ts:191-211` — `createProjectSet` hard-requires a live server (`waitForPeer(repo, 10000)`, throws "Could not reach sync server"). (Note: the sibling `connect` at `:122-183` already degrades gracefully offline — only *creation* is fatal.)
- Every production Repo is built *with* a WS network adapter (`client.ts:820-824`, `:1496-1499`; `projectSetService.ts:128-129`). No `network: []` path in the main app.
- `syncServer` is a mandatory field (`projectStorage.ts:64`, the synced set entry, `App.tsx:507`).

### Actor-ID mechanics (relevant to both local authorship and hub-project connect)
- Server derivation: `actor = hex(HMAC-SHA256(server_secret, sub || "\0" || project_id))` — stable per `(user, project)`, trusted (derived from verified `sub`), not client-computable (`crates/quarto-hub/src/auth.rs:729-737`). This is the actor a **hub** project uses from its first change (via `resolveActorId` at connect).
- `GET /auth/actor?project=<id>` does **no project validation** (`server.rs:788-790`) — returns the actor for any project id, seen or not. (This is what makes the adoption follow-on possible: the client can derive the stable actor for a locally-minted project the moment the user authenticates.)
- Actor set per-document via `applyActorId(handle, actorId)` = `handle.update(doc => automergeClone(doc, { actor }))` (`client.ts:475-478`), called on every `findDoc` (`client.ts:519`) — so authoring a handle under a chosen actor while preserving history is routine and proven.
- Null actor (auth disabled / offline today) → Automerge assigns a **random actor per handle, re-randomized on every reload** — authorship noise. Nothing persists a local actor today. **A3 fixes this.**
- Authorship maps actor→user via `IndexDocument.identities: Record<actorId, {name,color}>` (`ts-packages/quarto-automerge-schema/src/index.ts:24-27,58-63`), written by `setIdentity` (`:105-115`). Display falls back to `actor.slice(0,8)` + hashed color when no row exists (`hub-client/src/hooks/useAttribution.ts:69-87`). "Me" = the Automerge actor (`claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md`).

### Server connection auth (unchanged by this plan)
The hub validates the ambient cookie on every WS/REST call regardless of what the
SPA renders (`server.rs:901-950`). Removing the client gate does **not** weaken
server enforcement — an auth-required hub simply 401s the WS upgrade, which
becomes the trigger to log in. (Details live in the Part B plan.)

## Design

### A. Local-first documents + persisted local actor (v1)
The pre-connect (local, no auth) state a project lives in:
1. Client mints `indexDocId` (`generateAutomergeUrl()`) — already client-side.
2. Client generates a **persisted per-browser local actor** (16 random bytes → 32 hex, a valid Automerge actor id) and stores it in IndexedDB keyed to the local project (A3). *New persistence — nothing stores an actor today.* This replaces the current random-per-reload behavior with a stable local identity. **This is the authorship UX we want:** local edits attribute to one coherent author across reloads instead of a new 8-hex stub each time.
3. SyncClient builds a **storage-only Repo** (no network adapter — new path, mirrors the proven `createLocalStorageRepo`) (A2).
4. All docs are authored under `localActor` via `applyActorId`; write `identities[localActor] = { name: <local display name, e.g. "You">, color }` so local edits attribute coherently (A3).
5. Project recorded in the local list (IndexedDB), `syncServer` unset (A1/A2).

In v1 the local actor **persists for the life of the local project — it is never
switched.** Switching to the server-trusted HMAC actor when a local project is
*published* to a hub (the forward switch + display bridge) is the **adoption
follow-on**: `2026-07-06-hub-client-local-project-adoption.md`.

### B. Gate removal & connection UX
- Convert `AUTH_ENABLED` from a *gate* into runtime *capability* state: the app shell + project selector always render. `useAuth()` still probes `/auth/me` on mount to recognize an existing session, but a 401 / network error is non-blocking (already true at the hook level — see the entry-gate map; the render gate is what must go). Must tolerate a backend-less static deploy.
- Add an explicit **"Connect to a hub"** action that: (a) triggers login if there is no session, (b) **opens or creates hub-side projects** (today's connect path, HMAC actor from the first change). *Publishing an existing local project up to the hub is the adoption follow-on — not wired into this action in v1.*
  - **Placement (decided 2026-07-15): a header/account-level control**, not a per-project action button. Hub connection is *session-scoped* state (the ambient cookie), so it lives at account level next to the existing sign-in/out affordance: the header shows **"Connect to a hub"** when disconnected and **"Signed in as X ▾ / Sign out"** when connected. The three project-action buttons stay pure project operations — Create/Import default to **local**; hub open/create surfaces only once connected. This deliberately keeps "Connect to a hub" out of the action-buttons row so it does **not** collide with the existing **"Connect to Project"** button (`ProjectSelector.tsx:664-675`), which means the unrelated "join an existing Automerge doc by ID + sync-server URL" flow.
  - The header connection control replaces today's `onSignOut`-only header block (`ProjectSelector.tsx:553-565`), which is currently gated on `AUTH_ENABLED`; A4 rewires it to runtime connection state.
- Reconcile the `AUTH_ENABLED`-driven props (`onSignOut`, `authEmail/Name/Picture` on `ProjectSelector`, `App.tsx:657-662`) and the `useAuthProbe` `enabled` guard to runtime auth state.

## Phases (TDD-first) — v1, no D1 dependency

- [x] **A0 — Test scaffolding.** Fixtures for a local-only project (no server); a fake/mock hub for the connect leg (open/create a hub project); extend `MockAuthProvider` usage; E2E harness that boots the SPA with **no** `VITE_GOOGLE_CLIENT_ID` and with an auth-required hub. Write the failing tests for A1–A4 first.
  - **Done:** the reusable scaffolding is the set of red-then-green `*.local.test.*` fixtures established per phase over fake-indexeddb + a no-server sync-client factory: `client.local.test.ts`, `localFirstSync.test.ts` (A2), `projectSetService.local.test.ts` (A1), `localActor.test.ts` (A3), `useProjectSet.local.test.ts` + `ProjectSelector.connect.test.tsx` (A4). Each was written failing before its phase's implementation. The auth-required-hub E2E harness proper is A7v1.
- [x] **A1 — Local-only project-set creation.** Add a local creation path to `projectSetService` (mint set-doc id client-side, store in IndexedDB, defer sync); make `waitForPeer` non-fatal for local mode. Tests: create a project fully offline; reload and see it listed; no network call made.
  - **Done:** `createLocalProjectSet()` + `connectLocal()` build a storage-only Repo via a shared `buildRepo(syncServer?)` helper (no WS adapter, no `waitForPeer`); `flush()` + flush-on-disconnect persist local changes. Schema: `ProjectSetEntry.syncServer` is now optional (absent = local); `addProjectToSet` omits the key rather than storing `undefined`. IDB layer uses `''` as the local sentinel (`ProjectSelector` passes `entry.syncServer ?? ''`). Test: `projectSetService.local.test.ts` (create → add → reload → still listed, `syncServer` undefined).
- [x] **A2 — Storage-only Repo + optional `syncServer`.** Add a `network: []` / deferred-adapter construction path in `SyncClient` (`connect`, `createNewProject`); make `syncServer` optional through the creation flow + schema. Tests: create/edit/persist a doc with no network adapter; VFS + WASM render works offline (render path is already network-free). *(A1 and A2 apply the same "network-less Repo + non-fatal `waitForPeer`" change at two construction sites — `projectSetService.ts:129/197` and `client.ts:821/1496`; factor one shared helper + fixture rather than two parallel edits.)*
  - **Done:** `buildRepo({ syncServer? })` in `client.ts` returns a storage-only Repo (no WS adapter) when `syncServer` is empty/undefined; `connect`/`createNewProject` skip `waitForPeer` in that mode. `CreateProjectOptions.syncServer` is now optional. Added `SyncClient.flush()` (+ preview-runtime wrapper) and flush-on-create so a local project survives an immediate reload. Tests: `client.local.test.ts` (no-network contract) + `hub-client/.../localFirstSync.test.ts` (create→reload→read-back durability over fake-indexeddb).
- [x] **A3 — Persisted local actor.** Generate + persist a per-browser local actor in IndexedDB; author local docs under it; write the local `identities` row. In v1 this actor **persists for the life of the local project** — no switch (that is the adoption follow-on). Tests: two reloads → same actor on all changes (no re-randomization); authorship displays the local name, not an 8-hex stub.
  - **Done:** `getOrCreateLocalActor()` in `userSettings.ts` mints a stable 32-hex actor (16 random bytes) once per browser, persisted in the userSettings `identity` singleton (`UserSettings.localActorId`). Authoring under it is proven via the sync client (`createNewProject(files, localActor, name, color)` → `getActorId()` matches, `identities[localActor]` row present, stable across a reopen). Test: `localActor.test.ts`. *App wiring (make local-create pass this actor) lands in A4, which owns the local-first create path.*
- [x] **A4 — Gate removal.** Delete the App.tsx render gate; reconcile `AUTH_ENABLED` UI props + the `useAuthProbe` guard to runtime state; scope the auth-loss→disconnect effect (`App.tsx:400-409`) to connected projects. (`useAuth` is already non-blocking on `/auth/me` failure — no hook change needed there.) Tests: SPA usable with no IdP configured and with a backend-unreachable deploy; login no longer precedes first render; local project survives an auth-loss event.
  - **Done:** Removed the `AUTH_ENABLED`-gated LoginScreen + loading gates; sign-in is now an opt-in overlay shown only via `showLogin` (header "Connect to a hub") or a redirect `authError`. `useProjectSet` auto-creates a **local** project set on first run (no pointer + no legacy) and branches `connect`/`connectLocal` by pointer syncServer. `handleProjectCreated` branches local (authored under the local actor, no server) vs hub; `resolveActorForOpen` picks local-actor vs HMAC by the project's syncServer. `useAuthProbe` + the disconnect-on-auth-loss effect are scoped to `project?.syncServer` (hub only) so a local project is never torn down. Header shows "Connect to a hub" (disconnected) / "Signed in as X · Sign out" (connected); Create/Import default to local (sync-server field removed). Tests: `useProjectSet.local.test.ts`, `ProjectSelector.connect.test.tsx`. Full hub-client unit suite (710) + production build green. *(E2E boot + auth-required-hub verification is A7v1.)*
- [x] **A7v1 — End-to-end verification + docs.** Real browser: create a project offline, edit, reload (persists), then connect to a running hub and open/create a **hub** project. Record the exact steps + observed output per the repo's end-to-end policy. User-facing docs for local-first + connecting.
  - **Done (local leg, automated real-browser E2E):** `e2e/local-first.spec.ts` drives headless Chromium against the production bundle:
    - Invocation: `cd hub-client && VITE_E2E=1 npm run build && npx playwright test local-first.spec.ts --project=chromium`.
    - Observed: **2 passed.** (1) The app opens directly to the "Your Projects" selector with **no login gate**; the header shows a **"Connect to a hub"** button. (2) Creating a project (first project type, local — the create form has no sync-server field) navigates into the editor at `#/p/<id>/file/_quarto.yml` with **no hub contacted**, and after `page.goto('/')` the project is **still listed** (persisted in IndexedDB). This exercises gate removal + local bootstrap + local-actor authoring + durability through the real binary/bundle.
  - **Docs:** `hub-client/LOCAL-FIRST.md` (user-facing local-first + connect model), cross-linked from `README.md`; distinguished from the PWA asset cache in `OFFLINE.md`.
  - **Hub-connect leg (open/create a hub project after sign-in):** requires a live OIDC provider, which the headless harness has no client id for, so it is **not automated**. Manual verification path recorded in `LOCAL-FIRST.md` + the local-prod runbook in the repo `CLAUDE.md`. Honest status: the local leg is verified end-to-end in a browser; the OIDC sign-in leg is verified by unit tests (probe/teardown scoping, actor branching) + manual local-prod, not by an automated browser sign-in.

> The adoption follow-on (A5 — connect+adopt, A6 — offline-doc durability/D1, A7adopt — E2E) lives in `2026-07-06-hub-client-local-project-adoption.md`, `conditional-blocks` on `bd-10bdjmjb`.

## Follow-up fixes (post-A4)

- **Logged-off hub-project open was a silent no-op** (interactive testing). With
  the gate removed, a logged-off user reaches the selector and can click a hub
  project (one with a `syncServer`), but `resolveActorId` returned `null`
  (401) and the open path just `return`ed — nothing happened, no feedback.
  Fix: extracted `resolveActorForOpen` (`src/services/openActor.ts`) — local
  projects always open under the local actor; a hub project whose actor
  resolves to `null` now fires an `onNeedsSignIn` callback (App shows the
  sign-in screen + a "Sign in to open this hub project" message) instead of
  failing silently. `undefined` (auth-disabled/insecure hub) still opens with
  no prompt. Tests: `openActor.test.ts` (4 cases). *(Offline-read of a
  cached hub project while logged off is intentionally NOT done here — that is
  offline-durability / D1 territory.)*

- **Sync Server URL field restored to Create/Import** (user request,
  bd-ivkf752c, discovered-from bd-u4p8xhdc). A4 removed the field entirely,
  defaulting silently to `projectSetSyncServer ?? ''`. Reverted: the field is
  back in both forms and editable. Tests: `ProjectSelector.create.test.tsx`
  (new), `ProjectSelector.import.test.tsx`, `ProjectSelector.connect.test.tsx`
  (updated); `e2e/local-first.spec.ts` updated.

- **Regression: unconditional `DEFAULT_SYNC_SERVER` default broke local
  creation** (interactive testing, follow-up to bd-ivkf752c). The field
  restoration above initially defaulted to `DEFAULT_SYNC_SERVER`
  unconditionally (matching the Connect form). That silently turned local
  creation into a hub-creation attempt whenever the user didn't clear the
  field: `isLocal = !syncServer` in `App.tsx` saw a non-empty value and took
  the hub path; `createNewProject`'s `resolveActorId` callback got a 401 (no
  session) but `client.ts:1568-1571` swallows that (`?? undefined`) instead
  of aborting, so the project was created anyway, wired to a real WS
  adapter — then immediately torn down by the auth-loss-teardown effect
  (`App.tsx:480-489`, correctly acting on the now-truthy `syncServer`). User-
  visible symptom: a flash of the editor on create *or* on reopening that
  project, then bounced back to the selector. Fix: the field now defaults to
  `projectSetSyncServer ?? ''` (empty/local when not connected to a hub, the
  connected hub's server when connected) via new state
  `newProjectSyncServer`, reset on each Create/Import form open — separate
  from the Connect form's own `syncServer` state, which legitimately keeps
  defaulting to `DEFAULT_SYNC_SERVER` (joining an existing project always
  needs a real server). Tests updated in the same three files plus
  `e2e/local-first.spec.ts`, which now asserts the empty default directly
  instead of force-clearing the field — the prior version of that test
  masked exactly this regression.

## Non-goals
- **v1 does not publish an existing local project up to a hub** — that is the adoption follow-on plan (`2026-07-06-hub-client-local-project-adoption.md`), deferred behind D1 (`bd-10bdjmjb`).
- Does **not** change server-side auth enforcement; an auth-required hub still 401s unauthenticated connections.
- Does **not** change how the browser authenticates (that's the Part B plan); "Connect to a hub" triggers whatever browser auth flow exists (GIS today).

## Risks & open questions
- **Backend-less static deploys:** A4 must keep `/auth/me` failure fully non-blocking end-to-end (the hook already tolerates it; verify the render path against a pure static host).
- **Auth-loss teardown:** the disconnect-on-auth-loss effect (`App.tsx:400-409`) must not tear down a local project; scope it to connected projects in A4.
- ~~**"Connect to a hub" placement:** confirm the action's placement in the project selector UX.~~ **Resolved 2026-07-15:** header/account-level control (session-scoped connection state), not a fourth action button — see Design §B. Avoids the name clash with the existing "Connect to Project" (join-by-doc-ID) button.
- **D1 is out of this plan's critical path** — it gates only the adoption follow-on, tracked in that plan.

## Braid strand structure
- **Epic `bd-o3if4hrm`** (epic, p1, open). **v1 sub-strands A0–A4 + A7v1** (parent-child). The **adoption group (A5, A6, A7adopt)** stays under the same epic but is documented in the adoption plan and filed `conditional-blocks` on `bd-10bdjmjb` so it stays unready until D1 lands.
- Related links in place: `bd-10bdjmjb` (D1 — gates adoption only), `bd-3nzyd` (E2E 401 tests), `bd-qxgoti2b` (Epic 2). No hard `blocks` to the other two plans.

## References
### Plans
- `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md` — umbrella / path.
- `claude-notes/plans/2026-07-06-hub-client-local-project-adoption.md` — the adoption follow-on (publish a local project to a hub; deferred behind D1).
- `claude-notes/plans/2026-07-06-hub-client-auth-unification-pkce.md` — Part B (how the browser authenticates on connect).
- `claude-notes/plans/2026-06-12-sync-client-offline-race.md` — D1 durability.
- `claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md` — authorship model.
- `claude-notes/plans/2026-05-20-auth-provider-interface.md` — the `AuthProvider` seam (done).

### Strands
- `bd-10bdjmjb` — offline-fallback race / D1 (adoption follow-on).
- `bd-3nzyd` — E2E preview-iframe 401 tests (test-harness reference).

### Key files
- SPA gate/auth: `hub-client/src/App.tsx` (82, 400-409, 581-596, 85-93, 657-662), `hub-client/src/hooks/{useAuth,useAuthProbe}.ts`, `hub-client/src/services/authService.ts`, `hub-client/src/auth/{AuthProvider,GoogleAuthProvider}.tsx`.
- Storage/sync/actor: `ts-packages/quarto-sync-client/src/client.ts` (475-497, 519, 803-879, 1481-1552, 820-824), `storage-adapter.ts:72`, `hub-client/src/services/projectSetService.ts` (128-129, 191-211), `hub-client/src/services/projectStorage.ts`, `hub-client/src/debug/services/repo.ts:80`.
- Schema/attribution: `ts-packages/quarto-automerge-schema/src/index.ts` (24-27, 58-63, 105-115), `hub-client/src/hooks/useAttribution.ts:69-87`, `hub-client/src/services/attribution-runs.ts:275-276`.
- Server (read-only context): `crates/quarto-hub/src/auth.rs:729-737` (actor HMAC), `crates/quarto-hub/src/server.rs` (788-807 `/auth/actor`, 901-950 WS auth).
