// Full hub-client-side e2e stand-in: create a project, broadcast an exec/request
// (like the Run button), and read back the capture the provider writes.
//   node e2e-run.mjs <ws-url>
import { Repo } from '@automerge/automerge-repo';
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import zlib from 'node:zlib';

const url = process.argv[2];
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], sharePolicy: async () => true });

const qmd = `---\ntitle: JS project\nengine: knitr\n---\n\n\`\`\`{r}\ncat(1, 2, 3)\n\`\`\`\n`;
const fileHandle = repo.create();
fileHandle.change((d) => { d.text = qmd; });
await fileHandle.whenReady();
const idx = repo.create();
idx.change((d) => { d.files = { 'index.qmd': fileHandle.documentId }; d.identities = {}; d.version = 2; });
await idx.whenReady();
console.log('INDEX_DOC_ID=' + idx.documentId);

// Broadcast an exec/request every 1s (ephemeral, like the Run button) and poll
// the captures sidecar for the provider's write-back.
let done = false;
const onChange = async ({ doc }) => {
  const cap = doc?.captures?.['index.qmd'];
  if (done || !(cap?.captureDocId && cap.state === 'idle')) return;
  done = true;
  console.log(`CAPTURE state=${cap.state} docId=${cap.captureDocId}`);
  const capHandle = await repo.find(cap.captureDocId);
  const content = capHandle.doc()?.content;
  if (content) {
    const json = JSON.parse(zlib.gunzipSync(Buffer.from(content)).toString());
    console.log('ENGINE=' + json[0]?.engine_name);
    // The stdout of `cat(1,2,3)` appears in the captured markdown.
    const md = json[0]?.result?.markdown ?? '';
    const m = md.match(/cell-output-stdout[\s\S]*?```\n([\s\S]*?)\n```/);
    console.log('STDOUT=' + (m ? m[1].trim() : '(see result.markdown)'));
  }
  process.exit(0);
};
idx.on('change', onChange);
const timer = setInterval(() => {
  idx.broadcast({ kind: 'exec/request', path: 'index.qmd', requestId: 'r1', requesterActorId: 'js-e2e' });
}, 1000);
setTimeout(() => { if (!done) { console.log('NO_CAPTURE_WITHIN_TIMEOUT'); process.exit(1); } clearInterval(timer); }, 60000);
