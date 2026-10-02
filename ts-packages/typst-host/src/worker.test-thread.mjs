// worker_threads entry for worker.test.ts: runs the host's message handler in a real Worker.
// Node 24 strips the types from the package's .ts source.
import { parentPort } from 'node:worker_threads';
import { createHandler } from './protocol.ts';

const handle = createHandler((msg, transfer) => parentPort.postMessage(msg, transfer));
parentPort.on('message', (msg) => void handle(msg));
