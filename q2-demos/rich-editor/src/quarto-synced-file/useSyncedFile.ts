/**
 * React binding for `createSyncedFile`. Creates the synced file when params
 * are given, disposes it when they change or the component unmounts.
 */

import { useEffect, useState, useSyncExternalStore } from 'react'
import type { EditorContentChange } from '@quarto/quarto-sync-client'
import {
  createSyncedFile,
  idleSnapshot,
  type SyncedFile,
  type SyncedFileParams,
  type SyncedFileSnapshot,
} from './syncedFile.ts'

export interface SyncedFileState extends SyncedFileSnapshot {
  /** The file itself, for code that subscribes on its own. Null until created. */
  file: SyncedFile | null
  /** Splice edits into the synced text. Null when not connected. */
  applyEdits: ((changes: EditorContentChange[]) => void) | null
}

const subscribeNone = () => () => {}
const getIdleSnapshot = () => idleSnapshot

export function useSyncedFile(params: SyncedFileParams | null): SyncedFileState {
  const [file, setFile] = useState<SyncedFile | null>(null)

  useEffect(() => {
    if (!params) return
    const created = createSyncedFile(params)
    setFile(created)
    return () => {
      created.dispose()
      setFile(null)
    }
  }, [params?.syncServer, params?.indexDocId, params?.filePath])

  const snapshot = useSyncExternalStore(
    file ? file.subscribe : subscribeNone,
    file ? file.getSnapshot : getIdleSnapshot,
  )

  return { ...snapshot, file, applyEdits: file && snapshot.connected ? file.applyEdits : null }
}
