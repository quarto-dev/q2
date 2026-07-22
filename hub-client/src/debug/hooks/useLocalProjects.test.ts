/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import 'fake-indexeddb/auto'
import { IDBFactory } from 'fake-indexeddb'
import { renderHook, waitFor } from '@testing-library/react'
import { openDB } from 'idb'
import { DB_NAME, STORES } from '../../services/storage/types'
import { _resetLocalDbCacheForTesting } from '../services/localProjects'
import { useLocalProjects } from './useLocalProjects'

async function seed(
  projects: unknown[],
  collections?: Array<{ projectSetDocId: string; syncServer: string }>,
) {
  const db = await openDB(DB_NAME, 1, {
    upgrade(db) {
      db.createObjectStore(STORES.PROJECTS, { keyPath: 'id' })
      if (collections) {
        db.createObjectStore(STORES.PROJECT_SET, { keyPath: 'key' })
      }
    },
  })
  for (const p of projects) {
    await db.put(STORES.PROJECTS, p)
  }
  if (collections) {
    await db.put(STORES.PROJECT_SET, { key: 'collections', collections })
  }
  db.close()
}

describe('useLocalProjects', () => {
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

  it('starts in the loading state and transitions to the loaded state', async () => {
    await seed([
      {
        id: 'local-1',
        indexDocId: 'abc',
        syncServer: 'wss://h',
        description: 'Proj',
        createdAt: '2026-01-01',
        lastAccessed: '2026-04-10',
      },
    ])

    const { result } = renderHook(() => useLocalProjects())
    expect(result.current.loading).toBe(true)

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.projects).toHaveLength(1)
    expect(result.current.projects[0]?.indexDocId).toBe('abc')
    expect(result.current.projectSetPointer).toBeNull()
  })

  it('reports empty results when the database is untouched', async () => {
    const { result } = renderHook(() => useLocalProjects())
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.projects).toEqual([])
    expect(result.current.projectSetPointer).toBeNull()
    expect(result.current.collectionPointers).toEqual([])
  })

  it('surfaces every collection pointer so each synced doc is inspectable', async () => {
    await seed([], [
      { projectSetDocId: 'root-doc', syncServer: 'wss://h' },
      { projectSetDocId: 'team-doc', syncServer: 'wss://h' },
    ])

    const { result } = renderHook(() => useLocalProjects())
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.collectionPointers.map((c) => c.projectSetDocId)).toEqual([
      'root-doc',
      'team-doc',
    ])
  })
})
