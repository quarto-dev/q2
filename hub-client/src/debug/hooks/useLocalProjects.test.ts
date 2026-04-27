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

async function seed(projects: unknown[]) {
  const db = await openDB(DB_NAME, 1, {
    upgrade(db) {
      db.createObjectStore(STORES.PROJECTS, { keyPath: 'id' })
    },
  })
  for (const p of projects) {
    await db.put(STORES.PROJECTS, p)
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
  })
})
