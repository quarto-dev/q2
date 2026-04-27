/**
 * Tests for enumerating Automerge document IDs persisted to the shared
 * `automerge` IndexedDB database (same DB the hub-client's sync-client
 * writes to, same DB automerge-repo-storage-indexeddb uses by default).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import 'fake-indexeddb/auto'
import { IDBFactory } from 'fake-indexeddb'
import { openDB } from 'idb'
import {
  listLocalStoredDocumentIds,
  _resetLocalStoredDocsCache,
} from './localStoredDocs'

const AUTOMERGE_DB = 'automerge'
const STORE = 'documents'

async function seedAutomergeDb(entries: Array<{ key: unknown[]; binary: Uint8Array }>) {
  const db = await openDB(AUTOMERGE_DB, 1, {
    upgrade(db) {
      db.createObjectStore(STORE)
    },
  })
  for (const { key, binary } of entries) {
    await db.put(STORE, { key, binary }, key as IDBValidKey)
  }
  db.close()
}

describe('listLocalStoredDocumentIds', () => {
  beforeEach(() => {
    _resetLocalStoredDocsCache()
    Object.defineProperty(globalThis, 'indexedDB', {
      value: new IDBFactory(),
      writable: true,
    })
  })

  afterEach(() => {
    _resetLocalStoredDocsCache()
  })

  it('returns an empty list when the automerge DB does not exist', async () => {
    const ids = await listLocalStoredDocumentIds()
    expect(ids).toEqual([])
  })

  it('extracts unique document IDs from stored keys', async () => {
    await seedAutomergeDb([
      { key: ['doc-1', 'snapshot', 'a'], binary: new Uint8Array([1]) },
      { key: ['doc-1', 'incremental', 'b'], binary: new Uint8Array([2]) },
      { key: ['doc-2', 'snapshot', 'a'], binary: new Uint8Array([3]) },
      { key: ['doc-3'], binary: new Uint8Array([4]) },
    ])

    const ids = await listLocalStoredDocumentIds()
    expect([...ids].sort()).toEqual(['doc-1', 'doc-2', 'doc-3'])
  })

  it('returns empty list when the documents store is missing', async () => {
    // Create the DB without the documents store
    const db = await openDB(AUTOMERGE_DB, 1, {
      upgrade() {
        /* intentionally empty — no store */
      },
    })
    db.close()

    const ids = await listLocalStoredDocumentIds()
    expect(ids).toEqual([])
  })

  it('does not create any object stores when opening a missing DB (read-only)', async () => {
    await listLocalStoredDocumentIds()
    // Verify no `documents` store was created as a side effect.
    const db = await openDB(AUTOMERGE_DB)
    expect(db.objectStoreNames.contains(STORE)).toBe(false)
    db.close()
  })
})
