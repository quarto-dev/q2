/**
 * Read-only enumeration of Automerge document IDs persisted by the main
 * hub-client app. Used by the debug page's "Local IndexedDB" storage
 * mode to populate the quick-pick list from what's actually on disk.
 *
 * The `automerge` IndexedDB database is owned by
 * `@automerge/automerge-repo-storage-indexeddb` with its default
 * constructor args (database="automerge", store="documents"). Each
 * record's key is a `StorageKey` array whose first element is the
 * Automerge document ID; subsequent elements disambiguate snapshot vs.
 * incremental chunks. Enumerating unique first-elements gives the set
 * of documents present locally.
 */

import { openDB, type IDBPDatabase } from 'idb'

const AUTOMERGE_DB = 'automerge'
const STORE = 'documents'

let dbPromise: Promise<IDBPDatabase> | null = null

async function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    // No version number: open whatever exists, do not upgrade. If the DB
    // does not yet exist the idb library creates an empty one with no
    // stores — `listLocalStoredDocumentIds` guards against that below.
    dbPromise = openDB(AUTOMERGE_DB)
  }
  return dbPromise
}

export async function listLocalStoredDocumentIds(): Promise<string[]> {
  const db = await getDb()
  if (!db.objectStoreNames.contains(STORE)) return []

  const tx = db.transaction(STORE, 'readonly')
  const store = tx.objectStore(STORE)
  const uniqueIds = new Set<string>()

  let cursor = await store.openKeyCursor()
  while (cursor) {
    const key = cursor.key
    if (Array.isArray(key) && typeof key[0] === 'string') {
      uniqueIds.add(key[0])
    }
    cursor = await cursor.continue()
  }

  return [...uniqueIds].sort()
}

/** @internal Test-only helper to reset the cached DB promise. */
export function _resetLocalStoredDocsCache(): void {
  dbPromise = null
}
