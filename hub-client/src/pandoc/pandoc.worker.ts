/**
 * The pandoc worker shell: a thin wrapper around the host package's message handler.
 *
 * The file name is deliberate. Vite emits `pandoc.worker-<hash>.js`, which matches the PWA
 * config's `globIgnores: ['**\/*.worker-*.js']` and the `ondemand-assets` runtime route
 * (vite.config.ts), keeping the worker out of the service worker's precache.
 */
import { createHandler, type WorkerRequest } from '@quarto/pandoc-host';

const handle = createHandler((msg, transfer) => self.postMessage(msg, { transfer: transfer ?? [] }));

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  void handle(event.data);
};
