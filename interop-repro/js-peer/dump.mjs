import { Repo } from '@automerge/automerge-repo';
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
const [url, docId] = process.argv.slice(2);
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], sharePolicy: async () => true });
const h = await repo.find(docId);
for (let i = 0; i < 12; i++) {
  const d = h.doc();
  console.log(`[t=${i*500}ms] keys=${JSON.stringify(Object.keys(d||{}))} captures=${JSON.stringify(d?.captures||null)}`);
  if (d?.captures && Object.keys(d.captures).length) break;
  await new Promise(r => setTimeout(r, 500));
}
process.exit(0);
