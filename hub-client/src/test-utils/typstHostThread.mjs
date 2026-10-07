// worker_threads entry for the typst real-wasm tests: runs the host's message handler in a
// real Worker, as the browser shell (src/typst/typst.worker.ts) does. Node 24 strips the
// types from the package's .ts source. No package fetcher: packages outside the vendored
// set are reported as not found, which is what the offline tests want.
import { parentPort } from 'node:worker_threads';
import { createHandler } from '../../../ts-packages/typst-host/src/index.ts';

const handle = createHandler((msg, transfer) => parentPort.postMessage(msg, transfer));
parentPort.on('message', (msg) => void handle(msg));
