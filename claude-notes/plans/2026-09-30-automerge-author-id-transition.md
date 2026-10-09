---
title: 'Plan: Transition quarto-hub from stable actor IDs to automerge author IDs'
date: 2026-09-30
description: 'Moves quarto-hub attribution from stable per-user actor IDs to automerge''s author metadata, so actor IDs go back to random per-document values, and history readers fall back to actor IDs for older changes.'
---

Date: 2026-09-30
Status: Approved — in execution. Gordon's go-ahead given 2026-09-30,
lifting the gate bd-6f21d4c6 sets ("No production fix without Gordon's
go-ahead"). Execution epic bd-o1yn1fqy; phase strands bd-c0y8i71s (P0),
bd-kmycto1p (P1), bd-x3b1e0t9 (P2), bd-u3cbfv3g (P3), bd-7jdb6mnp (P4),
bd-r62zad5b (P5), each linked `discovered-from` bd-6f21d4c6.

## Overview

Automerge 3.5.0 (JS, 2026-09-16) / automerge 0.12.0 (Rust, 2026-09-16) shipped a
**change-level author metadata** feature: every change can carry an opaque
hex-string "author" in its metadata, recorded in the document history and
propagated by sync. See [the 3.5.0 release notes](https://github.com/automerge/automerge/releases/tag/js%2Fautomerge-3.5.0),
[`Author` in the Rust API](https://docs.rs/automerge/latest/automerge/struct.Author.html),
and the design-intent note in [This Month in Automerge: July \'26](https://automerge.org/blog/2026-july/)
("author provenance").

Quarto-hub currently solves attribution by giving each user a **stable actor
ID** and reusing it for every change, on every device and tab:

- **Authenticated deployments (OIDC):** the hub mints
  `HMAC-SHA256(server_secret, sub || "\0" || project_id)` and serves it from
  `GET /auth/actor`.
- **Auth-disabled deployments** (local-prod / `--allow-insecure-auth`): the
  client derives one from its IndexedDB `userId` (`actorIdFromUserId`).

This conflates two things automerge now separates:

- **Actor ID** — a CRDT-mechanics identifier. Each actor's changes must be
  strictly sequential. Reusing one actor across concurrent sessions is not a
  theoretical hazard: it is the root cause of the production duplicate-seq
  wedge in bd-6f21d4c6 (`RangeError: duplicate seq N found for actor`), which
  the 2026-09-17 self-heal plan worked around but explicitly left unfixed
  ("needs a change to `actorIdFromUserId` … tracked as future work"). A shared
  actor also makes the sync client's local-vs-remote change classification
  (`client.ts:627`, compares against the doc's own actor) misfile another
  tab's edit by the same user as local.
- **Author ID** — pure attribution metadata. Free-form, stable, per-user,
  carried inside each change, with first-class query APIs.

This plan transitions **all deployments** so that:

1. **Author IDs** carry the stable per-user identity: server-minted HMAC in
   authenticated deployments (same construction as today — the output is
   already a valid author hex string), locally derived from `userId` in
   auth-disabled deployments (same value `actorIdFromUserId` produces today).
2. **Actor IDs** return to the upstream-recommended role: automerge's random
   actor per document instance, never chosen by our code, never shared.
3. All attribution consumers — history readers *and* the "current user"
   checks — resolve `change.author ?? getAuthorForActor(doc, change.actor)
   ?? change.actor` (the middle step covers seq>1 changes, which carry no
   footer — see Phase 0 results), falling back to `change.actor` for
   pre-transition history (which has no author anywhere).

Scope: hub server endpoint, quarto-sync-client, the preview-runtime wrapper,
hub-client (open paths, local branches, attribution UI), and the
preview-renderer current-user plumbing. Server-side writes and quarto-hub-mcp
are audited in Phase 5.

## Current state inventory (re-verified 2026-09-30)

Dependency versions already in the tree — **no dependency bump needed**:

- `@automerge/automerge` 3.5.0 installed (`node_modules`), `^3.5.0` in
  `hub-client/package.json` and `ts-packages/quarto-sync-client/package.json`.
- `@automerge/automerge-repo` 2.6.0-alpha.5 — **no author support in its API**
  (grep of `dist/*.d.ts` finds nothing); author must be applied below the repo
  layer, same pattern as `applyActorId` uses today.
- `automerge = 0.12.0` (with `utf16-indexing`) in
  `crates/quarto-hub/Cargo.toml:48`.

Stable-actor-ID sites:

| Site | File:line | Role |
|---|---|---|
| HMAC minting | `crates/quarto-hub/src/auth.rs:751` (`sub_to_actor_id_for_project`) | Derives 64-hex per-project ID |
| HTTP endpoint | `crates/quarto-hub/src/server.rs:1313` (`auth_actor`), route at `:1758` | `GET /auth/actor?project=` |
| Client fetch | `hub-client/src/services/authService.ts:79-120` (`fetchActorId`, `resolveActorId`) | Three-valued contract: string / undefined (auth off) / null (auth failure) |
| App wiring | `hub-client/src/App.tsx:142,201,310` | `localActorId` state (from `actorIdFromUserId`), `resolveActorId` callback used by the four open paths (`:413, :616, :711, :850`) |
| E2E override | `hub-client/src/App.tsx:77-114` (`__QUARTO_TEST_ACTOR_ID__`) | Read by `hub-client/e2e/q2-preview-render-components-{comment,kanban,drag}.spec.ts` so `getActorId()` is stable and known inside the preview iframe |
| Doc application | `ts-packages/quarto-sync-client/src/client.ts:688-691` (`applyActorId`), `:699-710` (`createDoc`), `:732` (`findDoc`, also covers the self-heal re-fetch) | `automergeClone(doc, { actor })` / `automergeFrom(init, { actor })` |
| Client state | `client.ts:1180` (`connect`), `:2025-2188` (`createNewProject`, via the `resolveActorId` callback parameter), `:2229` (`getActorId`) | `state.actorId` — the single "my actor" the client assumes today |
| preview-runtime wrapper | `ts-packages/preview-runtime/src/automergeSync.ts:166` (`connect`), `:312-317` (`createNewProject`, `resolveActorId`), `:323` (`getActorId`) | Pass-through API used by hub-client and the q2 preview SPA |
| Identity map | `ts-packages/quarto-automerge-schema/src/index.ts:66` (`identities?: Record<string, ActorIdentity>`), written via `setIdentity` at `client.ts:1264, 2094` | actorId → \{name, color\} for attribution UI |
| Local fallback | `hub-client/src/services/userSettings.ts:32-40` (`actorIdFromUserId`) | Auth-disabled only — stable actor from IndexedDB `userId` |
| **Current-user key** | `getActorId()` → `components/render/ReactPreview.tsx:906` (`currentActor`) → `Q2SandboxedPreviewIframe.tsx` → `ts-packages/preview-renderer/src/framework/CurrentActorContext.tsx` (`useCurrentActor()`, `actor === me` checks in user TSX: comments, kanban, drag); `hub-client/src/components/ReplayDrawer.tsx:357,520` (`--me` highlight); `DevHarness.tsx:487` (comment) | How the UI knows which changes are *mine* |
| Replay attribution producer | `hub-client/src/services/attribution-runs.ts:269-285` (`replayChange`) | Replays history, stamps each attribution run with `decodeChange(change).actor` |
| Replay step metadata | `ts-packages/quarto-sync-client/src/replay.ts:75-84`; `ReplayDrawer.tsx` | Per-history-step `{ timestamp, actor }` shown in the replay drawer |
| Attribution wire | `hub-client/src/hooks/useAttribution.ts` builds `{ runs, identities }` keyed by `AttributionRun.actor`; Rust side `TransportAttributionRun.actor` / `identities: HashMap<String, Identity>` (`crates/quarto-core/src/attribution/types.rs:158-177`) via `PreBuiltAttributionProvider` | JS producer chooses the key; Rust only joins on it |
| Local branches | `hub-client/src/services/branchService.ts:133` (`A.load`), `:208` (`A.clone(sourceDoc)`) | Branch docs are edited through the ordinary change path and `A.merge`d back to main |
| Project-set docs | `hub-client/src/services/projectSetService.ts:595` (`automergeFrom(initial)`) | Created without an actor; no attribution UI |
| Duplicate-seq self-heal | `client.ts:740-880` (`installDuplicateSeqRecovery`, `recoverIndexDocument`, bd-6f21d4c6) | Workaround for the collision this plan removes at the source |

Deliberately **not** actor-keyed (no change needed): presence/cursors use
ephemeral `peerId` (`hub-client/src/hooks/usePresence.ts`); the execution
beacon's `actorId: 'exec-1'` is channel-message identity for q2 executor
processes, not document attribution; `hub-client/src/utils/facepile.ts`,
`ProjectsHome.tsx:1100` (contributors) and `Editor.tsx:902` (mention list)
read identity *values* (name/color) only, never keys.

Not writers: `quarto-preview` and `quarto-hub-provider` link automerge but
never `transact`. Hub server writes live in `index.rs` and `sync.rs` (Phase 5).
`quarto-hub-mcp` connects with `actorId` undefined
(`connection-manager.ts:299`), so bot edits already carry a random actor and
no identity.

## Verified author-ID API surface

JS (`@automerge/automerge` 3.5.0, from installed `dist/*.d.ts` and
`dist/mjs/implementation.js`):

- `InitOptions { actor?, author?, ... }` — accepted by `init`/`load`/`from`/
  `clone`, but **only `init` and `clone` honor `author`**
  (`handle.setAuthor(opts.author)` at `implementation.js:107, 161`); `from`
  delegates to `init`, so it inherits the handling. **`load` drops `author`
  silently** unless a `patchCallback` is also passed (that path routes through
  `init(opts)`); the plain path is `ApiHandler.load(data, { actor, ... })`
  with no `setAuthor` call. After a plain `load`, apply the author via
  `clone(loaded, { author })` or the escape hatch below.
- `clone` is `handle.fork(opts.actor, heads)` followed by `setAuthor`; with
  `actor` omitted, `fork` generates a fresh random actor **per call**.
- `getAuthor(doc): Author | null` — the doc's currently-set author.
- `getAuthors(doc): Author[]` — all authors appearing in history.
- `getAuthorForActor(doc, actor): Author | null`,
  `getActorsForAuthor(doc, author): ActorId[]` — mapping derived from history.
- `DecodedChange.author` / `ChangeMetadata.author`: declared `Author | null`;
  **runtime delivers `undefined` when absent** (pinned in Phase 0) — `??`
  resolution handles both. Absent for all pre-3.5 changes, which is what
  makes this a purely additive migration (no document rewrite).
- **The footer is per-actor, not per-change** (Phase 0's biggest finding):
  `transaction_args` attaches the author only when `seq == 1`
  (automerge.rs:568), and `set_author` mints a fresh random actor whenever
  the author value changes (automerge.rs:389-396). Each actor carries the
  author on exactly its first change; later changes by that actor resolve
  via the actor→author index (`getAuthorForActor`), rebuilt from history on
  load (change_graph.rs:908-911).
- `Author = string`, an opaque **hex** string.
- **Wire encoding (identical in JS and Rust — same Rust core):** the author
  is a footer in the change's existing `extra_bytes` field
  (`0x01 | leb128(len) | author bytes`, `change.rs:367-403`), not a new
  column. Old decoders store and relay `extra_bytes` uninterpreted, so
  pre-3.5 / pre-0.12 clients decode and re-sync author-stamped changes
  without error. Two corollaries: a change carries an author **or** user
  `extraBytes`, never both (`Transaction::extra_bytes()` returns only the
  author footer) — nothing in the tree writes `extraBytes`, so this costs us
  nothing; and our pre-transition changes all have empty `extra_bytes`, hence
  decode as `author: null` with no risk of a legacy footer misparsing as an
  author.
- Escape hatch: `getBackend(doc).setAuthor(author)` (`wasm_types.d.ts:458`).
- automerge-repo paths that (re)create a doc do so **without options**:
  `Repo.import` → `Automerge.load(binary)` (`Repo.js:500`); storage load →
  `A.loadIncremental(A.init(), binary)` (`storage/StorageSubsystem.js:139`). So the
  author, like the actor today, must be applied after the handle is ready —
  via `getBackend(handle.docSync()).setAuthor(author)` (the escape hatch
  above; D9), with `handle.update(doc => clone(doc, { author }))` as the
  documented fallback. Sync receive
  (`A.receiveSyncMessage`) and `handle.change` (`A.change`) operate on the
  same backend and preserve it.
- **Read paths need no automerge-repo changes**: `DocHandle.metadata()` is a
  wholesale `A.inspectChange()` passthrough (`DocHandle.js:292`), so `author`
  is present at runtime — only the narrow local `ViewableHandle` types in
  `attribution-runs.ts` and `replay.ts` need widening.

Rust (`automerge` 0.12.0, from the registry source):

- `Author<'a>(Cow<'a, [u8]>)` — `From<Vec<u8>>`/`&[u8]`, `FromStr` (hex
  decode → `InvalidAuthor`), `to_hex_string()`, serde impls.
- `LoadOptions::author(Author<'static>)` (`automerge.rs:166`);
  `Automerge::with_author` / `set_author` / `get_author` / `get_authors` /
  `get_author_for_actor` (`automerge.rs:376-414`); `AutoCommit::with_author` /
  `set_author` / `get_author` (`autocommit.rs:401-429`); `Change::author()`
  (`change.rs:48`).

## Target design

```mermaid
flowchart LR
    subgraph hub [hub server - auth enabled]
        A[/GET auth author?project=/] -->|HMAC-SHA256 secret sub project_id| B[author_id 64-hex]
    end
    subgraph local [hub-client - auth disabled]
        L[userId in IndexedDB] -->|authorIdFromUserId| B2[author_id hex]
    end
    subgraph client [hub-client per document]
        B --> C[resolveAuthorId]
        B2 --> C
        C --> D[set author on doc backend]
        E[automerge random actor per document] --> D
        D --> F[changes carry author + unique actor]
    end
    F --> G[attribution UI: key author or actor fallback; me = getAuthorId]
```

- Server mints the author ID with the **same HMAC construction** as today
  (same pseudonymity properties: per-project isolation, server-secret binding).
  Authors are free-form bytes rendered as hex, and the HMAC output is already
  64-hex — valid with no re-encoding. The minted value is therefore
  **byte-for-byte identical** to the actor ID that user would previously have
  received for the project; only its role changes (served from `/auth/author`,
  applied via `{ author }` instead of `{ actor }`).
- Auth-disabled deployments derive the author locally with the same
  derivation `actorIdFromUserId` uses today (renamed `authorIdFromUserId`), so
  the local author equals the stable actor that user had.
- No code chooses actors anymore. Each document instance gets automerge's
  default random actor (from `fork`/`init`). This removes the root cause of
  bd-6f21d4c6 in every deployment mode.
- `identities` in the IndexDocument becomes keyed by **author ID**; readers
  resolve `change.author ?? getAuthorForActor(doc, change.actor) ??
  change.actor` so both seq>1 post-transition changes and pre-transition
  history keep their attribution. Because the author ID equals the old actor
  ID, a user's identity-map key **does not change across the transition**:
  legacy changes (fallback to `change.actor`) and new changes (`change.author`
  or the actor→author index) resolve to the same key, so attribution is
  continuous with no aliasing or re-keying migration.
- The **current-user key** is `getAuthorId()`, which replaces `getActorId()`
  everywhere "me" is computed. Downstream prop and field names
  (`currentActor`, `CurrentActorContext`, `data-attr-actor`,
  `AttributionRun.actor`, `TransportAttributionRun.actor`) keep their names and
  are documented as carrying the *attribution key* (author, or actor for
  legacy changes).

## Decisions

- **D1 — Actor model.** Automerge default: random per document instance.
  Per-tab or persisted per-device actors are rejected — a per-device actor
  is shared by every tab on that device, which is exactly the bd-6f21d4c6
  collision.
- **D2 — Auth-disabled parity.** In scope. Auth-disabled deployments move to
  random actor + locally derived author. The production collision was
  observed in this mode; leaving it on a stable actor would preserve the
  known root cause.
- **D3 — Endpoint shape.** New `GET /auth/author`. `/auth/actor` stays,
  deprecated, and is removed in a follow-up strand once no released client
  calls it.
- **D4 — identities keying.** Re-key by author with actor fallback (additive,
  same map). No parallel `authorIdentities` map.
- **D5 — Derivation continuity.** `author_id == legacy actor_id`. Gives
  continuous attribution keys and a clean version-skew story: an old client
  emitting the stable actor and a new client emitting the same value as
  author resolve to one identity with no aliasing. A domain-separated HMAC
  would cost two identity entries per user plus aliasing logic in readers.
- **D6 — Wire and prop naming.** Keep the `actor` field/prop names in
  `AttributionRun`, `TransportAttributionRun`, `data-attr-actor`,
  `currentActor`. Document them as "attribution key". No end-to-end rename;
  the Rust side needs no code change.
- **D7 — Explicit-actor plumbing.** Removed rather than kept alongside. The
  `actorId` parameters of `connect`/`createNewProject` become `authorId`,
  `state.actorId` becomes `state.authorId`, `getActorId()` becomes
  `getAuthorId()`, and the e2e override becomes `__QUARTO_TEST_AUTHOR_ID__`.
- **D8 — Authorless where there is no author.** Writes that do not originate
  from a signed-in or locally identified user carry no author: hub-server
  writes (files map, capture sidecar, filesystem import), project-set
  documents, and any client path where `resolveAuthorId` yields `undefined`.
  No synthetic "hub" or "system" author. Readers already handle
  `author: null` via the actor fallback, so these show up exactly as legacy
  changes do.
- **D9 — Author application mechanism.** Set the author in place via
  `getBackend(doc).setAuthor(author)` on the repo handle's doc, not via
  `handle.update(doc => clone(doc, { author }))`. Author is runtime-only
  backend state (never serialized), so in-place mutation is semantically
  correct and avoids the clone's side effects: a full-document fork per
  `findDoc`, a re-randomized actor on every call (one random actor per
  document instance is D1's model, not one per find), a spurious
  `applyMutation` notification through the repo, and `DocHandle.update`\'s
  fixed-heads precondition. The backend is shared by construction:
  `fork`/`clone` are the only ways to get a second backend (`view` reuses
  the same handle), and on the paths our handles flow through — find,
  `Repo.import`, storage load, sync receive — the repo never forks (its
  internal clones are confined to `Repo.clone` and a `DocHandle` diff
  helper, which our handles never pass through). The mutation therefore
  sticks for every later `handle.change`.
  `getBackend` is a `/** @hidden */` export (stable in practice since
  pre-3.x); if it is ever removed the failure is loud at doc-open and caught
  by the Phase 0 spike and CI, and the clone form is the documented one-line
  fallback. `createDoc` is unaffected: `automergeFrom(init, { author })` is
  fully public API either way.

## Checklist

### Phase 0 — integration spike

The option-plumbing questions (does `clone` propagate author, does
`Repo.import` drop it) are answered by the source reading above. The spike
guards the one thing reading cannot: that `A.change` through the repo's
handle stamps the author set in place on the shared backend via the
`getBackend` escape hatch (D9) — and that the documented clone fallback
stamps identically.

- [x] File the braid epic + per-phase strands for execution tracking (each
  linked `discovered-from` bd-6f21d4c6, referencing this plan) and comment
  the plan link onto bd-6f21d4c6. Done up front, not in Phase 5, so the
  strand's close trail shows the whole remediation path.
  → Filed 2026-09-30: epic bd-o1yn1fqy; phases bd-c0y8i71s (P0),
  bd-kmycto1p (P1), bd-x3b1e0t9 (P2), bd-u3cbfv3g (P3), bd-7jdb6mnp (P4),
  bd-r62zad5b (P5), chained `blocks` in phase order; plan link commented
  onto bd-6f21d4c6.
- [x] One JS test in `ts-packages/quarto-sync-client`: import a doc via
  automerge-repo, apply `getBackend(handle.doc()).setAuthor(author)`
  (D9), then `handle.change(...)`; assert the new change's `author` equals
  `author` (via `decodeChange`) and `getAuthorForActor(doc, getActorId(doc))
  === author`. Assert the documented clone fallback stamps identically on a
  second handle (`handle.update(doc => clone(doc, { author }))`). Same test
  asserts a doc round-tripped through `Repo.import` *without* applying an
  author yields `author: null` (legacy simulation), and pins the **`A.load`
  author drop**: `A.load(bytes, { author })` followed by `A.change` yields
  `author: null`, while `clone(A.load(bytes), { author })` then `A.change`
  stamps correctly. Record `save()` length before/after (measures the
  on-disk cost of the author footer after the format's chunk compression).
  → `src/author-id-spike.test.ts`, 7 tests green.
- [x] Rust unit test in `crates/quarto-hub/src/auth.rs` tests:
  `Author::from_str(&sub_to_actor_id_for_project(...))` is `Ok`.
  → `auth::tests::actor_id_for_project_is_a_valid_automerge_author`, green.
- [x] Record results in this file.

### Phase 0 results (recorded 2026-09-30)

Spike test: `ts-packages/quarto-sync-client/src/author-id-spike.test.ts`
(7 tests, green; full package suite 159/159). Findings that amend the
design — the spike did its job:

1. **Author footers are per-actor, not per-change** — the plan's biggest
   correction. `transaction_args` attaches the author only when `seq == 1`
   (automerge.rs:568), and `set_author` mints a fresh random actor whenever
   the author value changes (automerge.rs:389-396; "If you are using
   authors *never* manually manage the ActorId"). Each actor carries the
   author on exactly its first change; later changes by that actor are
   attributed via the actor→author index (`getAuthorForActor`), rebuilt
   from history on load (change_graph.rs:908-911, with an asserted
   `seq() == 1` invariant on footer-bearing changes). Consequences:
   - Reader-side resolution (Phase 4) is
     `change.author ?? getAuthorForActor(doc, change.actor) ?? change.actor`.
     The original two-step `change.author ?? change.actor` would have
     misattributed every seq>1 change to its random actor.
   - On-disk cost is ~34 bytes **per actor** (i.e. per document session),
     not per change: measured `save()` 1140 → 1179 bytes (+39) for 51
     changes with an author vs without, with exactly one footer-bearing
     change confirmed via `getAllChanges`. The "Document size" paragraph
     in Compatibility is amended accordingly.
   - D1 is reinforced: `set_author` itself re-randomizes the actor when
     the value changes, and re-applying the *same* author is a no-op
     (pinned: actor unchanged), so `findDoc`\'s idempotent re-application
     is safe.
2. **Absence surfaces as `undefined`, not `null`** in JS:
   `DecodedChange.author` and `getAuthorForActor` return `undefined` for
   authorless changes/actors despite the `.d.ts` declaring `Author | null`.
   `??`-based resolution handles both; the spike pins `undefined`.
3. **`handle.docSync()` does not exist** in automerge-repo 2.6.0-alpha.5;
   the synchronous accessor is `handle.doc()`. D9's escape hatch is
   therefore `getBackend(handle.doc()!).setAuthor(author)`.
4. **D9 mechanism confirmed**: author set in place on the repo handle's
   backend stamps the next `handle.change` (seq-1 of the fresh actor),
   persists as `getAuthor`, survives idempotent re-application, and the
   clone fallback stamps identically. `Repo.import` without an author
   stays authorless (legacy simulation). The actor→author mapping survives
   a `save`/`load` round trip; the current-author *setting* does not —
   reloads must re-apply the author, which matches the createDoc/findDoc
   call sites.
5. **`A.load(bytes, { author })` silently drops the author** (pinned);
   `clone(A.load(bytes), { author })` stamps correctly — this is why
   branch loading in Phase 3 uses the `clone` form.

Rust side: `auth::tests::actor_id_for_project_is_a_valid_automerge_author`
confirms the HMAC output parses as `automerge::Author` (D5).

Local test-suite note: three sync-client suites failed on a clean tree
with `normalizeProjectPath is not a function` — stale
`ts-packages/quarto-automerge-schema/dist/` (predates that export), fixed
by `npm run build -w ts-packages/quarto-automerge-schema`. Pre-existing,
unrelated to this work.

### Phase 1 — hub server: mint author IDs

- [x] Tests first (`crates/quarto-hub/src/server.rs` / auth tests, following
  `claude-notes/instructions/testing.md`): `GET /auth/author?project=` returns
  deterministic 64-hex equal to `/auth/actor`\'s value for the same
  credential+project (D5), distinct per project, 401 unauthenticated / 403
  disallowed, 400 on missing `project`, works on both session-cookie and
  Bearer credential paths.
  → `session_auth.rs`: `auth_author_works_with_session_cookie`,
  `auth_author_supports_bearer`, `auth_author_requires_authentication`,
  `auth_author_rejects_missing_project`, `auth_author_refuses_banned_sub`;
  red (404) before the route existed, green after.
- [x] Add `auth_author` handler + `AuthAuthorResponse { author_id }` in
  `crates/quarto-hub/src/server.rs`, route `GET /auth/author`, calling the
  existing `sub_to_actor_id_for_project` (D5: byte-identical; no second HMAC
  body to drift). Update that function's doc comment to describe the
  identity role. Rename it when `/auth/actor` is removed (Phase 5 strand).
- [x] Keep `GET /auth/actor` untouched for older clients; mark deprecated in a
  doc comment.
- [x] `cargo nextest run -p quarto-hub` green (480/480);
  `cargo xtask verify --skip-hub-build` green except four pre-existing
  environmental failures: `typst/margin-layout/margin-table-{gt-r,flextable}[-crossref].qmd`
  need the R packages `gt` / `flextable`, not installed in this machine's
  R library — unrelated to this phase (hub HTTP surface), same on main.

### Phase 2 — quarto-sync-client and preview-runtime: apply author, drop actor

- [x] Tests first: `connect` with an `authorId` produces changes that
  resolve to that author (`change.author` on each actor's seq-1 change,
  `getAuthorForActor` thereafter — per the Phase 0 findings); two documents
  opened by the same client have different actors; the same document opened
  by two client instances has different actors (the literal bd-6f21d4c6
  scenario — two tabs of one user must not share an actor); `createDoc`
  stamps the author from the first change; a re-found document (the
  `findDoc` path) has the author applied.
  → `src/author-id.test.ts` (5 tests): all red before the change
  (author assertions undefined; actors shared 1-across-3 docs; the
  two-tab convergence test failed after 15 s — the literal production
  wedge), all green after.
- [x] Replace `applyActorId` with `applyAuthorId(handle, authorId)`
  (`client.ts:~688`): `getBackend(handle.doc()!).setAuthor(authorId)` —
  author is runtime-only backend state, so set it in place rather than
  forking the document (D9; `docSync()` does not exist in automerge-repo
  2.6.0-alpha.5, `doc()` is the synchronous accessor — Phase 0 finding 3).
  Call sites: `createDoc` (`:703`, after the `repo.import` that would
  otherwise drop the author) and `findDoc` (`:732`, also covers the
  self-heal re-fetch). Keep the clone form
  (`handle.update(doc => automergeClone(doc, { author }))`) in a code comment
  as the documented fallback if the `@hidden` `getBackend` hatch is ever
  removed (D9).
- [x] `createDoc` (`client.ts:~699`): `automergeFrom(init, { author })`.
- [x] Rename per D7: `state.actorId` → `state.authorId`; `connect(...,
  actorId, ...)` → `authorId`; `createNewProject(..., actorId, ...,
  resolveActorId)` → `authorId`, `resolveAuthorId`; `getActorId()` →
  `getAuthorId()`. No caller supplies actors; automerge generates them.
- [x] `setIdentity` call sites (`client.ts:1264, 2094`): key by author ID.
- [x] Local/remote change classification (`client.ts:627`) unchanged — it
  compares against the doc's own actor, which is now correct across tabs.
- [x] Collision-repro tests (bd-6f21d4c6's evidence base):
  `actor-id-collision.test.ts` and `full-stack-actor-collision.test.ts` in
  `ts-packages/quarto-sync-client/src/` manufacture the wedge by forcing two
  clients to share an actor — a setup that no longer exists once `connect`
  stops accepting one. Rewrite them as convergence proofs (two same-author
  clients converge; no duplicate-seq error; self-heal never fires) or delete
  them with the reason recorded in the commit message.
  → `full-stack-actor-collision.test.ts` deleted: its wedge setup is
  inexpressible once `connect`/`createNewProject` take no actor; the
  same-author convergence proof (no duplicate-seq, self-heal never
  fires, both tabs converge, both edits resolve to the same author) is
  the last test in `author-id.test.ts`. `actor-id-collision.test.ts`
  kept with a reframed header: bare-automerge hazard documentation for
  why no code may manage actor IDs (D1).
- [x] Keep `installDuplicateSeqRecovery` as a safety net while legacy clients
  still emit stable actors during the skew window; the removal is a Phase 5
  follow-up strand.
- [x] preview-runtime wrapper (`ts-packages/preview-runtime/src/automergeSync.ts`):
  `connect`, `createNewProject`, `getActorId` → author equivalents.
  q2-preview-spa needs no change: it imports only `connect`/`disconnect`
  from `@quarto/preview-runtime`, passes no actor, and stays authorless per
  D8.
- [x] `npm run test -w ts-packages/quarto-sync-client` and
  `npm run test -w ts-packages/preview-runtime` green (neither package has a
  `test:ci` script; also run preview-runtime's `test:integration`/`test:wasm`
  suites if the wrapper's behavior changed).
  → sync-client 163/163 (25 files), preview-runtime 79/79. The wrapper
  change is a pass-through rename only (no behavior change), and both
  extra suites are pre-existing vacuous: `test:integration` matches zero
  files; `test:wasm` names a config file that does not exist (documented
  in ts-test-suite.yml:298-301). Both `tsc --noEmit` clean.
  Mock-based suites needed `getBackend` stubs added to their wholesale
  `@automerge/automerge` mocks (client.ts now imports it for D9).
  Note: one order-dependent flake observed — `doc-inventory.test.ts`
  failed once in a full-suite run, green standalone and on two full-suite
  re-runs; unrelated to author handling (its projects are authorless).

### Phase 3 — hub-client: fetch and wire author IDs

- [x] Tests first (mirror existing authService tests): `fetchAuthorId` parses
  `author_id`, returns null on 401/403, throws on 500, and on **404 falls back
  to `/auth/actor`** (new client against an old server; by D5 the value is
  identical); `resolveAuthorId` preserves the three-valued contract
  (string / undefined / null + logout side-effect).
  → `authService.test.ts` (7 new `fetchAuthorId` + 9 `resolveAuthorId`
  tests, incl. the 404→`/auth/actor` fallback and fallback-401→null pins),
  `userSettings.test.ts` (rename), `branchService.test.ts` (3 new
  author-attribution tests: seq-1 footer + seq-2 actor→author index
  resolution after merge-back, the clone-on-load path after a simulated
  reload, and the authorless D8 case). All red before implementation
  (missing exports/seam), green after. One test-authoring fix along the
  way: `A.getHistory` returns decoded changes, so attribution assertions
  go through `A.getAllChanges` + `A.decodeChange` (same as the spike).
- [x] `hub-client/src/services/authService.ts`: add `fetchAuthorId(projectId)`
  and `resolveAuthorId(...)`. `fetchActorId` survives only as the 404 fallback.
- [x] `hub-client/src/services/userSettings.ts`: rename `actorIdFromUserId` →
  `authorIdFromUserId` (same derivation; update the doc comment, which
  currently explains the stable-actor rationale).
- [x] `hub-client/src/App.tsx`: `localActorId` → `localAuthorId` (`:142, :310`);
  `resolveAuthorId` in all four open paths (`:413, :616, :711, :850`);
  `__QUARTO_TEST_ACTOR_ID__` → `__QUARTO_TEST_AUTHOR_ID__` (`:77-114`).
- [x] E2E specs `q2-preview-render-components-{comment,kanban,drag}.spec.ts`:
  inject `__QUARTO_TEST_AUTHOR_ID__`; the value no longer needs to be a valid
  actor id but stays hex.
- [x] Current-user key: `render/ReactPreview.tsx:906` `currentActor={getAuthorId()}`;
  `ReplayDrawer.tsx:357` `currentActorId = getAuthorId()`; update the
  `DevHarness.tsx:487` comment. Prop names unchanged (D6); doc comments in
  `render/ReactRenderer.tsx:109`, `CurrentActorContext.tsx`, `PreviewRoot.tsx:150`,
  `entry.tsx:213` say the value is the attribution key.
  → Also updated the `getActorId` → `getAuthorId` mocks in
  `ReplayDrawer.test.tsx` and the three `ReactPreview.*.integration.test.tsx`
  files (missed by the plan's inventory).
- [x] Local branches: `branchService.ts:208` `A.clone(sourceDoc, { author })`
  and `:133` `clone(A.load(bytes), { author })` — **not** `A.load(bytes,
  { author })`, which silently drops the author (see "Verified author-ID API
  surface") — with `author = getAuthorId()`, so branch edits carry the author
  and merge back attributed (today they merge back as an unrelated random
  actor once actors are random).
  → Applied via an `authorGetter` seam mirroring `handleGetter`
  (`_setAuthorGetterForTesting` for tests); null/absent author passes no
  option (D8 authorless).
- [x] `projectSetService.ts:595`: authorless (D8; no attribution UI on
  project-set docs); say so in a code comment.
- [x] `npm run build:all` from `hub-client/` green (stricter than vitest per
  AGENTS.md); hub-client changelog updated with the two-commit workflow.
  → Full unit suite 1260/1260 (107 files), integration 143/143 (20 files),
  `build:all` green, preview-renderer `tsc --noEmit` clean.

### Phase 4 — attribution consumers: author-first resolution

- [x] Tests first: attribution/history display resolves an author-keyed
  identity for new changes and an actor-keyed identity for legacy
  (`author: null`) changes in the same document.
  → `attribution-runs.test.ts` (3 new: continuous key across a real
  legacy+two-session doc incl. payload single-identity assertion;
  incremental path resolving seq-2 via the state-carried map — red without
  it; `maybeMergeAt` coalescing across sessions at equal timestamps),
  `replay.test.ts` (4 new: author preference, seq>1 index fallback,
  legacy bare-actor fallback — stayed green throughout, cross-boundary
  continuity), `ReplayDrawer.test.tsx` (1 mixed-history UI continuity
  test — green from the start since the drawer only sees resolved keys;
  the load-bearing red pins live in the two producer suites). All red
  before implementation where the layer owns resolution, green after.
- [x] `ts-packages/quarto-automerge-schema/src/index.ts:66`: document
  `identities` as keyed by author ID (legacy actor keys remain readable);
  schema comment only, no version bump or type change.
- [x] `replayChange` (`attribution-runs.ts:276`): attribution key
  `decoded.author ?? authorForActor(decoded.actor) ?? decoded.actor`, where
  `authorForActor` is the actor→author map accumulated from seq-1 footers
  during replay (Phase 0 finding 1: seq>1 changes carry no footer).
  Doc comments on `CharAttribution.actor`
  and `AttributionRun.actor`: attribution key. Widen the
  `ViewableHandle.metadata` return type in `attribution-runs.ts:91` and
  `replay.ts:36` to include `author?: string | null`.
  → The map is carried on `RunListAttribution._authorByActor` so the
  incremental path resolves actors whose footer was replayed in an earlier
  build/update (the common case: one in-memory actor, many seq>1 changes).
- [x] `replay.ts` `getMetadataAt`: `actor: meta?.author ?? authorForActor(meta?.actor) ?? meta?.actor ?? null`.
  Rename the local `ChangeMetadata` type (it collides with automerge's
  exported `ChangeMetadata`), e.g. `ReplayStepMetadata`.
  → Renamed to `ReplayStepMetadata` (export in sync-client `index.ts`
  updated; no other consumers). `authorForActor` here is automerge's own
  `getAuthorForActor` against the session clone (full history, lazy,
  exact) — the accumulated-map variant belongs to the attribution-runs
  hot loop, which has no live doc to query.
- [x] `useAttribution.ts` `buildIdentityMap`: logic unchanged (keys already
  come from `runs`); update the comment.
- [x] Rust: no code change (D6). Doc comment on `TransportAttributionRun.actor`
  (`types.rs:161`) describing the key.
- [x] Mixed-history regression test (anchor: `ReplayDrawer.test.tsx`): a
  document containing both pre-transition changes (`author: null`, stable
  actor) and post-transition changes (author set, random actor) by the same
  user shows one continuous attribution — same identity entry, same palette
  color — across the boundary; same-author runs from different sessions
  coalesce in `maybeMergeAt`; the `--me` highlight matches both legacy and
  new steps for the current user.
  → Split across the three suites above: run-list continuity + single
  identity entry + coalescing in `attribution-runs.test.ts`, per-step key
  continuity in `replay.test.ts`, waveform single-color band + `--me`
  chip in `ReplayDrawer.test.tsx`.
- [x] hub-client changelog (second commit) if any hub-client file changed.
  → Full unit 1264/1264 (107 files), integration 143/143, sync-client
  167/167 (25 files), quarto-core attribution tests 81/81 + `cargo fmt
  --check` clean (doc-comment-only Rust change), `npm run build:all`
  green, ts-packages dist rebuilt after the type rename.

### Phase 5 — audits, deprecation, end-to-end verification

- [x] Hub-server-authored changes: `index.rs` (`transact` at `:118, :172,
  :191, :271, :306` — files map and capture sidecar) and `sync.rs` (`:164,
  :378` — filesystem import/update; the other grep hits are test-module
  helpers). Stay authorless
  (D8): no code change; add a test asserting a server-written change decodes
  with `author: null` so a future `LoadOptions::author` is a deliberate act.
  → `index::tests::server_writes_carry_no_author` (create + add/remove file
  + set/remove capture, ≥5 changes all authorless) and
  `sync::tests::server_sync_writes_carry_no_author` (text fork/merge +
  binary content write), both green; committed 064bf4353.
- [x] `ts-packages/quarto-hub-mcp`: fetch the author via the Bearer path
  (`/auth/author`, 404 fallback `/auth/actor`) and pass it as `authorId`;
  update the `PEER_TIMEOUT_MS` comment at `connection-manager.ts:197`. File a
  strand if the change is non-trivial.
  → Filed and closed bd-5y0han3a; committed 064bf4353. Best-effort failure
  semantics (stderr warning + authorless connect, D8-compatible) — the WS
  handshake stays the real auth gate. `connect` passes the fetched value as
  `authorId`; `createProject` wires a `resolveAuthorId` callback (the index
  doc id is generated inside the sync client). 5 new connection-manager
  tests + 8 existing ones updated for the extra per-connect fetch; package
  suite 256/256, `tsc` clean. Confirmed against the REAL endpoint: the
  package's `e2e-auth.test.ts` (real hub + mock IdP, Bearer) passes with the
  fetch in the connect path.
- [x] File follow-up strands: remove `GET /auth/actor` and the `fetchActorId`
  fallback; remove `installDuplicateSeqRecovery`; rename
  `sub_to_actor_id_for_project` — all gated on no released client emitting
  stable actors. Give the self-heal-removal strand an **evaluable closure
  criterion**, not just the gate: e.g. all clients older than a defined
  release no longer supported, or zero duplicate-seq recoveries observed in
  hub logs over a defined window.
  → bd-8oyidg7h (/auth/actor + both client fallbacks), bd-lh7e13o5
  (self-heal removal; closure criterion: all pre-transition releases
  unsupported AND zero recoveries in hub logs over a trailing 90-day
  window), bd-x9v6sgza (rename). All `discovered-from` bd-r62zad5b.
- [x] Disposition H1 and H2 — bd-6f21d4c6's other confirmed defects; this
  plan removes H4 only. File one strand per defect, linked
  `discovered-from` bd-6f21d4c6, as upstream-contribution candidates or
  explicitly accepted-as-latent per Gordon's call.
  → bd-bks7az35 (H1, carrying the open tier-2 real-socket confirmation
  question), bd-w7x5ajsv (H2).
- [x] E2E: two distinct authenticated users edit concurrently against a hub
  running with auth on — plain `npm run local-prod` is auth-off, so stand up
  the mock OIDC provider from the integration suite (`MockOidcProvider`,
  `crates/quarto-hub/tests/integration/support.rs`) the way
  `scripts/hub-sliding-sessions-e2e.mjs` does; confirm each change is
  attributed to the right author, actors are unique per document, and own
  edits highlight as "me" in both the replay drawer and the preview. Repeat
  in auth-disabled mode with two browser profiles. Run bd-6f21d4c6's literal H4 scenario as a negative test: one
  user in two browser tabs making simultaneous index-doc edits — no
  duplicate-seq error, both tabs converge, self-heal never fires. Open an
  older (pre-transition) document and confirm legacy attribution still
  renders via actor fallback. **Record the exact
  invocation and observed output here per the end-to-end verification policy.**
  → New Playwright suite `hub-client/e2e-author/` +
  `playwright.author-e2e.config.ts` (own globalSetup: mock OIDC IdP à la
  `hub-sliding-sessions-e2e.mjs`, one auth-on hub, one auth-disabled hub,
  two static+proxy origins via `scripts/local-prod-server.mjs`; session
  cookies minted through `POST /auth/session`). Wired into CI
  (`hub-client-e2e.yml`) and as `npm run test:e2e:author-id`. Four
  scenarios, all green twice in a row (19–20 s each run):
  1. **auth-on two users** — alice + bob edit `main.qmd` concurrently.
     Observer client decodes the file doc's changes: keys resolve to
     exactly the two server-minted authors; three distinct random actors
     (creation client + two browsers); no actor equals an author;
     identities map keyed `\{authorA, authorB\}`. Authors overlay in BOTH
     browsers keys each user's text by their author (per-word spans,
     aggregated per key in assertions). `__COMMENT_DIAG__.me` in the
     preview iframe reads authorA for alice, authorB for bob. Replay
     drawer: bob's latest step shows his key without `--me` for alice and
     with `--me` for bob; stepping to alice's steps flips it (assertions
     on the chip's `data-actor-key` / `data-current-actor` attributes,
     added to `ReplayDrawer.tsx`).
  2. **H4 negative** — alice in two tabs of one context, simultaneous
     edits: both tabs converge with both texts, zero console matches for
     `duplicate seq|recovered index document`, and the file doc shows ONE
     author (authorA) under ≥3 distinct actors (creation + tab1 + tab2).
  3. **auth-disabled parity** — two browser profiles: local authors equal
     `authorIdFromUserId(userId)` read from each profile's IndexedDB,
     distinct random actors, authorless creation change decodes
     `author: null` and attributes via bare-actor fallback (D8); overlay
     keyed correctly.
  4. **legacy continuity** — a pre-transition project crafted with raw
     automerge (`A.from(..., \{actor: aliceAuthorForProject\})`, two changes,
     `author: null` pinned) uploaded via `repo.import` with chosen doc
     IDs: overlay renders legacy text under the bare actor (= alice's
     author, D5), alice's NEW edit lands on the SAME key, the page shows
     exactly one `data-attr-actor` value, and the replay drawer shows
     `--me` from the newest step back to step 1.
  Invocation: `cd hub-client && npm run test:e2e:author-id` (full build)
  or `npx playwright test --config playwright.author-e2e.config.ts` (dist
  already built). Observed output inspected: `4 passed (19.4s)` with the
  per-scenario assertion flow above; the protocol-level change table was
  eyeballed during development (browser seq-1 changes carry the author
  footer; seq>1 resolve via the actor→author index — matching the Phase 0
  spike's model).
  **E2E surfacing worth noting:** the SPA's `AUTH_ENABLED` is a
  build-time flag (`VITE_GOOGLE_CLIENT_ID`); the e2e build doesn't set
  it, so the suite forces it via a new `VITE_E2E`-gated window override
  (`__QUARTO_TEST_AUTH_ENABLED__`, App.tsx, same pattern as
  `__QUARTO_TEST_AUTHOR_ID__`). Without it the app silently falls back to
  local authors even against an auth-on hub — the suite's first red run
  caught exactly that (browsers stamping userId-derived authors).
  Browser-stamped authors match `/auth/author?project=<bare indexDocId>`
  byte-for-byte, which also pins hub-mcp↔hub-client author consistency
  (both pass the bare id).
- [x] `cargo xtask verify` (full — hub-client and WASM legs affected) green.
  → All 14 steps passed 2026-09-30 (lints incl. CSS, Rust workspace build +
  nextest 15332 passed, ts-packages builds + MCP smoke, hub-client
  build:all + test:ci 1264 unit + 143 integration + 153 wasm, q2-preview-spa
  build). The smoke_all typst fixtures needed the R packages ``flextable``
  and ``gt`` installed in the session library (environmental, installed
  0.10.1 / 1.3.0).
- [ ] Close bd-6f21d4c6: record the outcome of the Carlos capture plan
  (forced H4 repro / IndexedDB export) or Gordon's waiver of the real
  specimen, then close the strand with a reason referencing the execution
  epic (filed in Phase 0), the H1/H2 disposition strands, and the E2E
  evidence above.

## Compatibility and migration

- **No document migration.** Author is additive change metadata; existing
  documents sync unchanged. Pre-transition changes have `author: null` and
  remain attributable via the actor → identity map, which Phase 4 keeps
  readable.
- **Client/server version skew.** New server keeps `/auth/actor`, so old
  clients work unchanged (stable actor, `author: null`). New client against
  an old server: `/auth/author` 404 → fall back to `/auth/actor` and apply
  the (identical, D5) value as the author — fully functional. A user running
  an old client on one device and a new client on another still resolves to
  one identity key. The old client's stable actor can still collide with
  itself across its own tabs (pre-existing), which is why the self-heal stays
  until the skew window closes.
- **Already-wedged documents are out of scope.** The transition stops new
  duplicate-seq wedges; it does not unwedge documents whose persisted
  history is already poisoned — `installDuplicateSeqRecovery` owns that
  (including Carlos's document, if still live), a second reason the
  self-heal stays until the skew window closes.
- **Wire-level forward compatibility.** Old clients tolerate *new* changes at
  the wire level, not just the API level: the author footer rides in
  `extra_bytes`, which pre-3.5 / pre-0.12 decoders store and relay
  uninterpreted. Released q2 preview SPAs, old quarto-hub-mcp bundles, and
  stale hub-client tabs therefore decode and re-sync author-stamped changes
  without error — they simply don't surface authors.
- **Trust model unchanged (say so honestly).** Author IDs, like the minted
  actor IDs today, are server-minted but client-applied and unsigned — the hub
  does not validate change actors on sync, and it will not validate authors
  either. Attribution is by convention in both designs. Server-side validation
  of `change.author` at the sync endpoint is possible future hardening (the hub
  terminates sync and can decode changes); out of scope here, worth a strand.
- **Document size.** The author footer costs ~34 bytes **per actor**
  (`0x01 | leb128(32) | 32 bytes` in `extra_bytes` on the actor's seq-1
  change only — Phase 0 finding 1), i.e. once per document session, not per
  change. Measured in the spike: `save()` 1140 → 1179 bytes (+39) for 51
  changes with an author versus without. The deduplicated `Authors` index
  is in-memory only, rebuilt from history on load.

## References

- [Automerge 3.5.0 release notes](https://github.com/automerge/automerge/releases/tag/js%2Fautomerge-3.5.0) — the feature announcement
- [Rust `Author` docs](https://docs.rs/automerge/latest/automerge/struct.Author.html) / [`LoadOptions::author`](https://docs.rs/automerge/latest/automerge/struct.LoadOptions.html)
- [This Month in Automerge: July \'26](https://automerge.org/blog/2026-july/) — author provenance design intent (Keyhive revocation context)
- `claude-notes/plans/2026-09-17-index-doc-duplicate-seq-self-heal.md` (bd-6f21d4c6) — the collision this plan removes at the source
- Current implementation: inventory table above
