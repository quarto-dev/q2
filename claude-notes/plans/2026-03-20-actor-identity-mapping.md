# Actor Identity Mapping Plan

## Overview

Tie Automerge actor IDs (SHA-256 hex strings from Google OAuth `sub`) to user screen names by storing an `identities` mapping in the IndexDocument. Add schema versioning so the document format can evolve. In replay, resolve actor hashes to screen names and use a distinct "Me" bounding box colour.

## Current State

- **IndexDocument schema** (`ts-packages/quarto-automerge-schema/src/index.ts`):
  ```typescript
  interface IndexDocument {
    files: Record<string, string>;  // path -> docId (exposed as FileEntry[])
  }
  ```
- **Screen names** are stored locally in IndexedDB (`userSettings` store) and used only for presence (ephemeral messaging). They are **not** persisted in the Automerge document.
- **Actor IDs** are SHA-256 hashes of Google OAuth `sub` claims, computed server-side and returned via `GET /auth/me`. They are set on the Automerge repo and embedded in change history.
- **Replay** shows `state.actor.slice(0, 8)` (truncated hex) or "Me" if the actor matches `currentActorId`. The "Me" case uses the same CSS styling as other actors.
- **Waveform** colours are derived from actor hash via `actorColor()` — deterministic hue from first 6 hex chars.
- **`getUserIdentity()`** (`hub-client/src/services/userSettings.ts`) is async — returns `Promise<UserSettings>` with `.userName`.
- **`SyncClientCallbacks`** is defined in `ts-packages/quarto-sync-client/src/types.ts`. The `onFilesChange` callback already fires on any IndexDocument change.

## Design

### Schema V1: IndexDocument with identities and version

```typescript
interface IndexDocument {
  version: number;                              // schema version (1)
  files: Record<string, string>;                // path -> docId (unchanged)
  identities: Record<string, string>;           // actorId -> screenName
}
```

- `version` field: integer, currently `1`. Missing `version` means V0 (pre-versioning).
- `identities` field: maps Automerge actor ID (64-char hex) to the user's current screen name.
- Both fields are added to the Automerge document, so they sync to all peers and persist in history.

### Migration Strategy

When a client opens a project and the IndexDocument has no `version` field:
1. Set `version = 1`
2. Initialize `identities = {}`
3. Write the current user's identity mapping

This is safe because Automerge merges concurrent changes — if two clients migrate simultaneously, both additions merge cleanly (they're setting non-conflicting keys).

**Automerge initialization note**: Inside a `handle.change()` callback, assigning `doc.identities = {}` creates an Automerge Map proxy. This is the correct pattern — Automerge intercepts the assignment. However, **do not** create a plain JS object outside the callback and assign it; always assign inside `change()`.

### Identity Sync on Connect

Every time a user opens a project (in `connect()` or `createNewProject()`):
1. Read `doc.identities[actorId]`
2. Compare to the user's current screen name
3. If missing or different, update `doc.identities[actorId] = screenName`

This ensures name changes are propagated to the document.

### Screen Name Loading

Screen name is loaded from `getUserIdentity()` (async, IndexedDB) in `App.tsx`. It must be available **before** any connect/create call. The loading sequence:

1. Auth resolves (`useAuth()` — provides `actorId` and OIDC `name`)
2. User identity resolves (`getUserIdentity()` — provides `userName`)
3. **OIDC name defaulting**: If auth provides a display name and the stored name is still an auto-generated anonymous one (starts with "Anonymous "), the screen name is automatically upgraded to the OIDC display name and persisted to IndexedDB. This happens once on first login; subsequent visits use the persisted value.
4. Both are available when project selector renders
5. `screenName` is passed alongside `actorId` through `connect()` and `createNewProject()`

For non-auth instances (`AUTH_ENABLED` is false), screen name loads immediately from IndexedDB without waiting for auth, preserving the anonymous name default.

### Screen Name Reset

The ProjectSelector "Your Identity" section has a reset button:
- **With auth**: "Reset Name" — resets screen name to the OIDC display name (`auth.name`)
- **Without auth**: "Randomize Name" — generates a new anonymous name (original behavior)

### Identity Change Notification

A separate `onIdentitiesChange` callback keeps concerns cleanly separated — file changes and identity changes are logically independent events (e.g. a new user connecting updates identities but not files). The `onFilesChange` signature stays untouched.

```typescript
onIdentitiesChange?: (identities: Record<string, string>) => void
```

In the `indexHandle.on('change', ...)` handler, the sync client diffs both `files` and `identities` from the previous state, firing each callback only when its data actually changes. This means consumers subscribe only to what they care about.

### Stale Identities

Identities persist in the Automerge document forever. This is intentional — the document history is immutable, and stale identity mappings are harmless (they only map actor ID to screen name). Old contributors' names remain resolvable in replay, which is a feature.

### Replay Changes

- **Actor label**: Instead of `state.actor.slice(0, 8)`, look up `identities[state.actor]` from the IndexDocument. Fall back to truncated hex if not found.
- **"Me" indicator**: Use a visually distinct bounding box (different border colour/background) instead of replacing the name with "Me". Display both the screen name AND the "Me" indicator so the user knows their own name as others see it.
- **Waveform**: `actorColor()` stays hash-based (deterministic, no lookup needed). No change.

## Work Items

### Phase 1: Tests for Schema Changes

- [x] **1.1** Write unit tests for `IndexDocument` migration in `ts-packages/quarto-automerge-schema`:
  - V0 doc (no version, no identities) -> migrates to V1
  - V1 doc (already has version) -> no-op
  - `setIdentity` adds new, overwrites changed, leaves unchanged
- [x] **1.2** Run tests — verify they pass (8 tests)

### Phase 2: Schema Implementation

- [x] **2.1** Update `IndexDocument` type in `ts-packages/quarto-automerge-schema/src/index.ts`:
  - Add `version?: number` and `identities?: Record<string, string>` fields
  - Make them optional since V0 docs won't have them
  - Export a `CURRENT_SCHEMA_VERSION = 1` constant
- [x] **2.2** Add a migration helper in `ts-packages/quarto-automerge-schema/src/index.ts`:
  - `migrateIndexDocument(doc: IndexDocument): boolean` — returns true if changes were made
  - Checks for missing `version`, sets to 1, initializes `identities` if absent
- [x] **2.3** Add an identity update helper:
  - `setIdentity(doc: IndexDocument, actorId: string, screenName: string): boolean` — returns true if identity was added/updated
- [x] **2.4** Run Phase 1 tests — verify they pass

### Phase 3: Tests for Sync Client Integration

- [ ] **3.1** Write unit tests for sync client identity flow in `ts-packages/quarto-sync-client`:
  - `connect()` with actorId + screenName writes identity to doc
  - `createNewProject()` initializes with version and identity
  - Screen name update overwrites stale value
  - `onIdentitiesChange` fires when identities change but not when only files change
- [ ] **3.2** Run tests — verify they fail

### Phase 4: Sync Client Implementation

- [x] **4.1** Add `onIdentitiesChange` callback to `SyncClientCallbacks` in `ts-packages/quarto-sync-client/src/types.ts`:
  - `onIdentitiesChange?: (identities: Record<string, string>) => void`
  - `onFilesChange` signature stays unchanged
- [x] **4.2** Update `connect()` in `ts-packages/quarto-sync-client/src/client.ts`:
  - Add `screenName?: string` parameter (after `actorId`)
  - After loading the IndexDocument, call `indexHandle.change()` to migrate schema if needed
  - Write `identities[actorId] = screenName` if it differs from what's stored
  - In the `indexHandle.on('change', ...)` handler, diff identities from previous state and call `onIdentitiesChange` only when identities actually changed
  - Fire `onIdentitiesChange` on initial load with current identities
- [x] **4.3** Update `createNewProject()` in the same file:
  - Add `screenName?: string` parameter (after `actorId`)
  - When initializing the IndexDocument, set `version: 1`, `identities: {}`, and write the creator's identity
  - Fire `onIdentitiesChange` with initial identities
- [ ] **4.4** Run Phase 3 tests — verify they pass

### Phase 5: Tests for Hub-Client UI

- [x] **5.1** Write tests for `ReplayDrawer`:
  - Renders screen name instead of hex when identity available
  - Renders truncated hex when identity not available
  - Applies `--me` CSS class for current actor
- [x] **5.2** Run tests — verify they pass (29 tests)

### Phase 6: Hub-Client Implementation

- [x] **6.1** Update `automergeSync.ts` (`hub-client/src/services/automergeSync.ts`):
  - Add `screenName` parameter to `connect()` and `createNewProject()`
  - Forward to sync client
- [x] **6.2** Update `App.tsx` (`hub-client/src/App.tsx`):
  - Add `screenName` state, loaded via `getUserIdentity().userName` in a `useEffect` (runs after auth resolves, or unconditionally when auth is disabled)
  - Pass `screenName` to all `connectAndLoadContents()` and `createNewProject()` calls
  - Store `identities` map in React state via `onIdentitiesChange` callback
  - Pass `identities` to `Editor`
- [x] **6.3** Update `Editor.tsx` (`hub-client/src/components/Editor.tsx`):
  - Accept `identities` prop (`Record<string, string>`)
  - Pass it through to `ReplayDrawer`
- [x] **6.4** Update `ReplayDrawer` (`hub-client/src/components/ReplayDrawer.tsx`):
  - Add `identities` to `Props` interface
  - Update actor label rendering:
    - Resolve `state.actor` to screen name via `identities[state.actor]`
    - Fall back to `state.actor.slice(0, 8)` if no identity found
    - For "Me": show `screenName (Me)` alongside the resolved name
  - Add `replay-drawer__actor--me` CSS class with distinct border/background colour
- [x] **6.5** Run Phase 5 tests — verify they pass

### Phase 7: Full Verification

- [x] **7.1** `cargo build --workspace` — passes
- [x] **7.2** Run hub-client tests: 383 tests pass (including schema + ReplayDrawer)
- [x] **7.3** Run hub-client build: passes
- [x] **7.4** e2e fixtures create V0 IndexDocuments; migration handles them at connect time — no changes needed

## Files Modified

### `ts-packages/quarto-automerge-schema/src/index.ts`
- `IndexDocument` type: add `version?` and `identities?`
- New exports: `CURRENT_SCHEMA_VERSION`, `migrateIndexDocument()`, `setIdentity()`

### `ts-packages/quarto-sync-client/src/types.ts`
- Add `onIdentitiesChange?: (identities: Record<string, string>) => void` callback
- `onFilesChange` signature unchanged

### `ts-packages/quarto-sync-client/src/client.ts`
- `connect()`: add `screenName` param, call migration + identity sync, fire `onIdentitiesChange`
- `createNewProject()`: add `screenName` param, initialize V1 schema with identity, fire `onIdentitiesChange`
- In change handler, diff identities and fire `onIdentitiesChange` only when identities changed

### `hub-client/src/services/automergeSync.ts`
- `connect()`, `createNewProject()`: add `screenName` passthrough

### `hub-client/src/App.tsx`
- New state: `screenName` (loaded from `getUserIdentity()`, auto-upgraded from OIDC name on first login)
- Pass `screenName` to connect/create calls (with `screenName` in dependency arrays)
- Store + update `identities` via `onIdentitiesChange` callback
- Pass `identities` to `Editor`
- Pass `authName` to `ProjectSelector` for screen name reset

### `hub-client/src/components/Editor.tsx`
- Accept + forward `identities` prop to `ReplayDrawer`

### `hub-client/src/components/ProjectSelector.tsx`
- Accept `authName` prop (OIDC display name)
- "Reset Name" button: resets to OIDC name when available, randomizes when not

### `hub-client/src/components/ReplayDrawer.tsx`
- Accept `identities` prop
- Resolve actor to screen name
- Add `--me` CSS class with distinct styling

### `hub-client/src/components/ReplayDrawer.css`
- New `.replay-drawer__actor--me` class

### Test files (new or updated)
- `ts-packages/quarto-automerge-schema/src/__tests__/migration.test.ts`
- `ts-packages/quarto-sync-client/src/__tests__/identity.test.ts`
- `hub-client/src/components/ReplayDrawer.test.tsx` (update existing)

### Possibly updated
- `hub-client/e2e/scripts/regenerate-fixtures.ts` (if fixtures include IndexDocument)

## Key Decisions

1. **Optional fields**: `version` and `identities` are optional on the type to handle V0 docs. Migration happens at connect time.

2. **Screen name source**: The screen name stored in IndexedDB is the authoritative source, persisted in the Automerge document's `identities` map. On first login, if the stored name is still an auto-generated "Anonymous ..." name, it is automatically upgraded to the OIDC display name (`auth.name`) and persisted to IndexedDB. Users can further customize it; a "Reset Name" button restores it to the OIDC display name. For non-auth instances, anonymous names are used as the default.

3. **Screen name loading**: Loaded in `App.tsx` via `useEffect` after auth. Available before any project connection. Passed explicitly through the call chain — not fetched inside the sync client (which has no IndexedDB access).

4. **No colour in identities**: Cursor colours are per-session (presence) and per-hash (replay waveform). Storing them in the document would create conflicts. Keep colours derived locally.

5. **Concurrent migration safety**: Automerge's CRDT semantics mean two clients setting `version = 1` and `identities = {}` simultaneously will merge correctly. Identity writes to different keys merge without conflict.

6. **"Me" indication**: Use a different CSS class (`--me`) with a distinct border colour rather than replacing the name. This way the user sees their own name as others see it while still knowing which edits are theirs.

7. **Separate `onIdentitiesChange` callback**: Files and identities are logically independent events (a user connecting changes identities but not files). A dedicated callback keeps `onFilesChange` untouched, lets consumers subscribe only to what they need, and fires only when identities actually change.

8. **Stale identities are fine**: Identity mappings persist forever in the Automerge document. The history is immutable, and keeping old mappings means replay can always resolve actor names — even for contributors who have left.

9. **Scrubber tooltip**: The waveform scrubber tooltip only shows timestamps (not actor info), so no changes needed there.
