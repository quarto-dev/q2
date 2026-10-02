// worker_threads entry for tests that must terminate a run (hang fault injection): runs
// the host's message handler in a real Worker, as the browser shell will. Node 24 strips
// the types from the package's .ts source; its V8 flags (exnref) are inherited from the
// process.
import { parentPort } from 'node:worker_threads';
import { createHandler } from '../../../ts-packages/pandoc-host/src/index.ts';

const handle = createHandler((msg, transfer) => parentPort.postMessage(msg, transfer));
parentPort.on('message', (msg) => void handle(msg));
