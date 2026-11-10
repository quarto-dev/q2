/**
 * The main thread's side of the render worker (preview VFS contract D3).
 *
 * `createWorkerBackend` turns an endpoint to a worker running
 * `serveEngine` into an {@link EngineBackend}: VFS writes update the
 * main-thread {@link VfsMirror} and are forwarded to the worker in order,
 * reads answer from the mirror, and renders and engine helpers are calls
 * to the worker. The worker's requests to rasterize an SVG (it has no DOM)
 * are answered here.
 *
 * `startWorkerEngine` spawns the worker and installs the backend, so the
 * existing `@quarto/preview-runtime` API runs the engine in the worker.
 */

import {
  installEngineBackend,
  type EngineBackend,
  type PreviewRenderResponse,
  type VfsResponse,
} from '../wasmRenderer';
import { perfEnabled } from '../perfCounters';
import { Rpc, Transfer, type Endpoint } from './rpc';
import { VfsMirror } from './vfsMirror';
import type { EngineInit, RenderPreviewResult } from './engineHost';
// Vite bundles the worker entry and gives back its URL.
import engineWorkerUrl from './engine.worker.ts?worker&url';

/** Answer the worker's rasterize requests with the page's DOM rasterizer. */
async function rasterizeOnMainThread(svg: Uint8Array, maxSide: number): Promise<Transfer<Uint8Array>> {
  const { jsRasterizeSvg } = await import('/src/wasm-js-bridge/rasterize.js');
  const png = await jsRasterizeSvg(svg, maxSide);
  return new Transfer(png, [png.buffer]);
}

/** Options for the worker engine. */
export interface WorkerEngineOptions {
  /**
   * The preview server's miss routes, ending in `/api/preview/` (preview VFS
   * contract D2). Absent: the worker's VFS answers only what sync wrote.
   */
  missBaseUrl?: string;
}

export function createWorkerBackend(
  endpoint: Endpoint,
  options: WorkerEngineOptions = {},
): EngineBackend & { mirror: VfsMirror; missStats(): Promise<Record<string, number>> } {
  const mirror = new VfsMirror();
  const rpc = new Rpc(endpoint, { rasterize: rasterizeOnMainThread });
  let initPromise: Promise<void> | null = null;
  let ready = false;

  /** Forward a VFS write; the mirror already answered for it. */
  const forward = (method: string, args: unknown[]): void => {
    rpc.call(method, args).catch((err: unknown) => {
      console.error(`[render worker] ${method} failed`, err);
    });
  };

  return {
    mirror,
    init() {
      initPromise ??= (async () => {
        const init: EngineInit = {
          ephemeralStorage:
            (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ === true,
          perf: perfEnabled(),
          missBaseUrl: options.missBaseUrl,
        };
        await rpc.call('init', [init]);
        ready = true;
      })().catch((err: unknown) => {
        initPromise = null;
        throw err;
      });
      return initPromise;
    },
    isReady: () => ready,
    vfsAddFile(path, content): VfsResponse {
      forward('vfsAddFile', [path, content]);
      return mirror.addText(path, content);
    },
    vfsAddBinaryFile(path, content): VfsResponse {
      forward('vfsAddBinaryFile', [path, content]);
      return mirror.addBinary(path, content);
    },
    vfsRemoveFile(path): VfsResponse {
      forward('vfsRemoveFile', [path]);
      return mirror.remove(path);
    },
    vfsClear(): VfsResponse {
      forward('vfsClear', []);
      return mirror.clear();
    },
    vfsListFiles: () => mirror.listFiles(),
    vfsReadFile: (path) => mirror.readFile(path),
    vfsReadBinaryFile: (path) => mirror.readBinaryFile(path),
    async renderPageForPreview(path, captureGzJson, options): Promise<PreviewRenderResponse> {
      const result = await rpc.call<RenderPreviewResult>('renderPreview', [
        path,
        captureGzJson,
        options,
      ]);
      mirror.applyArtifacts(result.artifacts);
      // Pulled files join the mirror, so main-thread readers (the asset
      // manifest) see them and their sync echo is an identical write.
      for (const { path, bytes } of result.pulled) mirror.addBinary(path, bytes);
      return result.response;
    },
    resolvePandocFormats: (path, options) => rpc.call('resolvePandocFormats', [path, options]),
    parseQmdContent: (text) => rpc.call('parseQmdContent', [text]),
    applyNodeEdit: (content, untransformed, destination, modified) =>
      rpc.call('applyNodeEdit', [content, untransformed, destination, modified]),
    regenerateNestedBuffers: (content, untransformed) =>
      rpc.call('regenerateNestedBuffers', [content, untransformed]),
    missStats: () => rpc.call('missStats', []),
  };
}

/**
 * Start the module worker at `url`.
 *
 * A worker script must be same-origin with the page that starts it. A page
 * framed in an opaque-origin sandbox (`sandbox="allow-scripts"` without
 * `allow-same-origin`) has origin `null`, so it cannot start a worker from
 * the preview server's URL, and Chromium refuses a *module* worker from a
 * `blob:` URL there too. It can start a classic worker from a `blob:` URL
 * it minted itself; that shim loads the real worker with a dynamic
 * `import()` (a cross-origin module fetch that needs CORS, as the page's
 * own module scripts already do in that setup). Messages that arrive
 * before the engine is listening are held and replayed.
 */
function spawnWorker(url: URL): Worker {
  const options: WorkerOptions = { type: 'module', name: 'quarto-render' };
  const viaBlob = () => {
    const shim = `
      const held = [];
      const hold = (event) => held.push(event.data);
      self.addEventListener('message', hold);
      import(${JSON.stringify(url.href)}).then(
        () => {
          self.removeEventListener('message', hold);
          for (const data of held) self.dispatchEvent(new MessageEvent('message', { data }));
        },
        (error) => setTimeout(() => { throw error; }),
      );`;
    const blob = new Blob([shim], { type: 'text/javascript' });
    return new Worker(URL.createObjectURL(blob), { name: options.name });
  };
  if (globalThis.location?.origin === 'null') return viaBlob();
  try {
    return new Worker(url, options);
  } catch (err) {
    if (err instanceof DOMException && err.name === 'SecurityError') return viaBlob();
    throw err;
  }
}

/**
 * Run the engine in a dedicated module worker and route this realm's
 * `@quarto/preview-runtime` engine API to it. Call once, before
 * `initWasm` (which then starts the worker's engine).
 */
export function startWorkerEngine(
  options: WorkerEngineOptions = {},
): EngineBackend & { missStats(): Promise<Record<string, number>> } {
  const worker = spawnWorker(new URL(engineWorkerUrl, globalThis.location?.href));
  worker.addEventListener('error', (event) => {
    console.error('[render worker] failed', event.message);
  });
  const backend = createWorkerBackend(worker, options);
  installEngineBackend(backend);
  return backend;
}
