# hub-client: adopt (publish) a local project into a hub

**Status:** proposed · **deferred** — gated on the D1 offline-doc durability fix (`bd-10bdjmjb`).
**Date:** 2026-07-06
**Part of:** the auth-reshape path — see the umbrella index `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`. This is the **fast-follow** to plan 1 (connection-gated local-first, `claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md`). It builds directly on the local-first foundation plan 1 ships (storage-only Repo, persisted local actor, optional `syncServer`).

## Overview

Plan 1 ships local-first documents and a "Connect to a hub" action that
opens/creates **hub-side** projects. This follow-on adds the missing direction:
**publishing an existing *local* project up to a hub** — the forward
actor-switch + display bridge and the sync-up.

This is a separate, deferred plan because it is the only part that hard-depends
on the unsolved offline-doc durability problem (`bd-10bdjmjb`, D1) and carries
all the actor-reconciliation risk. Plan 1's v1 delivers the headline local-first
value with **zero dependency on D1**; adoption lands once D1 does.

### Key reality that shapes this plan
**Automerge changes are immutable.** You can redirect *future* changes to a new
actor (routine — `applyActorId` runs on every `findDoc`), but you can never
re-attribute *past* changes. Pre-connect local edits are authored under a local
actor forever; the only reconciliation is the display-layer `identities` map.
This is why the local→connected actor design below is a *forward switch +
display bridge*, never a rewrite.

## Design: the local→connected actor-ID design

**Pre-connect (local, no auth)** — the state plan 1 v1 already leaves a local
project in (recap; nothing here is new work for this plan):
1. Client mints `indexDocId` (`generateAutomergeUrl()`) — client-side.
2. A **persisted per-browser local actor** is stored in IndexedDB keyed to the local project (plan 1 A3).
3. The Repo is **storage-only** (no network adapter — plan 1 A2).
4. All docs are authored under `localActor` via `applyActorId`; `identities[localActor] = { name, color }` is written so local edits attribute coherently across reloads (plan 1 A3).
5. Project recorded in the local list (IndexedDB), `syncServer` unset (plan 1 A1/A2).

**On auth + connect (adoption — the work in this plan):**
6. Auth completes (GIS today; PKCE after Part B) → cookie set.
7. Client calls `GET /auth/actor?project=<indexDocId>` → `stableActor` (works despite the server never having seen the project — no project validation: `server.rs:788-790`).
8. `applyActorId(handle, stableActor)` on all handles → **future edits carry the trusted, cross-device-stable HMAC actor.**
9. **Bridge the past (display only):** write `identities[stableActor] = { name: userName, color }` *and* upgrade `identities[localActor]` to the same `{ name: userName, color }`. Because `localActor` was persisted (step 2), this is deterministic. Now pre-connect history and post-connect edits both display as the same human. No change is rewritten.
10. Attach a network adapter to the existing Repo and sync `indexDocId` + all file docs up (A5; requires D1 durability, A6).
11. Record `syncServer` on the project now that it is known.

**Rejected alternative — keep `localActor` forever (never switch to HMAC):**
simpler (no actor switch, single author), but loses cross-device actor stability
(each browser has its own local actor) and server-trusted attribution
(client-chosen actor is spoofable; the HMAC actor is derived from a verified
`sub`). The hub's whole authorship model is built on the HMAC actor, so we
switch forward and bridge the display. *(Note: "keep `localActor`" is exactly
plan 1 v1's behavior for a local project until it is adopted — the switch +
bridge is what adoption adds.)*

## Phases (TDD-first) — gated on D1 / `bd-10bdjmjb`

- [ ] **A5 — Connect + adopt local project.** Adoption action on an existing local project: login if needed → `GET /auth/actor?project=<localIndexDocId>` → `applyActorId(stableActor)` on all handles → write/bridge `identities` (steps 8–9 above) → `addNetworkAdapter` → sync up → record `syncServer`. Tests: local project's future edits carry `stableActor`; pre-connect history + post-connect edits display as one human; content reaches the (fake) hub.
- [ ] **A6 — Offline-doc durability (D1).** Harden offline-created file docs so adoption reliably syncs every doc (coordinate with `bd-10bdjmjb` / `2026-06-12-sync-client-offline-race.md`). Tests: create N files offline, connect, assert all N arrive at the hub.
- [ ] **A7adopt — End-to-end adoption verification.** Real browser: create a project offline with N files, adopt it into a running hub, confirm all N sync + one-human authorship in DevTools/UI. Record steps + observed output per the repo's end-to-end policy.

## Non-goals
- Does **not** re-attribute past Automerge changes (impossible); adoption bridges attribution at the display layer only.
- Does **not** change server-side auth enforcement; an auth-required hub still 401s unauthenticated connections.
- Does **not** change how the browser authenticates (that's the Part B plan); the adoption action triggers whatever browser auth flow exists (GIS today).

## Risks & open questions
- **D1 durability (`bd-10bdjmjb`, in progress)** gates this entire plan; it is why adoption is a fast-follow rather than part of plan 1 v1.
- **Local→remote UX:** confirm whether a local project that has been adopted can also be opened read-only when offline after having synced.
- **Actor-reconciliation correctness:** the bridge (step 9) must run before the first post-connect change is observed by other peers, or a collaborator briefly sees the `localActor` history as an unknown author.

## Braid strand structure
- Part of **Epic `bd-o3if4hrm`** (shared with plan 1). The adoption group — **A5, A6, A7adopt** — is filed as a group `conditional-blocks` on `bd-10bdjmjb`, so it stays unready until D1 durability lands.
- Related: `bd-10bdjmjb` (D1 — gates this plan).

## References
### Plans
- `claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md` — plan 1 (the v1 foundation this builds on).
- `claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md` — umbrella / path.
- `claude-notes/plans/2026-07-06-hub-client-auth-unification-pkce.md` — Part B (how the browser authenticates on connect).
- `claude-notes/plans/2026-06-12-sync-client-offline-race.md` — D1 durability.
- `claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md` — authorship model.

### Strands
- `bd-10bdjmjb` — offline-fallback race / D1 (A6).

### Key files
- Storage/sync/actor: `ts-packages/quarto-sync-client/src/client.ts` (475-497 `applyActorId`, 519, 1529 `generateAutomergeUrl`), `hub-client/src/services/projectSetService.ts`, `hub-client/src/services/projectStorage.ts`.
- Schema/attribution: `ts-packages/quarto-automerge-schema/src/index.ts` (24-27, 58-63, 105-115 `setIdentity`), `hub-client/src/hooks/useAttribution.ts:69-87`.
- Server (read-only context): `crates/quarto-hub/src/auth.rs:729-737` (actor HMAC), `crates/quarto-hub/src/server.rs` (788-807 `/auth/actor`, no project validation).
