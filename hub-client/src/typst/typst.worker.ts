/**
 * The typst compile worker shell: a thin wrapper around the host package's message handler,
 * plus `fetch` for the package registry (which the core keeps out).
 *
 * The file name is deliberate, as for `pandoc.worker.ts`: Vite emits `typst.worker-<hash>.js`,
 * which matches the PWA config's `globIgnores: ['**\/*.worker-*.js']` and the `ondemand-assets`
 * runtime route (vite.config.ts), keeping the worker out of the service worker's precache.
 */
import { createHandler, type WorkerRequest } from '@quarto/typst-host';

// The package tarball cache is the Cache API on the main thread (TypstRunner serves the
// worker's `cache-get`/`cache-put` messages); see the note on the message types.
const handle = createHandler((msg, transfer) => self.postMessage(msg, { transfer: transfer ?? [] }), { fetch: (...a) => fetch(...a) });

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  void handle(event.data);
};
