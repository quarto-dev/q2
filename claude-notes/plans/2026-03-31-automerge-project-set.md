---
title: 'Automerge-Backed Project Set Storage'
date: 2026-03-31
---

## Overview

Replace the current per-browser IndexedDB project list with a synced Automerge
document (a "project set"), enabling users to share their project list across
multiple browsers. IndexedDB retains only a pointer to the project set document
ID.

## Problem

Today, each browser maintains its own list of `ProjectEntry` objects in
IndexedDB. This means:

1. Users cannot see their project list on a new browser without export/import.
2. Users with projects split across browsers have no way to unify them.
3. The export/import flow is manual and error-prone.

## Current Architecture

```
IndexedDB ("quarto-hub")
├── projects store          ← ProjectEntry[] (id, indexDocId, syncServer, description, timestamps)
├── userSettings store      ← singleton UserSettings
└── _meta store             ← schema version + migration history

Automerge (via IndexedDBStorageAdapter — separate IDB database)
└── Per-project IndexDocument  ← { files: { path → docId }, version, identities }
    └── Per-file document      ← TextDocumentContent | BinaryDocumentContent
```

Each `ProjectEntry` is purely local metadata pointing to an Automerge
`IndexDocument` on a sync server.

## Proposed Architecture

```
IndexedDB ("quarto-hub")
├── projectSet store        ← singleton { projectSetDocId, syncServer }
├── userSettings store      ← unchanged
└── _meta store             ← schema version bumped to 4

Automerge
├── ProjectSetDocument      ← NEW: { projects: { indexDocId → ProjectSetEntry }, version }
│   (synced across browsers via the same sync server)
│
└── Per-project IndexDocument  ← unchanged
    └── Per-file document      ← unchanged
```

### ProjectSetDocument Schema

A new Automerge document type in `quarto-automerge-schema`:

```typescript
export interface ProjectSetEntry {
  indexDocId: string;       // automerge: prefixed document ID
  syncServer: string;       // WebSocket URL for the sync server
  description: string;      // user-provided name
  addedAt: string;          // ISO timestamp when added to this set
  lastAccessed: string;     // ISO timestamp, updated on any browser
}

export interface ProjectSetDocument {
  projects: Record<string, ProjectSetEntry>;  // key = indexDocId (without prefix)
  version: number;                             // schema version (1 initially)
}
```

**Key decisions:**

- **Key = indexDocId (without prefix)**: Natural dedup key. Two browsers adding
  the same project converge automatically.
- **`lastAccessed` is synced, not local**: Since the project set is shared by
  the same user across their browsers, "when did I last open this anywhere?" is
  the useful question. Concurrent timestamp updates are benign — Automerge's LWW
  semantics pick one, and either value is a reasonable "recent" timestamp. Updates
  to different projects don't conflict at all (independent map entries).
- **`addedAt` instead of `createdAt`**: The project itself may have been created
  earlier; this is when it was added to this particular set.

### What stays in IndexedDB

IndexedDB becomes minimal — just the pointer to the project set document:

```typescript
interface ProjectSetPointer {
  key: 'projectSet';             // singleton
  projectSetDocId: string;       // automerge document ID for the project set
  syncServer: string;            // sync server for the project set doc
}
```

No per-browser local meta is needed. `lastAccessed` lives in the Automerge
document and syncs across browsers (see rationale above).

## Sharing the Project Set

### UI Flow: "Link a New Browser"

From the project selector, a new action: **"Link Another Browser"** (or similar
wording). This opens a dialog similar to ShareDialog:

1. Shows the project set document ID
2. "Copy Link" button generates a URL like:
   `#/link-project-set/<projectSetDocId>?server=<syncServer>`
3. User opens this URL on their other browser
4. That browser stores the `projectSetDocId` in its IndexedDB and connects

### UI Flow: "Set Up on New Browser"

When a user opens hub-client for the first time (no `projectSet` pointer in
IndexedDB), they see a setup screen:

1. **"Create New Project Set"** — creates a fresh ProjectSetDocument
2. **"Link to Existing Project Set"** — paste/scan a link from another browser

This replaces the current "empty project list" state.

### Security Considerations

- The project set document ID is a bearer token (same as indexDocId for
  projects). Anyone with it can read/modify your project list.
- The "Link Another Browser" dialog should have the same warning as ShareDialog:
  "Anyone with this link can access your project list."
- Project set URLs should be immediately replaced in browser history (same
  pattern as share URLs).

## Migration Plan

### Phase 1: Schema + Data Model

- [x] Add `ProjectSetDocument` and `ProjectSetEntry` types to `quarto-automerge-schema`
- [x] Add `ProjectSetPointer` type to hub-client storage types
- [x] Add IDB migration v3→v4: create `projectSet` store

### Phase 2: Migration Logic (Existing Users)

When an existing user (has projects in IndexedDB but no `projectSetDocId`)
opens hub-client:

- [x] Detect the pre-migration state (projects store has entries, no projectSet pointer)
- [x] Show a migration screen with:
  - An explanation of what's changing ("your project list will now sync across
    browsers")
  - A prominent **"Download Backup"** button that triggers the existing JSON
    export — framed as "back up your project list before migrating"
  - A **"Migrate"** button to proceed (enabled regardless of whether they backed
    up — we encourage it, not enforce it)
- [x] On "Migrate": create a new `ProjectSetDocument` on the sync server
- [x] Populate it with entries from the existing IndexedDB `projects` store
- [x] Store the `projectSetDocId` in the new `projectSet` IDB store
- [x] Show a success notification (transitions to connected project selector)
- [x] Keep old `projects` store data for one release cycle as a safety net, then
  remove in a later migration

**Cross-browser merge scenario**: A user who already has a project set on
browser A opens browser B (which still has the old IndexedDB project list):

- [x] The new-browser setup screen should have a third option: **"Merge with
  Existing Project Set"** — paste the project set link, then merge the local
  IDB projects into the Automerge document (skipping duplicates by indexDocId)
- [x] After merge, store the `projectSetDocId` and clear the old `projects` store

### Phase 3: Core Service Layer

- [x] Create `projectSetService.ts` — CRUD operations on the Automerge ProjectSetDocument
  - `connect(syncServerUrl, projectSetDocId)` → connects and returns entries
  - `createProjectSet(syncServerUrl)` → creates new doc, returns docId
  - `addProject(entry)` / `addProjectsBulk(entries)` → adds to the Automerge doc
  - `removeProject(indexDocId)` → removes from the Automerge doc
  - `updateProjectDescription(indexDocId, description)` → updates description
  - `listProjects()` → returns current entries sorted by lastAccessed
  - `touchProject(indexDocId)` → updates lastAccessed in the Automerge doc
- [x] Create `projectSetStorage.ts` — IndexedDB operations for the pointer
  - `getProjectSetPointer()` → returns the stored pointer or null
  - `setProjectSetPointer(docId, syncServer)` → stores the pointer
  - `clearProjectSetPointer()` → removes the pointer (for unlinking)
- [x] Wire up Automerge change callbacks to keep the UI reactive when remote
  changes arrive (another browser adds/removes a project)

### Phase 4: UI Changes

- [x] Modify `ProjectSelector.tsx` to read from `projectSetService` instead of
  `projectStorage`
- [x] Add first-time setup screen (create new / link existing / merge)
- [x] Add "Link Another Browser" button + dialog to project selector
- [x] Add "Project Set Settings" section (shows document ID, link another browser)
- [x] Update project creation flow to write to Automerge project set instead of IDB
- [x] Update share-link handling to also add shared projects to the Automerge set
- [x] Update export/import to work with the new model (export from Automerge doc)
- [x] Handle `#/link-project-set/` route in App.tsx initial URL handler

### Phase 5: URL scheme change and cleanup

**`projectStorage.ts` is still actively used** — not as a safety net, but because
URL routing uses local IDB project IDs. When a user selects a project from the
synced set, we look up or create a local IDB entry to get a local ID for the URL.

**RESOLVED: URL prefix changed from `#/project/` to `#/p/`.** The old prefix
leaked the word "project" which could be confused with the Automerge indexDocId.
The new `#/p/<localId>` prefix is opaque — it doesn't reveal that the ID is
local-only, and it's short. The local IDB project entries and `projectStorage.ts`
remain load-bearing because we intentionally avoid exposing the Automerge
document ID in daily-use URLs (it's a bearer token).

- [x] Rename URL prefix from `#/project/` to `#/p/`
- [ ] If switching URLs: remove local IDB project entries and `projectStorage.ts`
- [ ] Remove old `projects` IDB store in a future migration (v4→v5)
- [ ] Update tests throughout
- [ ] Update "Connect to Project" form to also write to project set

## Versioning Responsibility Split

| Concern | Storage Layer | Notes |
|---------|--------------|-------|
| Project set membership | Automerge ProjectSetDocument | Synced |
| Project metadata (description, syncServer, lastAccessed) | Automerge ProjectSetDocument | Synced |
| Project set pointer | IndexedDB `projectSet` | Local singleton |
| User identity (name, color) | IndexedDB `userSettings` | Local-only (future: could also sync) |
| Schema migration tracking | IndexedDB `_meta` | Local-only |
| Project file content | Automerge IndexDocument + file docs | Synced (unchanged) |

## Open Questions

1. **RESOLVED: Same sync server for project set and projects.** Yes. The
   deployment will eventually have a default sync server, further reducing this
   friction. Individual `ProjectSetEntry` records still carry their own
   `syncServer` field, so projects on different servers remain representable.

2. **RESOLVED: Single project set for now.** One project set per user. The
   design naturally admits multiple project sets in the future (e.g., a team
   could create a shared project set independently of personal lists) — the
   `ProjectSetDocument` is just an Automerge document with a shareable ID, so
   nothing prevents having several. But we won't build UI or plumbing for
   switching/combining sets in this iteration.

3. **RESOLVED: Migration must be atomic w.r.t. IndexedDB.** If the sync server
   is unreachable during migration, the migration **fails loudly** and leaves
   IndexedDB unchanged — no partial state. The old `projects` store continues to
   work as before. Migration is retried on next app load. Implementation notes:
   - Create the Automerge doc and confirm it's synced **before** writing the
     `projectSetDocId` pointer to IndexedDB.
   - Never delete or modify old IDB data until the pointer is successfully
     written.
   - Show the user a clear error: "Could not reach sync server — your projects
     are safe, migration will retry automatically."
   - This is hard to test end-to-end (requires simulating server failure), but
     the code structure should make the ordering guarantee obvious: the IDB
     write is the commit point, and it happens last.

4. **RESOLVED: New route type `#/link-project-set/<docId>?server=<url>`.**
   Distinct from `#/share/...` — linking a project set is a different operation
   from joining a single project. Using "project set" terminology also makes the
   future multiple-sets feature easier to describe.

5. **RESOLVED: `lastAccessed` lives in Automerge.** Since the project set is
   shared by the same user, "when did I last access this anywhere?" is the right
   question. Concurrent LWW conflicts are benign (both values are "recent").

6. **RESOLVED: Keep JSON export/import.** It remains useful as a backup
   mechanism. The export should source from the Automerge doc. The buttons will
   be de-emphasized in a future project selector UI redesign.

## Risks

- **Data loss during migration**: Mitigated by keeping old IDB data as a safety
  net for one release cycle.
- **Conflict resolution**: Automerge's CRDT semantics handle concurrent adds
  well (Record merges naturally). Since this is a single user managing their own
  list, adversarial concurrent edits are not a realistic concern.
- **Offline resilience**: The Automerge IndexedDBStorageAdapter already handles
  offline. The project set document will be cached locally like any other
  Automerge doc. First load on a new browser requires connectivity.
