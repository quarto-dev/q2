/**
 * Open a file on this machine in the visual editor, and keep the file and
 * the editor in step until you stop it (Ctrl-C).
 *
 *   bin/rich-editor <file.qmd>
 *
 * It starts a sync hub in this process, puts the file on it as a project
 * of one file, serves the editor with Vite and opens it in the browser.
 * Edits made in the editor are written to the file as they're made, and
 * changes made to the file by something else (a text editor, git) are
 * sent to the editor. Nothing leaves this machine: the hub listens on
 * 127.0.0.1 only.
 */

import { readFileSync, watch, writeFileSync } from 'node:fs'
import { basename, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFile } from 'node:child_process'
import { createServer } from 'vite'
import { createSyncClient, setSyncLogger } from '@quarto/quarto-sync-client'
import { startTestHub } from '../../../ts-packages/quarto-sync-client/src/test-hub.ts'

const arg = process.argv[2]
if (!arg || arg === '-h' || arg === '--help') {
  console.error('Usage: rich-editor <file>')
  process.exit(arg ? 0 : 1)
}
const path = resolve(process.env.RICH_EDITOR_CWD ?? process.cwd(), arg)
let disk: string
try {
  disk = readFileSync(path, 'utf8')
} catch (error) {
  console.error(`Can't read ${path}: ${(error as Error).message}`)
  process.exit(1)
}
// The project's name for the file: its own name, so the page shows it.
const name = basename(path)

setSyncLogger(() => {})

const hub = await startTestHub()
const client = createSyncClient({
  onFileAdded() {},
  onFileChanged(changed, text) {
    if (changed !== name || text === disk) return
    disk = text
    writeFileSync(path, text)
  },
  onBinaryChanged() {},
  onFileRemoved() {},
  onConnectionChange() {},
  onError(error) {
    console.error('[sync]', error.message)
  },
})
const { indexDocId } = await client.createNewProject({
  syncServer: hub.url,
  files: [{ path: name, content: disk, contentType: 'text' }],
  peerTimeoutMs: 10000,
  storage: 'memory',
})
// The hub must have the document before the browser asks for it.
if (!(await hub.hubHasHeadsOf(client))) {
  console.error('The sync hub never received the document.')
  process.exit(1)
}

// Changes to the file from elsewhere go to the editor as one splice: the
// text between the common prefix and suffix of the old and new text.
// Watching the directory, not the file, survives editors that save by
// replacing the file.
let pending: NodeJS.Timeout | undefined
watch(resolve(path, '..'), (_event, changed) => {
  if (changed !== name) return
  clearTimeout(pending)
  pending = setTimeout(() => {
    let text: string
    try {
      text = readFileSync(path, 'utf8')
    } catch {
      return // Mid-save, or removed; the next event will tell.
    }
    if (text === disk) return
    const old = client.getFileContent(name) ?? ''
    disk = text
    let start = 0
    while (start < old.length && start < text.length && old[start] === text[start]) start++
    let end = 0
    while (end < old.length - start && end < text.length - start && old[old.length - 1 - end] === text[text.length - 1 - end]) end++
    client.applyEditorOperations(name, [
      { rangeOffset: start, rangeLength: old.length - start - end, text: text.slice(start, text.length - end) },
    ])
  }, 50)
})

const root = fileURLToPath(new URL('..', import.meta.url))
const server = await createServer({ root, configFile: resolve(root, 'vite.config.ts'), logLevel: 'warn' })
await server.listen()
const base = server.resolvedUrls?.local[0]
if (!base) throw new Error('Vite gave no URL to open.')
const url = `${base}?${new URLSearchParams({ server: hub.url, doc: indexDocId, file: name })}`

console.log(`Editing ${path}`)
console.log(url)
console.log('Ctrl-C to stop.')
if (!process.env.RICH_EDITOR_NO_OPEN) {
  const opener = process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'explorer' : 'xdg-open'
  execFile(opener, [url], () => {})
}

let stopping = false
process.on('SIGINT', async () => {
  if (stopping) process.exit(1)
  stopping = true
  await server.close()
  await client.disconnect().catch(() => {})
  await hub.stop()
  process.exit(0)
})
