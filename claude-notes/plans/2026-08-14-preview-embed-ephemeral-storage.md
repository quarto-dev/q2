# Preview-embed ephemeral storage mode

## Overview

`q2 preview --ui editor` (and `--share`/`--join` guests) serves the
hub-client **preview-embed build** from a fresh origin per session
(random loopback port). That build currently persists to IndexedDB:

- the `automerge` database (full automerge document cache —
  `projectSetService.acquireServer` uses `IndexedDBStorageAdapter`)
- the `quarto-hub` database (project entries, user identity,
  project-set pointer — `projectStorage` / `userSettings` /
  `projectSetStorage` via `storage/db.ts:getDb`)

Because the origin is never reused, none of this is ever read again:
it accumulates unboundedly across preview sessions (documented wart in
`claude-notes/plans/2026-08-03-q2-preview-live-share-iroh.md`, Phase 4;
the "follow-up strand" was never filed — this plan files it).

**Fix (prevention, not cleanup):** an ephemeral storage mode selected at
build time by `VITE_EPHEMERAL_STORAGE=1` in the `build:preview-embed`
npm script (same pattern as `VITE_DISABLE_PWA=1`). When on:

1. `acquireServer` uses `MemoryStorageAdapter` (from
   `@quarto/quarto-sync-client`, same adapter the viewer SPA uses via
   `storage: 'memory'`) — the `automerge` DB is never created.
2. `getDb()` returns an in-memory facade of the `IDBPDatabase` subset
   the three consumer modules use — the `quarto-hub` DB is never
   created. Consumers are untouched.
3. Reload survival: with in-memory storage a reload loses the project
   entry, and the boot URL's `ephemeral=true` flag is gone (the share
   handler cleared the hash). Two consequences, both handled:
   - `App.tsx` derives `ephemeralHub` from the build flag as well as
     the boot URL, so onboarding gates stay off after reload.
   - On a `project`/`file` route miss, the app fetches
     `/api/preview/config` (already served by the preview server,
     already carrying `editorBoot` = `{indexDocId, file, name}` for
     editor sessions, bd-7htq16rx) and rewrites the hash to the share
     route, re-entering the normal share flow. Guests work too: their
     proxy tunnels the config fetch to the host.

Out of scope: `localStorage` (UI prefs — bytes, browser-capped),
debug-only services (`src/debug/`), the viewer SPA (already memory-only
via `storage: 'memory'`, PreviewApp.tsx:829).

## Work Items

### Phase 1: Tests (TDD — write first, verify they fail)

- [x] `src/services/ephemeralStorage.test.ts` — flag parsing
  (`vi.stubEnv` precedent: previewConfig.test.ts:123); repo storage
  adapter selection (memory when on; `IndexedDBStorageAdapter` mocked,
  mirroring projectSetService.connect.test.ts's websocket mock)
- [x] `src/services/storage/memoryDb.test.ts` — facade contract:
  get/put/delete per store keyPath (`projects` → `id`, others → `key`),
  `index('indexDocId').get`, `index('lastAccessed').getAll` (ascending,
  matching IDB index order), `objectStoreNames.contains`, pre-seeded
  `_meta` schema version, `migratePointerToCollections` self-heal path
- [x] `src/services/storage/ephemeralConsumers.test.ts` — with the flag
  on and **no `indexedDB` global** (node env: touching it throws), the
  real `projectStorage` / `userSettings` / `projectSetStorage` modules
  perform their CRUD in-memory
- [x] `previewConfig.test.ts` — `editorBoot` parsed when valid, dropped
  when malformed, absent OK
- [x] `routing.test.ts` — share-hash builder used by recovery (if a new
  helper is added)

### Phase 2: Implementation

- [x] `src/services/ephemeralStorage.ts` — `isEphemeralStorage()` (lazy
  `import.meta.env` read so tests can `stubEnv` without module reset) +
  `repoStorageAdapter()` selection
- [x] `src/services/storage/memoryDb.ts` — in-memory facade; pre-seed
  `_meta` with `CURRENT_SCHEMA_VERSION`
- [x] `src/services/storage/db.ts` — `getDb()` returns the facade when
  ephemeral (cached like `dbPromise`; `resetDbPromise` clears both);
  skips `openDB` and `runMigrations`
- [x] `src/services/projectSetService.ts` — `acquireServer` uses
  `repoStorageAdapter()`
- [x] `src/services/previewConfig.ts` — parse `editorBoot`
  (`{indexDocId, file, name}`, camelCase per
  `crates/quarto-preview/src/lib.rs:169-180`)
- [x] `src/App.tsx` — `ephemeralHub` from boot URL **or** build flag;
  route-miss recovery via config `editorBoot` → share-route hash
- [x] `hub-client/package.json` — `VITE_EPHEMERAL_STORAGE=1` in
  `build:preview-embed`

### Phase 3: Verification

- [x] `npm run test` (unit) green; new tests fail without the
  implementation (verified during Phase 1)
- [x] `npm run build:all` green (CRITICAL per CLAUDE.md — stricter than
  vitest)
- [x] `npm run build:preview-embed` green (the flagged build)
- [x] End-to-end: `cargo xtask build-hub-client-embed` +
  `cargo build --bin q2`, run `q2 preview --ui editor` on a fixture,
  drive a real browser session; assert via DevTools protocol that
  `indexedDB.databases()` is empty after editing, and that a page
  reload reconnects to the document (no "Project not found")

### Phase 4: Bookkeeping

- [x] hub-client changelog two-commit workflow (CLAUDE.md)
- [x] Close the braid strand filed by this plan
- [x] Note the fixed wart in the live-share plan's Phase 4 section

## Details

### Why a facade at `getDb()` instead of per-module branches

All three `quarto-hub` consumers funnel through `getDb()`; the IDB
surface they use is small (`get`/`put`/`delete`,
`transaction→objectStore→index→{get,getAll}`, `objectStoreNames.contains`,
`close`). One facade (~100 lines) replaces ~23 branch points across 15
exported functions, and the node-environment unit tests prove the
ephemeral path never touches the real `indexedDB` global (it is
undefined there, so any leak throws).

### Why the build flag, not the runtime `ephemeral=true` URL flag

The URL flag is captured once at boot; after the share handler clears
the hash, a reload no longer carries it (App.tsx:197). The build flag
is stable for the whole artifact, and `dist-preview-embed/` is only
ever served by `q2 preview`, so build-flag ≡ ephemeral session. The
runtime flag stays for the production build's share-route handling.

### Side effects (intended)

- The project-set migration screen wart (Phase 4 plan) disappears:
  in-memory `listProjects()` is always empty on boot, so
  `needs-migration` never triggers.
- Each reload creates a fresh project-set root doc against the
  ephemeral hub. Harmless: the hub is per-session too.

## End-to-end record (2026-08-14)

Binary: `target/debug/q2` with the freshly built `dist-preview-embed/`.
Fixture: `.tmp-ephemeral-e2e/index.qmd` (single-page project, deleted
after the run). Invocation:

```
./target/debug/q2 preview .tmp-ephemeral-e2e --ui editor --no-browser --port 4799
# → http://127.0.0.1:4799/#/share/<docId>?server=%2Fws&file=index.qmd&name=...&ephemeral=true
```

A Playwright (Chromium) script drove a real browser session: opened the
share URL, waited for the editor (`.monaco-editor`), read
`indexedDB.databases()`, reloaded the page, and re-checked.

Observed (inspected output):

- After boot: `indexedDB.databases()` = `["quarto-cache"]` — the
  `automerge` and `quarto-hub` databases are never created.
  (`quarto-cache` is the WASM bridge's LRU artifact cache, pre-existing
  and shared with the viewer SPA — follow-up strand bd-91mdd056.)
- After `page.reload()`: the session recovered via
  `/api/preview/config` `editorBoot` — the URL moved from
  `#/p/0b2b9936-…/file/index.qmd` to a fresh `#/p/7606c8c1-…/file/index.qmd`,
  the editor reconnected, and no "Project not found" error appeared.
- One non-fatal pageerror during boot ("WASM module not initialized")
  was reproduced on a build **without** these changes (stash → rebuild
  → probe), confirming it is pre-existing — filed as bd-scudmryg.

The same script run against the pre-fix build failed as expected:
`indexedDB.databases()` after boot contained `automerge`.

Verification: `npm run test` (908 passed), `npm run test:ci`,
`npm run build:all`, `npm run build:preview-embed`,
`cargo nextest run -p quarto --bin q2` (163 passed), and
`cargo xtask verify --skip-hub-build --skip-hub-tests` all green.
