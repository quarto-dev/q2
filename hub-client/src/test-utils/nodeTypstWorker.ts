import { Worker } from 'node:worker_threads';
import type { WorkerLike } from '../typst/typstRunner';

/** Adapts a `worker_threads` Worker running typstHostThread.mjs to the runner's `WorkerLike`. */
export function nodeTypstWorker(): WorkerLike {
  const w = new Worker(new URL('./typstHostThread.mjs', import.meta.url));
  const like: WorkerLike = {
    onmessage: null,
    onerror: null,
    onmessageerror: null,
    postMessage: (msg, transfer) => w.postMessage(msg, transfer as never),
    terminate: () => void w.terminate(),
  };
  w.on('message', (data) => like.onmessage?.({ data }));
  w.on('error', (e) => like.onerror?.({ message: String(e) }));
  w.on('messageerror', (e) => like.onmessageerror?.(e));
  return like;
}
