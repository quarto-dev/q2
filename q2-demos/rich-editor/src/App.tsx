/**
 * Root app component. Connection params come from the URL:
 *
 *   ?server=ws://127.0.0.1:8080/ws&doc=<index-doc-id>&file=index.qmd
 *
 * `server` defaults to the local-prod hub (`npm run local-prod` in hub-client).
 * The visual editor fills the page; the toolbar's Developer tools button
 * shows the source pane beside it.
 */

import { useMemo, useState, type ReactNode } from 'react'
import { useSyncedFile } from './quarto-synced-file/index.ts'
import { Editor } from './Editor.tsx'
import { VisualEditor } from './VisualEditor.tsx'

const DEFAULT_SERVER = 'ws://127.0.0.1:8080/ws'

function readParams() {
  const q = new URLSearchParams(window.location.search)
  const indexDocId = q.get('doc')
  const filePath = q.get('file')
  if (!indexDocId || !filePath) return null
  return {
    syncServer: q.get('server') ?? DEFAULT_SERVER,
    indexDocId,
    filePath,
  }
}

export function App() {
  const params = useMemo(readParams, [])
  const { file, text, parseError, connected, connecting, error, applyEdits } = useSyncedFile(params)
  const [devToolsOpen, setDevToolsOpen] = useState(false)

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
  // Only before the document first loads: after that, the editor stays
  // and keeps working while the connection is down (its status says so).
  if (!connected && text === null) return <Status>Disconnected.</Status>
  if (text === null || !file) return <Status>Waiting for {params.filePath}…</Status>

  const devTools = { open: devToolsOpen, toggle: () => setDevToolsOpen((o) => !o) }
  return (
    <div className={devToolsOpen ? 'panes' : 'panes panes-single'}>
      <VisualEditor file={file} devTools={devTools} />
      {devToolsOpen && <Editor text={text} parseError={parseError} onEdit={applyEdits} />}
    </div>
  )
}

function Status({ children, error }: { children: ReactNode; error?: boolean }) {
  return <p className={error ? 'status status-error' : 'status'}>{children}</p>
}
