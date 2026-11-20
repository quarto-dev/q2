/**
 * Minimal custom editor for `q2 preview --ui custom` (prototype).
 *
 * The preview server's URLs arrive as VITE_Q2_* env vars, set by
 * scripts/q2-preview-custom.mjs from the JSON line the CLI prints.
 *
 * - Editing: the textarea is synced with the project's automerge
 *   documents over the server's WebSocket (`wsUrl` + `indexDocId`).
 * - Viewing: the server serves no UI in this mode. This page parses
 *   the document itself (WASM) and shows the AST as JSON.
 */
import { createSyncClient } from '@quarto/quarto-sync-client'
// we will need this to be an npm package?
import initWasm, { parse_qmd_content } from 'wasm-quarto-hub-client'

const env = import.meta.env
const wsUrl: string = env.VITE_Q2_WS_URL
const indexDocId: string = env.VITE_Q2_INDEX_DOC_ID
const allowEdit = env.VITE_Q2_ALLOW_EDIT === 'true'

const files = document.querySelector<HTMLSelectElement>('#files')!
const status = document.querySelector<HTMLSpanElement>('#status')!
const editor = document.querySelector<HTMLTextAreaElement>('#editor')!
const ast = document.querySelector<HTMLPreElement>('#ast')!

let current: string = env.VITE_Q2_FILE ?? ''

/** Parse `text` and show its AST as JSON. */
function showAst(text: string) {
  const result = parse_qmd_content(text)
  ast.textContent =
    result.success === true
      ? JSON.stringify(JSON.parse(result.ast), null, 2)
      : JSON.stringify(result, null, 2)
}

/** Show `path` in the textarea and its AST next to it. */
function open(path: string) {
  current = path
  files.value = path
  editor.value = client.getFileContent(path) ?? ''
  showAst(editor.value)
}

const client = createSyncClient({
  onFileAdded: () => { },
  onFileRemoved: () => { },
  onBinaryChanged: () => { },
  // A change from elsewhere (another client, or the file on disk).
  onFileChanged: (path, text) => {
    if (path === current && text !== editor.value) {
      const { selectionStart, selectionEnd } = editor
      editor.value = text
      editor.setSelectionRange(selectionStart, selectionEnd)
      showAst(text)
    }
  },
  onConnectionChange: (connected) => {
    const mode = allowEdit ? 'edits are written to disk' : 'edits are not written to disk (pass --allow-edit)'
    status.textContent = connected ? `connected — ${mode}` : 'disconnected'
  },
  onError: (error) => {
    status.textContent = `error: ${error.message}`
  },
})

editor.addEventListener('input', () => {
  if (current === '') return
  client.updateFileContent(current, editor.value)
  showAst(editor.value)
})
files.addEventListener('change', () => open(files.value))

if (wsUrl === undefined || indexDocId === undefined) {
  status.textContent = 'No preview server configured: start this with `node scripts/q2-preview-custom.mjs`.'
} else {
  await initWasm()
  // Memory storage: the preview hub is ephemeral, so there is nothing to cache.
  const entries = await client.connect(wsUrl, indexDocId, undefined, undefined, undefined, {
    storage: 'memory',
  })
  const paths = entries.map((entry) => entry.path).filter((path) => path.endsWith('.qmd')).sort()
  files.replaceChildren(...paths.map((path) => new Option(path, path)))
  if (current === '' && paths.length > 0) current = paths[0]
  if (current !== '') open(current)
}
