# Collections as project sets

## Overview

Re-founder the projects-home "collections" (branch `explore/projects-collections-ui`)
on real synced documents, per the 2026-07-09 design discussion (Andrew, Carlos,
Elliot). Today a collection is a localStorage grouping of indexDocIds inside a
single ProjectSetDocument world; membership, sharing, and invites are mock.
After this change:

- **Each collection IS a ProjectSetDocument** (same schema, plus an optional
  `name`). We reuse all existing set logic: entries, dedupe keys, sync,
  link-another-browser.
- **The browser's root pointer becomes an array**: IndexedDB goes from one
  `{projectSetDocId, syncServer}` pointer to a list of collection pointers.
  "The root of all your information" is now a set of project sets.
- **Sharing a collection = sharing its doc id**, exactly like today's
  link-project-set flow. The join-collection invite flow stops being theater:
  joining appends the collection's doc id to your array.
- **Real contributor identities** come from each project index doc's
  `identities` map (per Carlos: the blessed, self-populating source), not
  presence-at-open.

Everything about individual projects (index docs, files, share links) is
untouched. "Minimum blast surface."

## Checklist

### Phase 1 — schema + tests
- [x] `ProjectSetDocument` gains optional `name?: string` (collection display
      name) with helper `setProjectSetName` + tests
- [x] New IDB shape: `CollectionsPointer { key: 'collections', collections:
      Array<{ projectSetDocId, syncServer }> }` in storage types
- [x] IDB migration 4→5 (structural no-op, transform converts the singleton
      `projectSet` pointer into a one-element `collections` array; old pointer
      kept as safety net, never deleted by the migration)
- [x] Migration tests: fresh install, existing pointer, re-run idempotence

### Phase 2 — service layer
- [x] `projectSetService` → manages a MAP of doc handles keyed by docId
      (connect-all on startup; add/remove collection connections at runtime)
- [x] Operations gain a collection-doc parameter: addProject(collectionId,
      entry), removeProject, updateSummary, touch, rename collection
- [x] Create-collection (new empty set doc), subscribe-collection (existing
      doc id), unsubscribe (drop from array; doc untouched — "leave")
- [x] `useProjectSet` → `useCollectionSets`: exposes
      `Array<{ docId, name, entries, syncServer }>` + actions

### Phase 3 — localStorage collections migration
- [x] On first load with the new code: for each entry in `qh-collections-v1`,
      create a new collection doc named after it, copy the matching entries
      from the personal set into it, append to the pointer array
- [x] The original set becomes the personal root collection (name it
      "My projects" when the name field is empty; user-editable)
- [x] Mark `qh-collections-v1` migrated (rename key, don't delete) — one-way,
      idempotent, safe to interrupt (commit point = pointer-array write)

### Phase 4 — UI rewiring
- [x] ProjectsHome reads collections from the hook instead of localStorage;
      "Everything else" = personal-root entries not present in any other
      collection (display-level; the root stays a superset so nothing is
      ever lost)
- [x] Move keeps current semantics (remove+add across docs); new "Add to
      collection" menu item adds without removing (multi-membership is now
      natural — entries in several sets)
- [x] Share/members popover backed by the real doc: invite link carries the
      collection doc id + server only (entries no longer inlined in the URL)
- [x] JoinCollectionLanding: real subscribe (append docId to pointer array),
      auto-create personal root for fresh browsers as today
- [x] Facepiles/peek contributors read from project index-doc `identities`
      (one-shot read piggybacked on the existing summary write; peek refresh
      also picks them up)

### Phase 5 — verification
- [x] `npm run build:all` + full `npm run test:ci`
- [x] Manual: fresh browser, upgraded browser (with and without localStorage
      collections), two-browser share/join round trip, debug.html inspection
      of resulting docs

## Details and open questions

**Everything-else semantics.** Keeping the personal root as a superset of all
your projects (and filtering the home display) means deleting a shared
collection can never orphan a project you've opened, matching "no action
deletes entries for people who are part of a collection." Alternative — root
holds only unfiled projects — is cleaner data but loses the safety property.
Recommend superset. **Flag for Carlos.**

**Summaries per entry.** The peek `summary` cache rides on entries, so a
project in three collections has three copies. Acceptable duplication for now;
revisit when identities move to index docs (Phase 4 reduces what summary needs
to carry).

**Members display.** A shared collection's facepile can be derived from the
union of its projects' index-doc identities, replacing mock members. A
dedicated `members` map on the collection doc (self-reported on join) is a
possible follow-on, not required for this plan.

**History viewer** (Carlos's ask: when were entries added/removed + restore)
is deliberately out of scope here; it layers on automerge history of the
collection doc afterward. Charlie's history view is prior art.

**Race note.** The current app connects the singleton set on init and the
link-project-set route races it (observed 2026-07-09: link failed to displace
a fresh pointer). The multi-connection refactor should make "append a
collection" not contend with init at all, fixing that class of bug.

## Local-prod verification (2026-07-14)

Ran the full stack via `npm run local-prod` (hub binary + local sync
server on 127.0.0.1:8080, fresh `.local-prod-data`). Verified end to end:
fresh-browser project-set create; new project; new collection ("Team
docs") + drag a project in; invite link is doc-id-only (`server=`, no
`entries=`); a second browser joined via the invite and saw the synced
project; created "meeting agenda" targeted at Team docs on browser 2;
it appeared **live** on browser 1 (no reload). Hub log clean (only the
expected `auth_disabled` warnings under `--allow-insecure-auth`).

Found + fixed one bug: the empty-state guard hid a subscribed collection
when the personal root was still empty (85108dde).

Deferred/known: the People popover shows "only you" until members open
projects (contributors derive from cached summaries; a real members map
is the documented follow-on).

