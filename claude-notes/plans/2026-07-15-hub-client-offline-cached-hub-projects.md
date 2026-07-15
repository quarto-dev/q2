# hub-client: offline read + write of cached hub projects

**Status:** proposed · **deferred/coordinated** — shares the actor-bridge with
the adoption follow-on and is gated on the same D1 offline-doc durability work
(`bd-10bdjmjb`).
**Date:** 2026-07-15
**Part of:** the auth-reshape path — umbrella
`claude-notes/plans/2026-07-06-connection-gated-auth-and-auth-unification.md`.
Fast-follow to plan 1 (connection-gated local-first,
`2026-07-06-hub-client-connection-gated-local-first.md`) and a sibling of the
adoption follow-on (`2026-07-06-hub-client-local-project-adoption.md`), with
which it shares the actor-ID reconciliation design.

## Overview

Plan 1 shipped: local-first documents, gate removal, and a "Connect to a hub"
control. In v1, selecting a **hub** project (one with a `syncServer`) while
logged off / offline first failed silently, then — as an interim fix — prompted
sign-in (`resolveActorForOpen` → `onNeedsSignIn`, commit `82b19375`).

This plan replaces that interim behavior with the real feature: **a cached hub
project opens offline and is fully editable; edits sync to the hub the moment
the user reconnects (signs in / comes online), with coherent one-human
authorship across the whole online→offline→online timeline.**

### Why this is not the rejected alternative

The adoption plan (`…-local-project-adoption.md`, "Rejected alternative", lines
45–52) rejected *"keep the local actor forever, never switch to HMAC"* for hub
projects — because a client-chosen actor is spoofable and loses cross-device
stability. **This plan does not do that.** It uses the persisted per-browser
local actor *only during offline windows* and performs the adoption-style
**forward switch to the HMAC actor + `identities` display-bridge on every
reconnect**. The local actor is transient authorship that is always reconciled
forward; the hub's HMAC-actor authorship model is preserved. In other words:
**this is the adoption forward-switch+bridge (adoption steps 8–9) generalized to
fire on each online transition, not just once at first publish.**

## Current architecture we build on (all shipped in plan 1)

- **Storage-only vs networked Repo** via `buildRepo({ syncServer? })`
  (`ts-packages/quarto-sync-client/src/client.ts`). A hub project builds a
  networked Repo; when the WS upgrade 401s (logged off) or times out (offline),
  `connect` already **degrades to offline-from-IndexedDB** (loads cached docs,
  `isOnline=false`). The only thing blocking an offline hub open today is the
  client-side `resolveActorId` → `null` gate, not the sync layer.
- **Persisted per-browser local actor** `getOrCreateLocalActor()`
  (`hub-client/src/services/userSettings.ts`, plan 1 A3).
- **`applyActorId(handle, actorId)`** switches future authorship on any handle,
  history-preserving (`client.ts:475`), already called on every `findDoc`.
- **`GET /auth/actor?project=<id>`** returns the stable HMAC actor for any
  project id with no project validation (`server.rs:788-807`) — works for a
  project the server has seen (unlike adoption's never-seen case).
- **Authorship display** via `IndexDocument.identities[actorId] = {name,color}`
  (`quarto-automerge-schema`, `setIdentity`), fallback `actor.slice(0,8)`
  (`useAttribution.ts`). "Me" = the Automerge actor.
- **Server accepts changes from any authenticated peer** — `AuditAccessPolicy.
  is_allowed` is peer/cookie-based and always returns true for an authenticated
  peer (`crates/quarto-hub/src/access_policy.rs`); it does **not** validate that
  incoming changes carry the HMAC actor. **Confirmed:** offline edits authored
  under the local actor will NOT be rejected on sync — the actor only affects
  authorship display, not acceptance.

## Design: local↔HMAC actor oscillation with a persistent display-bridge

For a hub project (`syncServer` set):

**Opening offline / logged off (no session):**
1. Do NOT abandon at the actor step. Resolve the actor as: HMAC `stableActor`
   if a session yields one; otherwise fall back to the persisted **local
   actor** (`getOrCreateLocalActor()`).
2. `connect(syncServer, indexDocId, <resolvedActor>, …)` — the WS 401s/offline
   and the sync client degrades to offline-from-cache (existing behavior). The
   project opens read+write from IndexedDB.
3. Offline edits author under the local actor; ensure `identities[localActor] =
   {name,color}` is written so they display as this human immediately.

**On reconnect (sign-in / online) for that project:**
4. `GET /auth/actor?project=<indexDocId>` → `stableActor`.
5. `applyActorId(stableActor)` on all handles → future edits carry the HMAC
   actor again.
6. **Bridge the offline window (display only):** `identities[stableActor] =
   identities[localActor] = {name: userName, color}`. Offline history + online
   edits now display as one human. No change is rewritten (changes are
   immutable — see the adoption plan).
7. The network adapter (re)connects and the offline edits sync up via normal
   automerge sync.

**Steady online state:** unchanged from today — HMAC actor, live sync.

This is the adoption `local→connected` design (steps 6–11 there) applied to an
already-hub project across a connectivity gap, run **every** time connectivity
returns rather than once.

## Phases (TDD-first) — coordinate with D1 (`bd-10bdjmjb`)

- [ ] **B0 — Test scaffolding.** Fixtures: a cached hub project (created online
  against a fake/real hub, then the peer dropped) reopened offline; an offline
  edit; a reconnect that flushes the edit up; authorship assertions across the
  timeline. Extend the `openActor` seam. Write the failing tests for B1–B3.
- [ ] **B1 — Open cached hub project offline (supersede prompt-sign-in).**
  `resolveActorForOpen` (and the `openActor` helper): for a hub project with no
  resolvable HMAC actor, fall back to the local actor and open from cache
  instead of firing `onNeedsSignIn`. Keep a genuine "never-cached + offline"
  case surfacing a clear "can't open — not cached, and you're offline" message
  (that one really can't open). Tests: cached hub doc opens offline read+write;
  never-cached hub doc offline reports the precise reason.
- [ ] **B2 — Offline authorship under the local actor.** Author offline edits
  under the local actor; write `identities[localActor]`. Tests: offline edits
  attribute to this human (not an 8-hex stub) and persist across reload.
- [ ] **B3 — Reconnect switch + display bridge + sync-up.** On sign-in/online
  for the project: fetch `stableActor`, `applyActorId`, bridge `identities`
  (steps 4–7), (re)attach/reactivate the network adapter, flush offline edits.
  Reuse the adoption A5 mechanics. Tests: future edits carry `stableActor`;
  offline + online history display as one human; offline edits reach the (fake)
  hub after reconnect.
- [ ] **B4 — Durability of offline edits (D1 coordination).** Ensure offline
  edits to *existing cached* docs reliably sync on reconnect. Determine whether
  this shares D1's announce-on-connect fix (`bd-10bdjmjb`) or is already covered
  by normal automerge sync for existing docs. Tests: edit N cached docs offline,
  reconnect, assert all N updates reach the hub.
- [ ] **B5 — End-to-end verification + docs.** Real browser: open a hub project
  online, go offline (or sign out), edit, reconnect/sign-in, confirm the edits
  reach the hub and authorship reads as one human in the UI/DevTools. Record
  exact steps + observed output per the repo end-to-end policy. Update
  `hub-client/LOCAL-FIRST.md` (the table currently says sync/collab are
  hub-only; add the offline-cached-hub-project row).

## Interaction with the shipped v1

- **Supersedes** the interim prompt-sign-in behavior for *cached* hub projects
  (commit `82b19375`). Prompt-sign-in remains correct for a hub project that is
  NOT cached locally and cannot be opened offline. The `openActor` seam and its
  unit tests are the extension point — B1 turns the "hub + null actor" branch
  from "prompt" into "open-from-cache-under-local-actor when cached".

## Non-goals

- Does **not** re-attribute past Automerge changes (impossible) — attribution is
  bridged at the display layer only (shared with adoption).
- Does **not** change server-side auth/acceptance. Access stays cookie/peer-based
  (`access_policy.rs`); an auth-required hub still 401s the live WS, which is
  exactly what triggers offline-from-cache.
- Does **not** change how the browser authenticates (Part B / PKCE plan).

## Risks & open questions

- **D1 gating (`bd-10bdjmjb`, in progress):** confirm whether syncing offline
  edits to *existing* cached docs needs the D1 announce-on-connect fix or is
  already covered by normal automerge sync (creation was the D1 case). This
  determines whether B can land ahead of D1.
- **Spoofable offline actor:** offline edits under a client-chosen local actor
  are display-bridged, not server-vouched — the same trust model plan 1 already
  accepts for local projects. Server acceptance is unaffected (peer/cookie-based).
  Document it; do not pretend offline edits carry server-trusted attribution.
- **Actor-bridge timing (from adoption):** the bridge (step 6) must run before
  the first post-reconnect change is observed by collaborators, or a peer
  briefly sees the local-actor history as an unknown author.
- **Stale-cache / large divergence UX:** long offline edits vs. concurrent
  collaborator edits merge via CRDT; consider surfacing "syncing offline changes"
  and conflict-heavy states. Likely a follow-up, not v1 of this plan.
- **Multiple browsers offline:** each browser has its own local actor; each is
  bridged on that browser's reconnect. Authorship stays one-human per browser;
  cross-browser it relies on the HMAC actor once online (unchanged).

## Braid strand structure

- Epic **`bd-xxjy9yfp`** (this plan), `related` to Epic `bd-o3if4hrm`
  (connection-gated v1) and `bd-10bdjmjb` (D1). Phase strands (parent-child):
  - `bd-ysusqcb3` — B0 (test scaffolding)
  - `bd-qklxdkwh` — B1 (open cached hub offline; supersede prompt-sign-in) — blocked by B0
  - `bd-ab44wv07` — B2 (offline authorship under local actor) — blocked by B1
  - `bd-g5apu5bm` — B3 (reconnect switch + bridge + sync-up) — blocked by B2, `related` to `bd-10bdjmjb`
  - `bd-7drrqapw` — B4 (durability / D1 coordination) — blocked by B3, `related` to `bd-10bdjmjb`
  - `bd-qe84xjd6` — B5 (E2E + docs) — blocked by B4
- B3/B4 are `related` (not hard-blocked) to D1 because **B4's job is to
  determine** whether D1's announce-on-connect fix is required; escalate to
  `conditional-blocks` if B4 finds it is.

## References
### Plans
- `2026-07-06-hub-client-connection-gated-local-first.md` — plan 1 (foundation + the interim prompt-sign-in fix this supersedes).
- `2026-07-06-hub-client-local-project-adoption.md` — adoption follow-on (shared actor-bridge design; its open question line 67 is this feature's seed).
- `2026-06-12-sync-client-offline-race.md` — D1 durability.
- `2026-05-25-reactji-authorship-q2-preview.md` — authorship model.

### Key files
- `hub-client/src/services/openActor.ts` (+ `.test.ts`) — the actor-open seam (B1).
- `hub-client/src/services/userSettings.ts` — `getOrCreateLocalActor` (B2).
- `ts-packages/quarto-sync-client/src/client.ts` — `buildRepo`, `connect` offline degrade, `applyActorId`, `flush` (B1–B4).
- `ts-packages/quarto-automerge-schema/src/index.ts` — `setIdentity`/`identities` bridge (B2–B3).
- `hub-client/src/App.tsx` — open paths + reconnect wiring (B1, B3).
- `crates/quarto-hub/src/access_policy.rs` — confirms peer/cookie-based acceptance (no actor gate).
