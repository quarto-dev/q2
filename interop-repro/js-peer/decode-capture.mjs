import { Repo } from '@automerge/automerge-repo';
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import zlib from 'node:zlib';
const [url, capId] = process.argv.slice(2);
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], sharePolicy: async () => true });
const h = await repo.find(capId);
for (let i = 0; i < 20; i++) {
  const content = h.doc()?.content;
  if (content) {
    const caps = JSON.parse(zlib.gunzipSync(Buffer.from(content)).toString());
    console.log('ENGINE=' + caps[0]?.engine_name);
    console.log('RESULT=' + JSON.stringify(caps[0]?.result));
    break;
  }
  await new Promise(r => setTimeout(r, 500));
}
process.exit(0);
