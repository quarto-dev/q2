// Create a realistic Quarto project on a sync server, the way hub-client does:
// an index doc with files[path]=fileDocId, and a text file doc per file.
//   node create-project.mjs <ws-url>
import { Repo } from '@automerge/automerge-repo';
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';

const url = process.argv[2];
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], sharePolicy: async () => true });

// A qmd file document: { text: "<qmd source>" }.
const qmd = `---\ntitle: JS project\nengine: knitr\n---\n\n## Hello from a hub-client-style project\n\n\`\`\`{r}\ncat(1, 2, 3)\n\`\`\`\n`;
const fileHandle = repo.create();
fileHandle.change((d) => { d.text = qmd; });
await fileHandle.whenReady();

const idx = repo.create();
idx.change((d) => {
  d.files = { 'index.qmd': fileHandle.documentId };  // plain string -> automerge Text
  d.identities = {};
  d.version = 2;
});
await idx.whenReady();
console.log('INDEX_DOC_ID=' + idx.documentId);
console.log('FILE_DOC_ID=' + fileHandle.documentId);
setInterval(() => {}, 1000);
