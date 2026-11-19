/**
 * The visual editor on a file on this machine, in the desktop app
 * (`src-tauri/`): no sync server. The app's Rust side reads the file,
 * writes the editor's edits to it, and reports changes made to it by
 * anything else, which the editor merges with its own (`diskFile.ts`).
 */

import { StrictMode, useEffect, useState, useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { initWasm } from '../quarto-synced-file/index.ts'
import { createDiskFile, type DiskFile } from '../quarto-synced-file/diskFile.ts'
import { VisualEditor } from '../VisualEditor.tsx'
import { Editor } from '../Editor.tsx'
import '../index.css'

interface OpenedFile {
  path: string
  name: string
  text: string
}

function DesktopApp() {
  const [file, setFile] = useState<DiskFile | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let disposed = false
    let created: DiskFile | null = null
    let unlisten: (() => void) | null = null
    ;(async () => {
      await initWasm()
      const opened = await invoke<OpenedFile>('open_file')
      if (disposed) return
      await getCurrentWindow().setTitle(opened.name)
      created = createDiskFile(opened.text, (text) => invoke('save_file', { text }))
      unlisten = await listen<string>('file-changed', (event) => created?.applyDiskText(event.payload))
      if (disposed) {
        unlisten()
        return
      }
      setFile(created)
    })().catch((e) => setError(e instanceof Error ? e.message : String(e)))
    return () => {
      disposed = true
      unlisten?.()
      created?.dispose()
    }
  }, [])

  if (error) return <p className="status status-error">{error}</p>
  if (!file) return <p className="status">Loading…</p>
  return <Panes file={file} />
}

function Panes({ file }: { file: DiskFile }) {
  const { text, parseError, error } = useSyncExternalStore(file.subscribe, file.getSnapshot)
  const [devToolsOpen, setDevToolsOpen] = useState(false)
  if (error) return <p className="status status-error">{error}</p>
  const devTools = { open: devToolsOpen, toggle: () => setDevToolsOpen((o) => !o) }
  return (
    <div className={devToolsOpen ? 'panes' : 'panes panes-single'}>
      <VisualEditor file={file} devTools={devTools} />
      {devToolsOpen && text !== null && <Editor text={text} parseError={parseError} onEdit={file.applyEdits} />}
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <DesktopApp />
  </StrictMode>,
)
