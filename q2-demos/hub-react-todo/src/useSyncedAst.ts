/**
 * React hook for AST-synced documents.
 *
 * Connects to a sync server, subscribes to a file, and provides
 * the latest successfully parsed AST as React state.
 */

import { useState, useEffect, useRef } from 'react'
import { createSyncClient } from '@quarto/quarto-sync-client'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { initWasm, parseQmdContent, writeQmdFromAst } from './wasm.ts'

export interface SyncedAstState {
  ast: RustQmdJson | null
  connected: boolean
  error: string | null
  connecting: boolean
}

interface SyncedAstParams {
  syncServer: string
  indexDocId: string
  filePath: string
}

export function useSyncedAst(params: SyncedAstParams | null): SyncedAstState {
  const [ast, setAst] = useState<RustQmdJson | null>(null)
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [connecting, setConnecting] = useState(false)

  // Use ref for the client so we can disconnect on cleanup
  const clientRef = useRef<ReturnType<typeof createSyncClient> | null>(null)

  useEffect(() => {
    if (!params) return

    let cancelled = false
    const { syncServer, indexDocId, filePath } = params

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

      const client = createSyncClient(
        {
          onFileAdded: () => {},
          onFileChanged: () => {},
          onBinaryChanged: () => {},
          onFileRemoved: () => {},
          onConnectionChange: (isConnected) => {
            if (!cancelled) setConnected(isConnected)
          },
          onError: (err) => {
            if (!cancelled) setError(err.message)
          },
          onASTChanged: (path, astValue) => {
            if (!cancelled && path === filePath) {
              setAst(astValue as RustQmdJson)
            }
          },
        },
        {
          parseQmd: (content: string) => parseQmdContent(content),
          writeQmd: (astValue: unknown) => writeQmdFromAst(astValue as RustQmdJson),
          fileFilter: (path: string) => path === filePath,
        },
      )

      clientRef.current = client

      try {
        await client.connect(syncServer, indexDocId)
        if (!cancelled) {
          setConnecting(false)
        }
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

  return { ast, connected, error, connecting }
}
