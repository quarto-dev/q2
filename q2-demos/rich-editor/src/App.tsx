/**
 * Root app component. Connection params come from the URL:
 *
 *   ?server=ws://127.0.0.1:8080/ws&doc=<index-doc-id>&file=index.qmd
 *
 * `server` defaults to the local-prod hub (`npm run local-prod` in hub-client).
 */

import { useMemo, type ReactNode } from 'react'
import { useSyncedFile } from './useSyncedFile.ts'
import { Editor } from './Editor.tsx'

const DEFAULT_SERVER = 'ws://127.0.0.1:8080/ws'

function readParams() {
  const q = new URLSearchParams(window.location.search)
  const indexDocId = q.get('doc')
  const filePath = q.get('file')
  if (!indexDocId || !filePath) return null
  return { syncServer: q.get('server') ?? DEFAULT_SERVER, indexDocId, filePath }
}

export function App() {
  const params = useMemo(readParams, [])
  const { text, ast, parseError, connected, connecting, error, applyEdits } = useSyncedFile(params)

  if (!params) {
    return (
      <Status>
        Add <code>?doc=&lt;index-doc-id&gt;&amp;file=&lt;path.qmd&gt;</code> to the URL
        (optionally <code>&amp;server=&lt;ws-url&gt;</code>, default <code>{DEFAULT_SERVER}</code>).
      </Status>
    )
  }
  if (error) return <Status error>{error}</Status>
  if (connecting) return <Status>Connecting to {params.syncServer}…</Status>
  if (!connected) return <Status>Disconnected.</Status>
  if (text === null) return <Status>Waiting for {params.filePath}…</Status>

  return <Editor text={text} ast={ast} parseError={parseError} onEdit={applyEdits} />
}

function Status({ children, error }: { children: ReactNode; error?: boolean }) {
  return <p className={error ? 'status status-error' : 'status'}>{children}</p>
}
