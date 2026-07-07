// Minimal JS automerge-repo peer: create or read a doc { value: 42 }.
//   node peer.mjs create <ws-url>
//   node peer.mjs read   <ws-url> <doc-id>
import { Repo } from '@automerge/automerge-repo';
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';

const [cmd, url, docId] = process.argv.slice(2);
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], sharePolicy: async () => true });

if (cmd === 'create') {
  const handle = repo.create();
  handle.change((d) => { d.value = 42; d.files = { 'index.qmd': 'x', 'about.qmd': 'y' }; });
  await handle.whenReady();
  console.log('DOC_ID=' + handle.documentId);
  setInterval(() => {}, 1000); // stay alive so the doc stays served
} else if (cmd === 'read') {
  const handle = await repo.find(docId);
  for (let i = 0; i < 30; i++) {
    const doc = handle.doc();
    const v = doc && doc.value;
    console.log(`[JS read t=${i * 500}ms] value=${v === undefined ? 'none' : v}`);
    if (v !== undefined) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  process.exit(0);
} else {
  console.error('usage: node peer.mjs create|read <url> [docId]');
  process.exit(1);
}
