// Minimal JS automerge-repo sync server (control for the interop matrix).
import { WebSocketServer } from 'ws';
import { Repo } from '@automerge/automerge-repo';
import { NodeWSServerAdapter } from '@automerge/automerge-repo-network-websocket';
import { NodeFSStorageAdapter } from '@automerge/automerge-repo-storage-nodefs';

const port = Number(process.env.PORT || 3044);
const wss = new WebSocketServer({ port });
// eslint-disable-next-line no-unused-vars
const repo = new Repo({
  network: [new NodeWSServerAdapter(wss)],
  storage: new NodeFSStorageAdapter(process.env.STORAGE || '/tmp/js-sync-storage'),
  sharePolicy: async () => true,
});
console.log('JS sync server listening on ws://127.0.0.1:' + port);
