import { useEffect, useState } from 'react'
import {
  listLocalProjects,
  getLocalProjectSetPointer,
  getLocalCollectionPointers,
} from '../services/localProjects'
import type { ProjectEntry } from '@quarto/preview-renderer/types/project'
import type { ProjectSetPointer, CollectionPointerEntry } from '../../services/storage/types'

export interface LocalProjectsState {
  loading: boolean
  projects: ProjectEntry[]
  projectSetPointer: ProjectSetPointer | null
  collectionPointers: CollectionPointerEntry[]
  error?: string
}

/**
 * Load the local project list and project-set pointer from IndexedDB.
 *
 * Runs once on mount. The helpers do not write to IndexedDB, so this is
 * safe to invoke from the debug tab even while the main app is running
 * in another tab.
 */
export function useLocalProjects(): LocalProjectsState {
  const [state, setState] = useState<LocalProjectsState>({
    loading: true,
    projects: [],
    projectSetPointer: null,
    collectionPointers: [],
  })

  useEffect(() => {
    let cancelled = false
    Promise.all([
      listLocalProjects(),
      getLocalProjectSetPointer(),
      getLocalCollectionPointers(),
    ])
      .then(([projects, projectSetPointer, collectionPointers]) => {
        if (cancelled) return
        setState({ loading: false, projects, projectSetPointer, collectionPointers })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        const message = err instanceof Error ? err.message : String(err)
        setState({
          loading: false,
          projects: [],
          projectSetPointer: null,
          collectionPointers: [],
          error: message,
        })
      })
    return () => {
      cancelled = true
    }
  }, [])

  return state
}
