# Collections in project export/import

**Branch:** `feature/collections-export` (off main at `c6ab84c2`)
**Braid:** not filed — braid CLI unresolved on this machine (`projects.toml` missing `[projects.q2]`); file a strand when available.

## Overview

Every project-list export surface emits a flat, pre-collections (schemaVersion 4)
shape with no collection information:

- **Projects home "Export list"** — `handleExportJson` in
  `hub-client/src/components/ProjectsHome.tsx` (~line 894): exports the root
  set's entries only.
- **Classic selector export** — `handleExport` in
  `hub-client/src/components/ProjectSelector.tsx` (~line 491): same flat shape.
- **IDB-level `exportData()`** — `hub-client/src/services/projectStorage.ts`
  (~line 114): `projects` store + `userSettings`; never reads the
  `collections` pointer array from the `projectSet` store.

Consequence: an export → import round trip on a fresh browser restores projects
but loses (a) which collections existed / which projects were in them, and
(b) the collection **doc ids**, so the browser cannot re-subscribe to the shared
collection ProjectSetDocuments (the user must re-acquire invite links).

Because collections are synced Automerge docs, restoring the **pointer**
(docId + syncServer) restores everything else: membership, name, and future
edits all sync down once subscribed. So import only needs to re-subscribe;
it must NOT write membership into collection docs.

## Design

New export shape (bump export `schemaVersion` to 5; the field is independent of
the IDB migration version but happens to align):

```ts
interface ExportedCollection {
  projectSetDocId: string;   // the collection's ProjectSetDocument id
  syncServer: string;
  name?: string;             // display only — the doc is authoritative
  isRoot?: boolean;          // true for the personal root superset
  projectIds?: string[];     // member indexDocIds, display/debug only
}
// ExportData gains: collections?: ExportedCollection[]
```

Notes:
- `name`/`projectIds` are informational (human-readable export, debugging);
  import ignores them and trusts the synced doc.
- The root collection IS included (marked `isRoot`) so the export is a complete
  pointer record, but import never re-subscribes the root — the importing
  browser already has (or creates) its own root.
- Import re-subscribes via the existing `subscribeCollection(docId, syncServer)`
  action (`useCollectionSets.ts` ~line 395), deduped by
  `addCollectionPointer`'s existing same-docId check.
- Old exports (schemaVersion 4 / no `collections` field) must import exactly as
  today. New exports importing into old builds degrade gracefully (unknown
  field ignored) — verified by shape, no code needed there.

### New module: `src/services/projectListExport.ts`

Pure, unit-testable helpers shared by both export surfaces:

- `buildProjectListExport(projects, collections)` → `ExportData` JSON string
  (schemaVersion 5, ISO `exportedAt`, flat `projects` + `collections`).
- `parseProjectListImport(json)` → `{ projects, collections }` — accepts v4
  (flat / no collections) and v5; throws on newer-than-known schema, same as
  `importData` does today.

`ProjectsHome` and `ProjectSelector` call these instead of hand-rolling JSON.
`projectStorage.importData` keeps handling the legacy IDB merge of `projects`;
the collections half is handled by the caller (needs the React-layer
`subscribeCollection` action, which services can't reach).

### Wiring

- `ProjectsHome` gains prop `onSubscribeCollection?: (docId, syncServer) => Promise<void>`;
  `App.tsx` passes `projectSetActions.subscribeCollection` (already exists —
  today only `JoinCollectionLanding` uses it, App.tsx ~line 733).
- `handleImportJson` (ProjectsHome): after importing projects, loop
  non-root exported collections → `onSubscribeCollection(docId, syncServer)`;
  report `Imported N project(s), subscribed to M collection(s)` (and count
  skipped/already-subscribed ones honestly).
- `ProjectSelector` (classic view) export gains collections when in
  project-set mode; its import path stays projects-only for now (classic view
  has no collections UI) — parse tolerates the field either way.

## Phases

### Phase 0 — BUG: imported projects don't appear (reported 2026-08-04)

Symptom: importing a JSON list into a new browser reports "Imported 30
project(s)" but the home displays none of them.

Mechanism (confirmed by code reading, to be confirmed by live repro):
- `handleImportJson` → `projectStorage.importData()` writes only to the legacy
  IDB `projects` store (projectStorage.ts ~line 180); the collections home
  renders from the root ProjectSetDocument's entries.
- The reconciler that folds IDB → root set
  (`reconcileIntoConnectedProjectSet`) only runs when set status transitions
  to `connected` (useCollectionSets.ts ~172) — once per page load, so a
  post-load import is never swept in without a manual reload; a fresh browser
  that hasn't completed setup never reaches that sweep at all.

Fix design: imports go through the hook, not raw storage.
- [x] Repro live in local-prod at current main (2026-08-04, main `c6ab84c2`):
  fresh browser profile → Create New Project Set → avatar menu → Import
  project list (JSON) with a 3-project schemaVersion-4 export → alert
  "Imported 3 project(s)", home still shows "No projects yet"; IDB inspection
  confirmed all 3 in the legacy `projects` store and none in the root set.
  After a manual reload the reconciler swept them in and all 3 appeared under
  "Everything else" — confirming both the stranding and the reconciler-only-
  on-load mechanism. (User's real-world 30-project import is recoverable the
  same way: reload the browser.)
- [x] New service fn `importProjectsAndReconcile(json)` in
  `projectSetReconciler.ts` (`importData` → `reconcileIntoConnectedProjectSet`;
  returns `{ imported, reconciled, connected }`), exposed as `importProjects`
  action on `useCollectionSets` (refreshes collections state). 4 new tests in
  `projectSetReconciler.test.ts` (written first, confirmed failing).
- [x] `ProjectsHome` gains `onImportProjects` prop; `App.tsx` wires the action;
  `handleImportJson` uses it (falls back to the old path in legacy mode).
  Honest messages: `Imported N` / `All projects were already in your list` /
  offline "Saved N — they'll appear when the sync connection is restored".
- [x] Repro again at `main-CNAshGsM.js` build: same steps, alert
  "Imported 3 project(s)", and all three projects appeared under
  "Everything else" immediately — no reload. Screenshot in session transcript.

### Phase 1 — tests first (pure module)

- [ ] `src/services/projectListExport.test.ts`:
  - build: v5 shape, root marked `isRoot`, member ids attached, ISO timestamp
  - build: no collections connected → `collections: []` (not absent), still v5
  - parse: v5 round trip returns projects + collections
  - parse: v4 flat export (real fixture shape) → projects, `collections: []`
  - parse: legacy bare-array format (pre-ExportData) → still accepted
  - parse: schemaVersion 6 → throws (forward-compat guard, mirrors importData)
  - parse: malformed JSON / wrong types → clear errors
- [ ] Run, confirm all fail.

### Phase 2 — implement pure module

- [ ] `src/services/projectListExport.ts` with the two helpers + types
  (`ExportedCollection`; extend `ExportData` in `storage/types.ts`).
- [ ] Tests green.

### Phase 3 — wire export surfaces

- [ ] `ProjectsHome.handleExportJson` → `buildProjectListExport(items, collections)`.
- [ ] `ProjectSelector.handleExport` (set mode) → same helper (it has
  `projectSetEntries`; check what collection data it can see — if none is
  available in classic mode, export `collections` from the pointer store via a
  small storage read instead).
- [ ] `projectStorage.exportData()` gains the collection pointers from the
  `projectSet` store (`getCollectionPointers`) so the IDB-level backup is
  complete too (names unavailable at that layer — pointers only).

### Phase 4 — wire import

- [ ] `ProjectsHome` prop `onSubscribeCollection`; pass from `App.tsx`.
- [ ] `handleImportJson`: parse via helper; import projects (existing path);
  subscribe non-root collections; honest result message; per-collection
  failures logged and counted, not silently swallowed.
- [ ] Component-level test for the import handler logic if extractable as a
  pure function (`planCollectionResubscribes(parsed, currentPointers)` —
  filters root + already-subscribed).

### Phase 5 — verify + ship

- [x] `npm run test:ci` green (876 + 109 + 130); typecheck clean.
- [x] End-to-end in local-prod (2026-08-04, `main-fP6fngYt.js` build):
  fresh browser → created root + 3 projects + collection "Restored docs" with
  "imported alpha" moved in → real Export menu produced a v5 file
  (`schemaVersion: 5`, 3 projects, 2 collections: root `isRoot: true` with 3
  member ids; "Restored docs" with 1) → wiped all browser state (fresh
  profile) → created a new root → real Import menu with that file → alert
  "Imported 3 project(s), joined 1 collection(s)"; "Restored docs" reappeared
  with "imported alpha" inside (membership arrived via sync, nothing written
  by import), root correctly NOT re-subscribed, "Everything else" = 2.
  Screenshots in session transcript.
- [ ] `npm run build:all` (deferred to commit step; build:local-prod passed).
- [ ] Two-commit workflow (code, then changelog with hash).
- [ ] Push only with explicit user approval.

## Open questions / decisions taken

- **Root in export:** included but never re-subscribed on import (decided).
- **Membership on import:** never written by import; the synced doc is the
  source of truth (decided).
- **Classic-view import of collections:** out of scope; ProjectsHome is the
  primary surface (decided — revisit if classic view sticks around).
- **`package-lock.json` churn from `npm install` at new main:** two lockfiles
  show diffs unrelated to this work; revert before committing unless the diff
  proves substantive.
