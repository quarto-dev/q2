# Project-set deletion tombstones (latest-wins reconcile)

Branch: `bugfix/bd-f5a0c6rv-project-set-deletion-tombstones`
Strand: bd-f5a0c6rv (bug, in progress)
Status: **applied** — this file records what shipped, not what is planned.

## Context / problem

Deleting a project from the synced project set (the × button in
ProjectSelector, "Remove from this device" in ProjectsHome) appeared to
work but the project came back on the next page load.

Root cause: the delete path only mutated the synced Automerge document
(`useCollectionSets.removeProject` → `projectSetService.removeProjectFromCollection`).
Every project also has a row in the legacy IndexedDB `projects` store
(written by the share-route handler, imports, and project creation), and
the delete path never touched it. On the next load, the "connected"
effect in `useCollectionSets` runs `reconcileIntoConnectedProjectSet()`,
which upserts any IDB row missing from the synced set — so the stale row
resurrected the deleted project. The same mechanism could resurrect
cross-device: browser B's stale IDB row would re-add a project deleted
on browser A once B reconnected.

## Design decision

Latest-wins on per-key deletion tombstones, per user direction.

- `ProjectSetDocument` gains `tombstones?: Record<key, deletedAtISO>`.
  Optional: existing documents lack the map; it is lazily created inside
  the first `change()` that deletes something. No schema version bump —
  the field is forward-compatible in both directions (old clients ignore
  the extra map; new code treats its absence as "no deletions yet").
- `removeProjectFromSet` deletes the entry AND stamps the tombstone.
  `addProjectToSet` (and every direct-write path) clears it: a re-add
  wins over an earlier delete.
- The reconciler compares each candidate IDB row's `lastAccessed`
  against the key's tombstone. Tombstone at-or-newer → the row is a
  stale pre-delete copy: skip it and purge it from IDB. Row newer → a
  genuine later access/re-add: restore it. Ties go to the deletion (a
  simultaneous delete/re-add pair can't loop, because the re-add clears
  the tombstone).

Rejected alternative: a single whole-set `lastModified` (or
`lastDeletedAt`) timestamp. The set is too chatty — touches, renames and
summary updates would advance the stamp and suppress legitimate pending
share-link adds (the reconciler's raison d'être), and purging on that
verdict could permanently lose never-synced projects. One timestamp can
also only record one event, so later deletions of project Y clobber the
resolution window for earlier deletions of project X. The reconciler's
question is per-key, so the timestamp must be per-key.

## What was applied

### ts-packages/quarto-automerge-schema

- [x] `src/index.ts`: `ProjectSetDocument.tombstones?: Record<string, string>`
- [x] `src/index.ts`: `removeProjectFromSet(doc, id, now?)` writes the
      tombstone (only when an entry was actually removed)
- [x] `src/index.ts`: `addProjectToSet` clears the key's tombstone on
      both the add and update paths (heals torn present+tombstoned state
      after concurrent add/delete merges)
- [x] `src/index.ts`: new `getProjectSetTombstones(doc)` reader (`{}`
      for pre-tombstone documents)
- [x] `src/projectSet.test.ts`: 5 new tests (tombstone written, no
      tombstone for absent entries, re-add clears, update path clears,
      legacy-doc reader)
- [x] `dist/` rebuilt (`npm run build`); dist is gitignored

### hub-client

- [x] `services/projectSetService.ts`: `addProjectsBulk` and
      `moveProjectBetweenCollections` clear tombstones for keys they
      (re-)add; new `getRootTombstones()` accessor
- [x] `services/projectSetReconciler.ts`: `computeReconcileAdds` takes
      an optional tombstones map and skips losing rows; new
      `computeReconcilePurges` returns tombstone-losing rows;
      `reconcileIntoConnectedProjectSet` purges losers from IDB before
      adding winners (shared `dedupeByKey` / `tombstoneWins` helpers)
- [x] `services/projectStorage.ts`: new `deleteProjectByIndexDocId`
      purge helper — deletes all rows matching the canonical key, both
      `automerge:`-prefixed and unprefixed historical variants
- [x] `hooks/useCollectionSets.ts`: `removeProject` unchanged in
      behaviour (pure set operation); comment documents the tombstone
      mechanism. The reconciler is the single place that resolves
      IDB-vs-set conflicts — no hook-level IDB delete
- [x] `services/projectSetReconciler.test.ts`: mocks extended
      (`getRootTombstones`, `deleteProjectByIndexDocId`); 6 new
      latest-wins tests + 2 purge-orchestration tests
- [x] `services/projectStorage.test.ts`: 4 new tests for
      `deleteProjectByIndexDocId` (exact, prefix variants, duplicate
      rows, no-op)

## Resulting semantics

- Delete from the set → entry removed everywhere this browser is
  subscribed, tombstone stamped. Next load: reconciler purges the stale
  IDB row instead of resurrecting it.
- Cross-device: the tombstone syncs with the document, so browser B's
  reconciler reaches the same verdict and purges its own stale row.
- Re-add after delete (share link revisited, project re-opened) wins:
  the newer `lastAccessed` beats the tombstone, and the add clears it.
- Import of an old project-list export does NOT restore a project
  deleted after the export was made (the tombstone is newer). Consistent
  with latest-wins; an explicit "import means restore" path would clear
  tombstones for imported ids — not implemented, see follow-ups.
- Mixed client versions: old clients don't write tombstones, so their
  deletions can still be resurrected by stale IDB rows until upgraded.

## Verification

- `ts-packages/quarto-automerge-schema`: `npm run build` clean;
  `vitest run` — 72 passed (4 files)
- `hub-client`: `vitest run` on projectSetReconciler, projectStorage,
  projectSetService.connect, projectSetService.debug, projectSetStorage —
  68 passed (5 files)
- `hub-client`: `npm run typecheck` clean
- Grep sweep: every direct `doc.projects[key]` write path maintains
  tombstones (addProjectsBulk, moveProjectBetweenCollections,
  addProjectToSet); no bypassing writers remain

## Follow-ups (not applied)

- Import-as-restore: `importProjectsAndReconcile` could clear tombstones
  for explicitly imported ids.
- `useProjectSet` (dead predecessor hook) shares the reconciler, so it
  inherits the fix automatically; the hook itself remains unused.
- Optional: expose tombstones in the `quartoDebug.am` debug snapshot
  (`getProjectSetDebugSnapshot`) for field diagnosis.

## Deferred: outbox semantics for the legacy `projects` store

Tracked as strand bd-ec8eop0c (discovered-from bd-f5a0c6rv).
Discussed 2026-08-21; decision was to ship tombstones as-is and revisit.

The `projects` IDB store is currently a permanent shadow of the synced
set (rows re-created/touched on every project open because URL routes
are keyed by the IDB row uuid — `#/p/<local-id>`, see useRouting.ts and
App.tsx `handleSelectProject`). Best practice would be outbox semantics:
rows written only when the set is unreachable, drained by the reconciler
once confirmed in the set (`deleteProjectByIndexDocId` is already the
drain mechanism).

Staged path when revisited:

1. Route by `indexDocId` instead of the local row uuid, with a legacy
   fallback so existing bookmarks/history still resolve (touches
   utils/routing, App.tsx, ProjectSelector, ProjectsHome, share handler).
2. Remove the shadow writes (`handleOpen` in ProjectsHome,
   ProjectSelector open paths) that the routing coupling forces.
3. Reconciler drains rows after `addProjectsBulk` confirms them.
4. Optional: drain post-migration backup rows after the pointer commit;
   v6 migration normalizing `automerge:`-prefix duality.

Why tombstones stay even with an outbox: a genuinely pending row on an
offline browser (share link clicked while disconnected) cannot be
ordered against a deletion made on another browser without a deletion
record. The outbox removes the cause of stale rows; tombstones make
deletions ordered events. Same-browser correctness is achievable with
the outbox alone; cross-device is not.
