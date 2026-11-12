/**
 * One synced qmd file, with the same data and edit API that hub-client's
 * editor uses. Framework-free: subscribe to changes and read snapshots.
 *
 * - `text`: the file's source text from its automerge document, updated on
 *   every local or remote change (`onFileAdded` / `onFileChanged`).
 * - `ast`: the WASM parse of `text`. When the text doesn't parse, the last
 *   good AST is kept and `parseError` is set.
 * - `applyEdits`: positional splices into the automerge text, the same call
 *   hub-client makes for every Monaco edit (`applyEditorOperations`). Changes
 *   are applied in order, each against the text left by the previous one.
 *
 * Text-to-AST is one-way here, as in hub-client: to change the document from
 * an edited AST, write qmd with `incrementalWriteQmd` and send the difference
 * as splices (`diffToChange`).
 */

import { createSyncClient, type EditorContentChange } from '@quarto/quarto-sync-client'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { initWasm, parseQmdContent } from './wasm.ts'

export interface SyncedFileParams {
  syncServer: string
  indexDocId: string
  filePath: string
}

export interface SyncedFileSnapshot {
  text: string | null
  ast: RustQmdJson | null
  parseError: boolean
  connected: boolean
  connecting: boolean
  error: string | null
}

export interface SyncedFile {
  /** Register a listener for snapshot changes; returns an unsubscribe function. */
  subscribe(listener: () => void): () => void
  /** The current snapshot. Stable until the next change. */
  getSnapshot(): SyncedFileSnapshot
  /** Splice edits into the synced text. Ignored while not connected. */
  applyEdits(changes: EditorContentChange[]): void
  /** Disconnect and stop notifying listeners. */
  dispose(): void
}

export const idleSnapshot: SyncedFileSnapshot = {
  text: null,
  ast: null,
  parseError: false,
  connected: false,
  connecting: false,
  error: null,
}

export function createSyncedFile({ syncServer, indexDocId, filePath }: SyncedFileParams): SyncedFile {
  let snapshot: SyncedFileSnapshot = { ...idleSnapshot, connecting: true }
  let disposed = false
  const listeners = new Set<() => void>()

  function update(patch: Partial<SyncedFileSnapshot>) {
    if (disposed) return
    snapshot = { ...snapshot, ...patch }
    for (const listener of listeners) listener()
  }

  function setText(text: string | null) {
    if (text === null) {
      update({ text: null, parseError: false })
      return
    }
    const parsed = parseQmdContent(text)
    update({ text, ast: parsed ?? snapshot.ast, parseError: parsed === null })
  }

  const client = createSyncClient({
    onFileAdded: (path, file) => {
      if (path === filePath && file.type === 'text') setText(file.text)
    },
    onFileChanged: (path, text) => {
      if (path === filePath) setText(text)
    },
    onBinaryChanged: () => {},
    onFileRemoved: (path) => {
      if (path === filePath) setText(null)
    },
    onConnectionChange: (connected) => update({ connected }),
    onError: (err) => update({ error: err.message }),
  })

  async function connect() {
    try {
      await initWasm()
    } catch (e) {
      update({ error: `Failed to initialize WASM: ${e}`, connecting: false })
      return
    }
    if (disposed) return

    try {
      await client.connect(syncServer, indexDocId)
      if (disposed) return
      // The file may have loaded before our callbacks saw it.
      if (snapshot.text === null) setText(client.getFileContent(filePath))
      update({ connecting: false })
    } catch (e) {
      update({
        error: `Connection failed: ${e instanceof Error ? e.message : String(e)}`,
        connecting: false,
      })
    }
  }

  connect()

  return {
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    getSnapshot: () => snapshot,
    applyEdits(changes) {
      if (disposed || !snapshot.connected) return
      client.applyEditorOperations(filePath, changes)
    },
    dispose() {
      disposed = true
      listeners.clear()
      client.disconnect()
    },
  }
}
