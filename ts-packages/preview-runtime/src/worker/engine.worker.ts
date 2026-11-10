/**
 * Entry point of the render worker (preview VFS contract D3). Runs the
 * engine (`wasmRenderer.ts` and the WASM it loads) in this worker and
 * serves it to the page through `serveEngine`. Started by
 * `startWorkerEngine` in `./engineClient`.
 */

import * as engine from '../wasmRenderer';
import { setPerfEnabled } from '../perfCounters';
import { serveEngine } from './engineHost';
import type { Endpoint } from './rpc';

serveEngine(self as unknown as Endpoint, engine, {
  setPerf: setPerfEnabled,
  // A worker has no `<img>` or `<canvas>` for SVG; ask the page.
  installRasterizer: async (rasterize) => {
    const bridge = await import('/src/wasm-js-bridge/rasterize.js');
    bridge.setRasterizer(rasterize);
  },
});
