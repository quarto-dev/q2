/**
 * The visual editor on a fixture document held in memory, with no hub:
 * for trying the editor without a server. (The e2e tests use a hub.)
 *
 *   /fixture.html?fixture=<name>   loads e2e/fixtures/<name>.qmd
 *
 * The fixture name defaults to `basic`.
 */

import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { createLocalFile, initWasm, type SyncedFile } from '../quarto-synced-file/index.ts'
import { VisualEditor } from '../VisualEditor.tsx'
import '../index.css'

const fixtures = import.meta.glob<string>('../../e2e/fixtures/*.qmd', {
  query: '?raw',
  import: 'default',
  eager: true,
})

function FixtureApp() {
  const [file, setFile] = useState<SyncedFile | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const name = new URLSearchParams(window.location.search).get('fixture') ?? 'basic'
    const text = fixtures[`../../e2e/fixtures/${name}.qmd`]
    if (text === undefined) {
      setError(`No fixture named ${name}.`)
      return
    }
    initWasm().then(
      () => setFile(createLocalFile(text)),
      (e) => setError(`Failed to initialize WASM: ${e}`),
    )
  }, [])

  if (error) return <p className="status status-error">{error}</p>
  if (!file) return <p className="status">Loading…</p>
  return (
    <div className="panes panes-single">
      <VisualEditor file={file} />
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <FixtureApp />
  </StrictMode>,
)
