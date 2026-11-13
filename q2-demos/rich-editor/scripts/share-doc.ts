/**
 * Put a qmd file on a sync server as a new project, for trying the editor
 * together, and print the query string that opens it.
 *
 *   npm run share-doc -- [file.qmd]
 *
 * The file defaults to `e2e/fixtures/basic.qmd`, and the server to the
 * public automerge sync server (`SYNC_SERVER=<ws-url>` overrides it).
 * Anyone who has the printed document id can read and edit the document.
 */

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { createSyncClient } from '@quarto/quarto-sync-client'

const FILE = 'index.qmd'
const server = process.env.SYNC_SERVER ?? 'wss://sync.automerge.org'
const path = process.argv[2] ?? fileURLToPath(new URL('../e2e/fixtures/basic.qmd', import.meta.url))
const source = readFileSync(path, 'utf8')

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

function client() {
  return createSyncClient({
    onFileAdded() {},
    onFileChanged() {},
    onBinaryChanged() {},
    onFileRemoved() {},
    onConnectionChange() {},
    onError(error) {
      console.error('[sync]', error.message)
    },
  })
}

const author = client()
const { indexDocId } = await author.createNewProject({
  syncServer: server,
  files: [{ path: FILE, content: source, contentType: 'text' }],
  peerTimeoutMs: 15000,
  requireOnline: true,
  storage: 'memory',
})
// Leave time for the documents to reach the server before disconnecting.
await sleep(5000)
await author.disconnect()

// Check that the server has them: open the project from a fresh client.
const reader = client()
await reader.connect(server, indexDocId, undefined, undefined, undefined, { peerTimeoutMs: 15000, storage: 'memory' })
let text: string | null = null
for (let i = 0; i < 40 && text !== source; i++) {
  await sleep(500)
  text = reader.getFileContent(FILE)
}
await reader.disconnect()
if (text !== source) {
  console.error(`The server at ${server} doesn't have ${FILE} of project ${indexDocId}.`)
  process.exit(1)
}

console.log(`\n?${new URLSearchParams({ server, doc: indexDocId, file: FILE })}`)
process.exit(0)
