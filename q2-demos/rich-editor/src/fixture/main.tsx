/**
 * The visual editor on a fixture document, with no hub: for the e2e tests
 * and for trying the editor without a server.
 *
 *   /fixture.html?fixture=<name>   loads e2e/fixtures/<name>.qmd
 *
 * The fixture name defaults to `basic`.
 */

import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { initWasm, parseQmdContent } from '../quarto-synced-file/index.ts'
import { VisualEditor } from '../VisualEditor.tsx'
import '../index.css'

const fixtures = import.meta.glob<string>('../../e2e/fixtures/*.qmd', {
  query: '?raw',
  import: 'default',
  eager: true,
})

function FixtureApp() {
  const [ast, setAst] = useState<RustQmdJson | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const name = new URLSearchParams(window.location.search).get('fixture') ?? 'basic'
    const text = fixtures[`../../e2e/fixtures/${name}.qmd`]
    if (text === undefined) {
      setError(`No fixture named ${name}.`)
      return
    }
    initWasm().then(
      () => {
        const parsed = parseQmdContent(text)
        if (parsed) setAst(parsed)
        else setError(`Fixture ${name} doesn't parse.`)
      },
      (e) => setError(`Failed to initialize WASM: ${e}`),
    )
  }, [])

  if (error) return <p className="status status-error">{error}</p>
  if (!ast) return <p className="status">Loading…</p>
  return (
    <div className="panes panes-single">
      <VisualEditor ast={ast} />
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <FixtureApp />
  </StrictMode>,
)
