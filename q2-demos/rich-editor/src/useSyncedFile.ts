/**
 * React hook giving one synced qmd file the same data and edit API that
 * hub-client's editor uses:
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
 * as splices.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { createSyncClient, type EditorContentChange } from '@quarto/quarto-sync-client'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { initWasm, parseQmdContent } from './wasm.ts'

export interface SyncedFileParams {
  syncServer: string
  indexDocId: string
  filePath: string
}

export interface SyncedFileState {
  text: string | null
  ast: RustQmdJson | null
  parseError: boolean
  connected: boolean
  connecting: boolean
  error: string | null
  /** Splice edits into the synced text. Null when not connected. */
  applyEdits: ((changes: EditorContentChange[]) => void) | null
}

export function useSyncedFile(params: SyncedFileParams | null): SyncedFileState {
  const [text, setText] = useState<string | null>(null)
  const [connected, setConnected] = useState(false)
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const clientRef = useRef<ReturnType<typeof createSyncClient> | null>(null)
  const filePathRef = useRef<string | null>(null)

  const applyEdits = useCallback((changes: EditorContentChange[]) => {
    const client = clientRef.current
    const path = filePathRef.current
    if (!client || !path) return
    client.applyEditorOperations(path, changes)
  }, [])

  useEffect(() => {
    if (!params) return

    let cancelled = false
    const { syncServer, indexDocId, filePath } = params
    filePathRef.current = filePath

    async function connect() {
      setConnecting(true)
      setError(null)

      try {
        await initWasm()
      } catch (e) {
        if (!cancelled) {
          setError(`Failed to initialize WASM: ${e}`)
          setConnecting(false)
        }
        return
      }
      if (cancelled) return

      const client = createSyncClient({
        onFileAdded: (path, file) => {
          if (!cancelled && path === filePath && file.type === 'text') setText(file.text)
        },
        onFileChanged: (path, newText) => {
          if (!cancelled && path === filePath) setText(newText)
        },
        onBinaryChanged: () => {},
        onFileRemoved: (path) => {
          if (!cancelled && path === filePath) setText(null)
        },
        onConnectionChange: (isConnected) => {
          if (!cancelled) setConnected(isConnected)
        },
        onError: (err) => {
          if (!cancelled) setError(err.message)
        },
      })
      clientRef.current = client

      try {
        await client.connect(syncServer, indexDocId)
        if (cancelled) return
        // The file may have loaded before our callbacks saw it.
        setText((current) => current ?? client.getFileContent(filePath))
        setConnecting(false)
      } catch (e) {
        if (!cancelled) {
          setError(`Connection failed: ${e instanceof Error ? e.message : String(e)}`)
          setConnecting(false)
        }
      }
    }

    connect()

    return () => {
      cancelled = true
      clientRef.current?.disconnect()
      clientRef.current = null
    }
  }, [params?.syncServer, params?.indexDocId, params?.filePath])

  const lastGoodAst = useRef<RustQmdJson | null>(null)
  const { ast, parseError } = useMemo(() => {
    if (text === null) return { ast: null, parseError: false }
    const parsed = parseQmdContent(text)
    if (parsed) lastGoodAst.current = parsed
    return { ast: lastGoodAst.current, parseError: parsed === null }
  }, [text])

  return {
    text,
    ast,
    parseError,
    connected,
    connecting,
    error,
    applyEdits: connected ? applyEdits : null,
  }
}
