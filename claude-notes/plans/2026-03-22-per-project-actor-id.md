# Per-Project Actor ID via Server-Secret HMAC

## Overview

The current `actor_id = SHA256(sub)` design creates a **globally stable, server-independent
user tracking token**. Any party with read access to multiple Hub projects can observe the
same `actor_id` across projects, correlate a user's presence and edit history across unrelated
documents, and — combined with document content — infer their identity.

### Fix

Replace `SHA256(sub)` with `HMAC-SHA256(server_secret, sub || "\0" || project_id)`:

- **Per-project isolation**: Same user gets a different `actor_id` in every project.
  Cross-project correlation via actor_id is impossible.
- **Server-secret binding**: The actor_id cannot be computed outside the server even if
  an attacker knows both the `sub` and `project_id`.
- **Per-session consistency**: Within a single project, the same user gets the same
  `actor_id` across sessions/devices/reconnections (needed for Automerge attribution).

### Migration note

Existing Automerge documents contain change history attributed to the old `SHA256(sub)`
actor IDs. After migration, the same user will appear as a new actor in existing documents.
This is an accepted trade-off for the privacy improvement.

---

## Work Items

### Phase 1 — Server Secret Persistence

- [x] Add `hmac = "0.12"` and move `rand = "0.9"` from dev-deps to regular deps in
  `crates/quarto-hub/Cargo.toml` (`sha2 = "0.10"` is already present)
- [x] Add `.quarto/hub/hub.json` to the repository root `.gitignore` to prevent
  accidental secret commits (hub directories are created inside project roots)
- [x] Upgrade `HubStorageConfig::save()` to enforce `0o600` permissions on Unix by
  opening the file with the restricted mode **before** writing — this avoids the TOCTOU
  window that exists when `fs::write` is followed by a separate `set_permissions` call
- [x] Write tests for server secret resolution:
  - `QUARTO_HUB_SERVER_SECRET` env var set → used directly, no file I/O
  - `QUARTO_HUB_SERVER_SECRET` set to invalid hex → returns error
  - No env var, new config → generates a 32-byte hex secret, saves, returns it
  - No env var, loaded config → returns the same secret across calls (no regeneration)
  - No env var, old config without `server_secret` field → generates, saves, returns new secret
- [x] Add `server_secret: Option<String>` field to `HubStorageConfig`
- [x] Add `resolve_server_secret(config: &mut HubStorageConfig, hub_dir: &Path) -> Result<[u8; 32]>`
- [x] Call `resolve_server_secret` from `StorageManager::init`; store as `server_secret: [u8; 32]`
  with `server_secret(&self) -> &[u8]` accessor
- [x] Add `server_secret_bytes(&self) -> &[u8]` method to `HubContext`

### Phase 2 — HMAC Actor ID Function

- [x] Write tests for `sub_to_actor_id_for_project` in `auth.rs`
- [x] Implement `sub_to_actor_id_for_project(server_secret: &[u8], sub: &str, project_id: &str) -> String`
- [x] Remove old `sub_to_actor_id` function and its tests
  - Note: the plan's separator injection test was incorrect (both inputs evaluate to the
    same message when `\0` is embedded); replaced with a test showing that the separator
    prevents ambiguity in VALID inputs (which cannot contain `\0`).

### Phase 3 — REST Endpoint: GET /auth/actor

- [x] Add `AuthActorQuery` struct (`project: String`) for query param extraction
- [x] Add `AuthActorResponse` struct (`actor_id: String`)
- [x] Add `auth_actor` async handler
- [x] Register route: `.route("/auth/actor", get(auth_actor))`
- [x] Remove `actor_id` field from `AuthMeResponse` and update `auth_me` handler accordingly
- Note: integration test for the endpoint is covered by unit-level tests in auth.rs
  and authService.test.ts; an HTTP-level test would require a running server and is
  deferred to end-to-end testing.

### Phase 4 — TypeScript Client

- [x] Write tests for `fetchActorId` in `authService.test.ts`
- [x] Update all `actorId: 'abc123'` mock objects in `useAuth.test.ts`
- [x] Remove `actorId: string` from `AuthState` interface in `authService.ts`
- [x] Remove `actor_id: string` from `AuthMeResponse` interface
- [x] Remove `actorId: data.actor_id` mapping in `fetchAuthMe`
- [x] Add `fetchActorId(projectId: string): Promise<string | null>` to `authService.ts`
- [x] Add `actorId: string | undefined` to `App` component state; clear on disconnect
- [x] Update all call sites in `App.tsx` that pass `auth?.actorId` (route change,
  shareable link, URL load, project selector, createNewProject)
- [x] Update `<Editor actorId={...}>` to use the new `actorId` state variable
- [x] Remove `auth?.actorId` from `useCallback` dependency arrays; add `logout` where needed

---

## Design Details

### Secret resolution order

1. **`QUARTO_HUB_SERVER_SECRET` env var** (highest priority): 64-char lowercase hex string. Use this for
   containers, secret managers (Vault, AWS Secrets Manager, etc.), and CI. When set, `hub.json`
   is not read or written for the secret.
2. **`hub.json` config file**: auto-generated on first run, persisted for subsequent restarts.
3. **Auto-generate and save**: if neither is present, generate 32 random bytes, write to `hub.json`.

Do not expose the secret via CLI argument — arguments appear in `ps aux` and shell history.

### Secret format

- 32 random bytes generated using `rand::RngCore::fill_bytes`
- Stored as lowercase hex string (64 chars) in `hub.json`
- `hub.json` permissions set to `0o600` (owner read/write only) on every save on Unix by
  opening the file with `OpenOptions::mode(0o600)` **before** writing, avoiding the TOCTOU
  window that a post-write `set_permissions` call would create. On Windows, gitignore is
  the primary protection.
- `hub.json` must be in `.gitignore` to prevent accidental secret commits
- Decoded to `[u8; 32]` once at startup by `resolve_server_secret`; stored opaquely in
  `StorageManager` — not re-decoded on each request

### `save()` and file permissions

`resolve_server_secret` calls `config.save(hub_dir)` (not a separate write). The `save()`
method is upgraded to open the file via `OpenOptions::mode(0o600)` on Unix (`#[cfg(unix)]`)
so the file is never visible with permissive permissions. This ensures:
- All `hub.json` writes (version bumps, index doc ID updates, secret addition) always have
  secure permissions on Unix with no TOCTOU window
- Non-Unix builds compile cleanly without `OpenOptionsExt`
- Note: existing `hub.json` files from prior versions retain their old permissions until next
  overwrite; a one-time `set_permissions` call at startup can correct these if desired

### HMAC message format

```
HMAC-SHA256(key=server_secret, message="{sub}\0{project_id}")
```

A null byte (`\0`) is used as the separator. It cannot appear in JWT `sub` claims (which are
JSON strings, and null bytes are not valid in JSON string values) or in Automerge IDs
(`automerge:<bs58>`, where bs58 is printable ASCII). This prevents separator injection attacks
where different `(sub, project_id)` pairs could produce identical messages (e.g., with `:` as
separator, `sub="user:automerge"` + `project_id="abc"` collides with `sub="user"` +
`project_id="automerge:abc"`).

### Project ID

The `project_id` passed to `GET /auth/actor?project=<id>` is the Automerge index document ID
(e.g., `automerge:abc123...`). The client already has this before connecting, so there is no
additional round-trip in the happy path.

### /auth/actor security properties

- **Auth required**: Returns 401 if no valid cookie — actor_ids are not public.
- **No CSRF needed**: GET is idempotent and returns no sensitive data that could be
  stolen via CSRF (attacker already needs the user's cookie to call it).
- **No server-side project validation**: The endpoint does not check that `project_id`
  refers to an existing project. An invalid project_id just yields an actor_id that
  will never match any document content. This keeps the endpoint stateless and simple.

### Client call pattern

Before:
```typescript
const actorId = auth?.actorId;   // from /auth/me, globally stable SHA256
await connectAndLoadContents(server, indexDocId, actorId, screenName, color);
```

After:
```typescript
if (!auth) return;
const actorId = await fetchActorId(indexDocId);
if (actorId === null) {
  // Session expired — 401/403 from /auth/actor
  await logout();
  setAuth(null);  // triggers <LoginScreen />
  return;
}
await connectAndLoadContents(server, indexDocId, actorId, screenName, color);
setActorId(actorId);  // store in App state for Editor prop
```

`fetchActorId` mirrors the `fetchAuthMe` pattern: returns `null` on 401/403, throws
on unexpected errors. This ensures an expired session during project connect redirects
the user to the login page rather than silently failing.

This adds one HTTP round-trip before the WebSocket connection, but that connection
already requires network access and takes much longer in practice.

### `actorId` App state

`auth?.actorId` is used in two independent ways in `App.tsx`:
1. Passed to `connectAndLoadContents` / `createNewProject` (Automerge identity)
2. Passed as `<Editor actorId={...}>` prop (for `ReplayDrawer.currentActorId`)

Both usages must move off `auth.actorId`. The solution: add `const [actorId, setActorId] =
useState<string | undefined>()` to `App`. Set it after each successful project connection
(after `fetchActorId`), clear it in `handleDisconnect`. Pass `actorId` (not `auth?.actorId`)
to `<Editor>`.
