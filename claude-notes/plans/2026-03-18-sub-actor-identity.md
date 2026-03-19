# OIDC `sub` as Automerge Actor Identity

## Overview

When auth is enabled in quarto-hub, we use the OIDC `sub` (subject identifier) claim directly as the Automerge document actor ID. The `sub` is a persistent, unique, opaque string assigned by the identity provider -- it contains no PII and is safe to embed permanently in document history. This eliminates the need for salted hashing, salt management, and privacy deletion of identity data.

A separate optional "display document" maps actor IDs back to human-readable info (email, name) for the replay UI. This document is a pure UI enhancement -- everything works without it, and deleting it only affects display.

### Why `sub` instead of email-based hashing

A previous plan used `hash(salt + email)` as the actor ID, requiring a per-project salt, an actors document to store the salt and email mapping, and a privacy deletion mechanism. The OIDC `sub` claim makes all of this unnecessary:

- **`sub` is already opaque** -- no hashing or salt needed
- **`sub` is already persistent** -- same value across sessions, devices, reconnections
- **`sub` is already unique** per provider -- no collision concerns within a single-provider deployment
- **`sub` contains no PII** -- safe to embed permanently in Automerge document history
- **No privacy deletion needed for identity** -- the actor IDs in file history are meaningless strings

### Current State

- **Server**: Documents created with `Automerge::new()` in `context.rs` -- random actor IDs
- **Client**: `new Repo({ network: [wsAdapter] })` in `ts-packages/quarto-sync-client/src/client.ts` -- random actor IDs per document
- **Auth flow**: `OidcClaims` struct (`auth.rs:104-112`) already parses `sub`, `email`, `email_verified`, `name`, `picture` from the JWT
- **`/auth/me`**: Returns `{ email, name, picture }` but NOT `sub` or actor ID (server.rs:535-541)
- **Replay**: `useReplayMode.ts` displays actor IDs as short hashes with deterministic colors via `actorColor()`
- **PeerId->email map**: Server maintains `peer_emails: HashMap<PeerId, String>` for audit logging

### Verified Approach: `handle.update()` + `Automerge.clone()`

Tested and confirmed that `handle.update(doc => Automerge.clone(doc, { actor: hexActor }))` works on both `repo.create()` and `repo.find()` paths. After the update, all subsequent `handle.change()` calls attribute changes to the new actor. This sidesteps the fact that `Repo.create()` doesn't expose an actor parameter.

**Why clone is safe (no sync side-effects):** `Automerge.clone()` preserves identical heads — same history, same changes, only the actor differs. Inside `DocHandle`, the xstate subscription calls `#checkForChanges(before, after)` after every `update()`. Since `getHeads(before) === getHeads(after)`, `docChanged` is `false` — no `"heads-changed"` or `"change"` events fire, no sync is triggered. The old `Doc<T>` WASM handle is dereferenced by xstate's `assign({ doc })` and freed via `FinalizationRegistry` on GC.

**Why clone is wasteful:** The Rust `automerge` crate has `doc.set_actor()` — a single field assignment, no allocation. But the WASM bindings don't expose it, so `Automerge.clone()` copies the entire internal structure just to change one field. For this project's documents (small text files, often empty at creation time), the cost is negligible in practice. See "Upstream Improvements" for the fix.

Note: The initial `repo.create()` writes one change with a random actor ID before `update+clone` switches to the `sub`-derived actor. This random actor persists in document history as noise -- it's not a privacy concern (it's random, not identity-derived) but is worth a code comment for future maintainers.

### Actor ID Format

The `sub` string is SHA-256 hashed to produce the Automerge actor ID. This gives uniform 32-byte (64 hex char) actor IDs regardless of provider, and ensures true opacity (some providers like Auth0 include identifying prefixes in the `sub`).

In the Automerge JS API, actor IDs are hex strings representing bytes, so:

```
sub: "114946389732038281927"
SHA-256: [0xa3, 0x1f, ...]  (32 bytes)
hex actor ID: "a31f..."       (64 hex chars, always)
```

All providers produce the same fixed-length actor ID:
- Google: `"114946389732038281927"` -> 64 hex chars
- Azure AD: `"AAAAABBBBBcccccc"` -> 64 hex chars
- Auth0: `"auth0|5f7c8ec7c33c6c004bbafe82"` -> 64 hex chars

**Rust side (single source of truth):** `ActorId::from(Sha256::digest(sub.as_bytes()).as_slice())` -- using the `sha2` crate (already a transitive dependency via automerge). The server computes the hex actor ID and returns it in `/auth/me`, so the client never needs to hash anything.

## Work Items

### Phase 1: Core identity (`sub` as actor ID)

Compute `actorId` server-side, expose it via `/auth/me`, store it in auth state, add `createDoc`/`findDoc` helpers, and wire `actorId` through the connect flow.

**Server -- compute and expose `actorId`:**

- [x] Add a `sub_to_actor_id(sub: &str) -> String` helper (SHA-256 hash, hex-encoded, 64 chars) — using the `sha2` crate (already a transitive dep via automerge)
- [x] Add `actor_id` field to `AuthMeResponse` in `server.rs` (line 535-541): computed from `claims.sub` via the helper
- [x] Add tests for `sub_to_actor_id`: ASCII sub (Google numeric), mixed-case sub (Azure), sub with special chars (Auth0 `auth0|...`), determinism (same sub = same output), uniform length (always 64 hex chars regardless of input)

**Auth service -- store `actorId` from server response:**

- [x] Add `actorId: string | null` to `AuthState` interface in `authService.ts` (null when no auth)
- [x] In the auth fetch flow, store `actorId = response.actor_id` from the `/auth/me` response into `AuthState`
- [x] Update `useAuth.test.ts` mock data to include `actor_id` in the `/auth/me` response and verify `actorId` is set on `AuthState`

**`createDoc`/`findDoc` helpers in `ts-packages/quarto-sync-client/src/client.ts`:**

Instead of sprinkling `applyActorId()` at every `repo.create()`/`repo.find()` call site, we replace direct `state.repo` calls with two helper functions that encapsulate the actor ID logic. All document creation and lookup goes through these helpers, so the actor ID is applied in exactly two places.

```ts
function applyActorId<T>(handle: DocHandle<T>, actorId: string | null): void {
  if (!actorId) return;
  handle.update(doc => Automerge.clone(doc, { actor: actorId }));
}

function createDoc<T>(): DocHandle<T> {
  const handle = state.repo!.create<T>();
  applyActorId(handle, state.actorId);
  return handle;
}

async function findDoc<T>(docId: DocumentId): Promise<DocHandle<T>> {
  const handle = await state.repo!.find<T>(docId);
  await handle.whenReady();
  applyActorId(handle, state.actorId);
  return handle;
}
```

Note: There are two subscribe helpers in `client.ts`:
- `subscribeToFile` (line 141) — calls `handle.whenReady()`, used by `repo.find()` paths
- `subscribeToFileInternal` (line 197) — no `whenReady()`, used by `repo.create()` paths (handle already ready)

Since `findDoc` already handles `whenReady()`, `subscribeToFile` drops its `whenReady()` call and becomes a pure subscription setup (like `subscribeToFileInternal`).

- [x] Add `actorId: string | null` to `SyncClientState` (null when no auth)
- [x] Create `applyActorId`, `createDoc`, and `findDoc` helpers as shown above
- [x] In `connect()` and `createNewProject()`: store `state.actorId = actorId ?? null`
- [x] Replace all 5 `state.repo.create()` calls with `createDoc()`:
  - `createFile` text handle (line 407)
  - `createBinaryFile` binary handle (line 459)
  - `createNewProject` index handle (line 577)
  - `createNewProject` binary file handle (line 592)
  - `createNewProject` text file handle (line 609)
- [x] Replace all 3 `state.repo.find()` calls with `findDoc()`:
  - `loadFileDocuments` (line 229)
  - `syncWithFiles` (line 245)
  - `connect` index handle (line 281)
- [x] Remove `await handle.whenReady()` from `subscribeToFile` (line 142) — `findDoc` already handled it. This makes `subscribeToFile` consistent with `subscribeToFileInternal`.
- [x] Remove the standalone `await indexHandle.whenReady()` (line 284) in `connect()` — `findDoc` already handled it.

**Wiring `actorId` through the connect flow:**

The server pre-computes `actorId` and the auth service stores it, so `App.tsx` just passes `auth.actorId` down through the wrapper layer.

Call chain: `App.tsx` → `hub-client/src/services/automergeSync.ts` (thin wrappers) → `ts-packages/quarto-sync-client/src/client.ts` (core logic).

- [x] Add `actorId?: string` parameter to `connect()` and `createNewProject()` wrappers in `hub-client/src/services/automergeSync.ts`, pass through to the core client's `connect()` / `createNewProject()`
- [x] Add `actorId?: string` parameter to `connect()` and `createNewProject()` in `ts-packages/quarto-sync-client/src/client.ts`
- [x] In `hub-client/src/App.tsx`, pass `auth.actorId` to `automergeSync.connect()` (line 31) and `automergeSync.createNewProject()` (line 354) at existing call sites

**Replay -- resolve own actor ID to "Me":**

When entering replay mode, the current user's `actorId` (from `AuthState`) should be matched against the actor IDs in the document history. When the current user's hex actor ID matches a history actor, display "Me" instead of the raw hex value. This makes replay immediately legible -- you can tell which changes are yours vs others'.

The `useReplayMode` hook (`hub-client/src/hooks/useReplayMode.ts`) currently takes only `filePath` as a parameter. Actor metadata comes from `session.getMetadataAt(index).actor` via `@quarto/quarto-sync-client`. The "Me" resolution is a display concern in the rendering layer (`ReplayDrawer.tsx`), not in the hook itself.

- [x] Pass the current user's `actorId` (from auth state) to `ReplayDrawer` (or the component that renders actor labels)
- [x] When rendering actor labels in replay, compare each actor ID against the current user's `actorId` -- if they match, display "Me" instead of the short hex hash
- [x] Preserve the deterministic color for "Me" (same color as the hex hash would have had)

**Tests:**
- [ ] `createDoc`: returned handle has the correct actor ID (verify via `Automerge.getActorId(handle.docSync())`)
- [ ] `createDoc`: when `actorId` is null, handle retains its original random actor ID
- [ ] `findDoc`: returned handle has the correct actor ID after `whenReady()`
- [ ] Same `sub` produces same actor ID across `createDoc` and `findDoc` calls
- [ ] End-to-end: `connect()` with `actorId` results in changes attributed to that actor
- [x] Replay: current user's actor ID displays as "Me" instead of hex hash
- [x] Replay: other actors still display as short hex hashes
- [x] Replay: "Me" retains the same deterministic color as its underlying actor ID

## Deferred: Display document (email/name lookup for replay)

Deferred to a separate plan. The display document maps opaque actor IDs to human-readable info (email, name) for the replay UI. Before designing this, we need to decide whether the mapping should be:

- **A synced Automerge document** (current sketch) -- lives alongside project docs, synced to all clients. Simple but means all collaborators see each other's emails, and the mapping persists in client-side IndexedDB.
- **Server-side storage** (e.g., database or config) -- only the server holds the mapping. Clients request it on-demand via a REST endpoint during replay. Better privacy but adds a new API surface and server-side state.
- **Hybrid** -- server stores the mapping, embeds it in the WebSocket handshake or a dedicated message, clients hold it only in memory (not persisted).

Phase 1 is fully functional without this. Replay shows hex actor IDs with deterministic colors (same as today with random IDs), but now they're consistent per user.

## Appendix: Deferred -- Server-side actor identity

Deferred because most server-created documents are either project initialization (no specific user) or internal operations. The main value (attribution in history) comes from client-side changes. The user-attributable cases (`resource.rs:118` `create_binary_document` for file uploads, `sync.rs:683` for new docs during sync) would require threading `OidcClaims.sub` through the request handler call chain.

- [ ] In server-side document creation, when the operation is attributable to a specific user, use `Automerge::new().with_actor(ActorId::from(hex::decode(sub_to_actor_id(&sub)).unwrap()))` (reusing the same `sub_to_actor_id` helper from Phase 1)
- [ ] For server-only operations (e.g., project init from disk via `context.rs:422`, index creation via `index.rs:56`), keep using random actors

## Appendix: Provider scoping

The OIDC `sub` is unique only within a single identity provider. Two users at different providers (Google vs Azure) could theoretically share the same `sub` value. This is not a concern today because:

- Each hub instance is configured with a single OIDC provider (`--oidc-client-id`)
- Multi-provider support is not on the roadmap

If multi-provider support is added later, hash `"iss\0sub"` (null-separated) instead of just `"sub"`. This produces distinct actor IDs even if two providers issue the same `sub`. The display document already stores email/name independently of the encoding scheme, so no other changes needed.

## Design Decisions

1. **Why SHA-256 hash instead of direct byte encoding?** Hashing gives uniform 32-byte actor IDs regardless of provider (instead of 32-70+ hex chars). It also ensures true opacity -- some providers like Auth0 include identifying prefixes (`auth0|...`) in the `sub` that would leak through direct encoding. SHA-256 is collision-resistant enough to be effectively unique, and the original `sub` is always available server-side in the JWT for debugging. The hash is computed server-side only (`sha2` crate, already a transitive dep via automerge), so the client has no crypto dependency.

2. **Same `sub` = same actor within a project.** Intentional -- a user's changes are consistently attributed to them across reconnections, devices, and browser refreshes.

3. **`handle.update()` + `clone()` over alternatives.** We verified that `peerId` is NOT used as the document actor (separate concepts). Patching automerge-repo internals is fragile. The `update`+`clone` approach uses only public APIs and works on both created and found documents.

4. **`createDoc`/`findDoc` helpers over scattered `applyActorId` calls.** Replacing raw `state.repo.create()`/`find()` calls with two helpers centralizes the actor ID logic in 2 functions instead of 7 insertion points. `findDoc` atomically handles `find()` + `whenReady()` + actor assignment, eliminating timing races. New code naturally uses the helpers — a missed helper call is more obvious than a missed post-call. No Proxy, no monkey-patching.

5. **No-auth fallback: random actors.** When auth is disabled, `sub` is unavailable. `applyActorId` is a no-op and Automerge uses its default random actor IDs. No special handling needed.

## Upstream Improvements

### 1. Expose `set_actor` in automerge WASM bindings (`automerge/automerge`)

The Rust `automerge` crate has `doc.set_actor(ActorId)` — a single field assignment with no allocation or copying. But the WASM bindings (`automerge_wasm_bg.js`) don't expose it. The only way to change the actor from JS is `Automerge.clone(doc, { actor })`, which copies the entire document.

Adding `setActor(actor: string)` to the WASM `Automerge` class is a one-line binding to the existing Rust method. The JS layer would then expose it as `Automerge.setActorId(doc, actorId)` or similar. This would let us replace:

```ts
// Current: clone entire document to change one field
handle.update(doc => Automerge.clone(doc, { actor: actorId }))

// With set_actor exposed: mutate in place, no allocation
handle.update(doc => { Automerge.setActorId(doc, actorId); return doc })
```

**Priority: low.** The clone is safe (no sync side-effects, old doc is GC'd) and cheap for our document sizes. But it's the right fix long-term.

### 2. Add `actor` option to `Repo.create()` / `Repo.find()` (`automerge/automerge-repo`)

Ideally, `Repo.create()` and `Repo.find()` would natively accept an `actor` option:

```ts
repo.create<T>({ actor: hexActorId })
repo.find<T>(docId, { actor: hexActorId })
```

This would eliminate the `update+clone` workaround and all its downsides:

- **Noise in history**: `create()` writes one change with a random actor before the clone switches to the real one. That random actor is permanently baked into the document.
- **Performance**: `Automerge.clone()` copies the entire document just to change the actor ID.

Migration would be straightforward — update the `createDoc`/`findDoc` helpers to pass the native `actor` option and remove `applyActorId`.
