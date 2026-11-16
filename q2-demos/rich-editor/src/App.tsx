/**
 * Root app component. Connection params come from the URL:
 *
 *   ?server=<ws-url>&doc=<index-doc-id>&file=index.qmd
 *
 * `server` defaults to the public automerge sync server, and `file` to
 * `index.qmd`. Without `doc`, the page makes a new project on the server
 * from the tutorial (`tutorial.qmd`), and puts it in the URL, so that
 * reloading opens it again and the URL can be shared.
 * The visual editor fills the page; the toolbar's Developer tools button
 * shows the source pane beside it.
 */

import { useMemo, useState, type ReactNode } from 'react'
import { useSyncedFile, type SyncedFileParams } from './quarto-synced-file/index.ts'
import { Editor } from './Editor.tsx'
import { VisualEditor } from './VisualEditor.tsx'
import tutorial from './tutorial.qmd?raw'

const DEFAULT_SERVER = 'wss://sync.automerge.org'
const DEFAULT_FILE = 'index.qmd'

function readParams(): SyncedFileParams {
  const q = new URLSearchParams(window.location.search)
  const syncServer = q.get('server') ?? DEFAULT_SERVER
  const indexDocId = q.get('doc')
  const filePath = q.get('file') ?? DEFAULT_FILE
  if (indexDocId) return { syncServer, indexDocId, filePath }
  return {
    syncServer,
    indexDocId: null,
    filePath,
    create: {
      content: tutorial,
      // In the URL, without reloading: the page goes on with the client
      // that made it.
      onCreated(doc) {
        const query = new URLSearchParams({ server: syncServer, doc, file: filePath })
        window.history.replaceState(null, '', `${window.location.pathname}?${query}`)
      },
    },
  }
}

export function App() {
  const params = useMemo(readParams, [])
  const { file, text, parseError, connected, connecting, error, applyEdits } = useSyncedFile(params)
  const [devToolsOpen, setDevToolsOpen] = useState(false)

  if (error) return <Status error>{error}</Status>
  if (connecting) {
    return (
      <Status>
        {params.indexDocId === null ? 'Making a document on' : 'Connecting to'} {params.syncServer}…
      </Status>
    )
  }
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
