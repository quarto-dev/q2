/**
 * IndexedDB-based storage for connection entries.
 *
 * Simplified version of hub-client's projectStorage — no migrations,
 * no import/export, no schema versioning. Single DB version.
 */
import { openDB } from 'idb'
import type { IDBPDatabase } from 'idb'

export interface ConnectionEntry {
  id: string
  syncServer: string
  indexDocId: string
  filePath: string
  description: string
  createdAt: string
  lastAccessed: string
}

const DB_NAME = 'kanban-connections'
const STORE_NAME = 'connections'
const DB_VERSION = 1

let dbPromise: Promise<IDBPDatabase> | null = null

function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = openDB(DB_NAME, DB_VERSION, {
      upgrade(db) {
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          const store = db.createObjectStore(STORE_NAME, { keyPath: 'id' })
          store.createIndex('lastAccessed', 'lastAccessed')
        }
      },
    })
  }
  return dbPromise
}

export async function listConnections(): Promise<ConnectionEntry[]> {
  const db = await getDb()
  const tx = db.transaction(STORE_NAME, 'readonly')
  const index = tx.objectStore(STORE_NAME).index('lastAccessed')
  const entries = await index.getAll()
  return entries.reverse()
}

export async function getConnection(id: string): Promise<ConnectionEntry | undefined> {
  const db = await getDb()
  return db.get(STORE_NAME, id)
}

export async function addConnection(
  syncServer: string,
  indexDocId: string,
  filePath: string,
  description?: string,
): Promise<ConnectionEntry> {
  const now = new Date().toISOString()
  const entry: ConnectionEntry = {
    id: crypto.randomUUID(),
    syncServer,
    indexDocId,
    filePath,
    description: description || `${filePath} @ ${syncServer}`,
    createdAt: now,
    lastAccessed: now,
  }
  const db = await getDb()
  await db.put(STORE_NAME, entry)
  return entry
}

export async function touchConnection(id: string): Promise<void> {
  const db = await getDb()
  const entry = await db.get(STORE_NAME, id)
  if (entry) {
    entry.lastAccessed = new Date().toISOString()
    await db.put(STORE_NAME, entry)
  }
}

export async function deleteConnection(id: string): Promise<void> {
  const db = await getDb()
  await db.delete(STORE_NAME, id)
}
