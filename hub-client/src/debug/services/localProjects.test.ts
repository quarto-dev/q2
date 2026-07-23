/**
 * Tests for local IndexedDB enumeration used by the debug quick-pick panel.
 *
 * The debug page opens the hub-client IndexedDB database in read-only mode
 * (no version upgrade, no migrations) and surfaces project entries + the
 * project-set pointer. These tests verify behavior against a seeded
 * fake-indexeddb instance as well as the empty-DB edge case.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import 'fake-indexeddb/auto'
import { IDBFactory } from 'fake-indexeddb'
import { openDB } from 'idb'
import { DB_NAME, STORES } from '../../services/storage/types'
import {
  listLocalProjects,
  getLocalProjectSetPointer,
  getLocalCollectionPointers,
  _resetLocalDbCacheForTesting,
} from './localProjects'

async function seedDatabase(
  projects: Array<Record<string, unknown>>,
  pointer?: { projectSetDocId: string; syncServer: string },
  collections?: Array<{ projectSetDocId: string; syncServer: string }>,
) {
  const needsProjectSetStore = !!pointer || !!collections
  const db = await openDB(DB_NAME, 1, {
    upgrade(db) {
      db.createObjectStore(STORES.PROJECTS, { keyPath: 'id' })
      // Only create the projectSet store when the caller actually wants to
      // seed a pointer — this lets tests verify the read helper's behavior
      // when the store is missing entirely.
      if (needsProjectSetStore) {
        db.createObjectStore(STORES.PROJECT_SET, { keyPath: 'key' })
      }
    },
  })
  for (const p of projects) {
    await db.put(STORES.PROJECTS, p)
  }
  if (pointer) {
    await db.put(STORES.PROJECT_SET, { key: 'projectSet', ...pointer })
  }
  if (collections) {
    await db.put(STORES.PROJECT_SET, { key: 'collections', collections })
  }
  db.close()
}

describe('localProjects (debug page IndexedDB read helpers)', () => {
  beforeEach(() => {
    _resetLocalDbCacheForTesting()
    Object.defineProperty(globalThis, 'indexedDB', {
      value: new IDBFactory(),
      writable: true,
    })
  })

  afterEach(() => {
    _resetLocalDbCacheForTesting()
  })

  it('returns an empty list when the database has no projects store', async () => {
    // No DB exists yet — opening creates an empty v1 DB with no stores.
    const result = await listLocalProjects()
    expect(result).toEqual([])
  })

  it('returns seeded projects', async () => {
    await seedDatabase([
      {
        id: 'local-1',
        indexDocId: 'abc123',
        syncServer: 'wss://sync.example.com',
        description: 'Project One',
        createdAt: '2026-01-01T00:00:00Z',
        lastAccessed: '2026-04-10T00:00:00Z',
      },
      {
        id: 'local-2',
        indexDocId: 'def456',
        syncServer: 'wss://sync.example.com',
        description: 'Project Two',
        createdAt: '2026-02-01T00:00:00Z',
        lastAccessed: '2026-04-15T00:00:00Z',
      },
    ])

    const result = await listLocalProjects()
    expect(result).toHaveLength(2)
    expect(result.map((p) => p.id).sort()).toEqual(['local-1', 'local-2'])
    expect(result.find((p) => p.id === 'local-1')?.indexDocId).toBe('abc123')
  })

  it('returns null when no project set pointer exists', async () => {
    const pointer = await getLocalProjectSetPointer()
    expect(pointer).toBeNull()
  })

  it('returns the project set pointer when seeded', async () => {
    await seedDatabase([], {
      projectSetDocId: 'pset-xyz',
      syncServer: 'wss://sync.example.com',
    })

    const pointer = await getLocalProjectSetPointer()
    expect(pointer).not.toBeNull()
    expect(pointer!.projectSetDocId).toBe('pset-xyz')
    expect(pointer!.syncServer).toBe('wss://sync.example.com')
  })

  it('returns [] for collection pointers when none are seeded', async () => {
    expect(await getLocalCollectionPointers()).toEqual([])
  })

  it('returns every collection pointer (each its own synced ProjectSetDocument)', async () => {
    await seedDatabase([], undefined, [
      { projectSetDocId: 'root-doc', syncServer: 'wss://s' },
      { projectSetDocId: 'team-doc', syncServer: 'wss://s' },
      { projectSetDocId: 'synctest-doc', syncServer: 'wss://s' },
    ])

    const collections = await getLocalCollectionPointers()
    expect(collections.map((c) => c.projectSetDocId)).toEqual([
      'root-doc',
      'team-doc',
      'synctest-doc',
    ])
  })

  it('does not run the pointer→collections migration (read-only)', async () => {
    // A browser with only the legacy singleton (no collections record yet):
    // the read helper must NOT synthesize/migrate — it just reports [].
    await seedDatabase([], { projectSetDocId: 'legacy-root', syncServer: 'wss://s' })
    expect(await getLocalCollectionPointers()).toEqual([])
  })

  it('does not upgrade or create stores that are missing (read-only)', async () => {
    // Seed only the projects store (no projectSet store at all)
    await seedDatabase([])

    // Requesting the pointer should succeed and return null — it must NOT
    // create the projectSet store, since that would be a write from the
    // debug page to shared IndexedDB.
    const pointer = await getLocalProjectSetPointer()
    expect(pointer).toBeNull()

    // Reopen and verify the projectSet store was never created.
    const db = await openDB(DB_NAME)
    expect(db.objectStoreNames.contains(STORES.PROJECT_SET)).toBe(false)
    db.close()
  })
})
