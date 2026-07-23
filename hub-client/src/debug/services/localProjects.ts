/**
 * Read-only IndexedDB access for the debug page's quick-pick panel.
 *
 * The debug page intentionally does NOT use the main app's getDb() helper,
 * because that runs schema migrations and registers upgrade callbacks that
 * could mutate the shared database. Instead, we open the database without
 * a version number (so idb opens whatever version exists without triggering
 * an upgrade) and guard all reads behind `objectStoreNames.contains(...)`.
 *
 * The debug page has no need to read user settings or any mutable state —
 * it just wants the project list and the project-set pointer so it can
 * pre-populate the Subscribe input.
 */

import { openDB, type IDBPDatabase } from 'idb'
import {
  DB_NAME,
  STORES,
  type ProjectSetPointer,
  type CollectionsPointer,
  type CollectionPointerEntry,
} from '../../services/storage/types'
import type { ProjectEntry } from '@quarto/preview-renderer/types/project'

let dbPromise: Promise<IDBPDatabase> | null = null

async function getReadOnlyDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    // No version number → open current version, no upgrade callback,
    // no migrations. If the DB doesn't exist yet it will be created at
    // version 1 with no object stores, which our store-existence checks
    // handle gracefully.
    dbPromise = openDB(DB_NAME)
  }
  return dbPromise
}

/**
 * List all projects stored locally. Returns an empty array if the database
 * or projects store does not exist.
 */
export async function listLocalProjects(): Promise<ProjectEntry[]> {
  const db = await getReadOnlyDb()
  if (!db.objectStoreNames.contains(STORES.PROJECTS)) {
    return []
  }
  const tx = db.transaction(STORES.PROJECTS, 'readonly')
  const all = (await tx.objectStore(STORES.PROJECTS).getAll()) as ProjectEntry[]
  // Sort by lastAccessed desc for a natural quick-pick order. Entries without
  // lastAccessed are pushed to the end.
  return all.sort((a, b) => {
    const aTime = a.lastAccessed ?? ''
    const bTime = b.lastAccessed ?? ''
    if (!aTime) return 1
    if (!bTime) return -1
    return bTime.localeCompare(aTime)
  })
}

/**
 * Return the singleton project-set pointer if present, else null.
 */
export async function getLocalProjectSetPointer(): Promise<ProjectSetPointer | null> {
  const db = await getReadOnlyDb()
  if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) {
    return null
  }
  const entry = await db.get(STORES.PROJECT_SET, 'projectSet')
  return (entry as ProjectSetPointer | undefined) ?? null
}

/**
 * Return the collections pointer array. Each entry is its own synced
 * Automerge `ProjectSetDocument` (the root plus every named collection), so
 * the debug page can list and subscribe to them individually.
 *
 * Returns `[]` when the `collections` record is absent — e.g. a browser that
 * predates the v5 pointer→collections migration, whose root still lives in the
 * legacy singleton (see `getLocalProjectSetPointer`). Unlike the app's
 * `getCollectionPointers`, this is strictly read-only: it does NOT run the
 * migration or self-heal, because the debug page must not write to the shared
 * database.
 */
export async function getLocalCollectionPointers(): Promise<CollectionPointerEntry[]> {
  const db = await getReadOnlyDb()
  if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) {
    return []
  }
  const entry = (await db.get(STORES.PROJECT_SET, 'collections')) as
    | CollectionsPointer
    | undefined
  return entry?.collections ?? []
}

/** @internal Test-only helper to reset the cached DB promise between tests. */
export function _resetLocalDbCacheForTesting(): void {
  dbPromise = null
}
