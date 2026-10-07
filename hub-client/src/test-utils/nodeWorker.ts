import { Worker } from 'node:worker_threads';
import type { WorkerLike } from '../pandoc/pandocRunner';

/** Adapts a `worker_threads` Worker running pandocHostThread.mjs to the runner's `WorkerLike`. */
export function nodePandocWorker(): WorkerLike {
  const w = new Worker(new URL('./pandocHostThread.mjs', import.meta.url));
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
