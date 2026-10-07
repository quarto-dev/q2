import { registryFetcher, type TarballCache } from './packages.ts';
import { TypstSession, type SessionInit } from './session.ts';
import type { CompileInput, CompileResult } from './types.ts';

/** How long the worker waits for the main thread to answer a cache read before fetching instead. */
export const CACHE_REPLY_MS = 5_000;

/** Messages to the worker. The shell in hub-client forwards `onmessage` data to `createHandler`. */
export type WorkerRequest =
  | { type: 'init'; init: Omit<SessionInit, 'fetchPackage'> }
  | { type: 'run'; id: number; input: CompileInput }
  /** The answer to a `cache-get`; `bytes` is absent on a miss. */
  | { type: 'cache-reply'; reqId: number; bytes?: Uint8Array };

/** Messages from the worker. */
export type WorkerResponse =
  | { type: 'ready'; fontFamilies: string[] }
  | { type: 'init-failed'; message: string }
  | { type: 'progress'; id: number; stage: 'preparing' | 'compiling' }
  | { type: 'result'; id: number; result: CompileResult }
  /**
   * The package tarball cache lives on the main thread: WebKit loses Cache API writes made by a
   * worker that is terminated soon after (observed in Playwright's WebKit), and a worker per compile is
   * the design. The worker asks, the main thread reads or writes the Cache API.
   */
  | { type: 'cache-get'; reqId: number; url: string }
  | { type: 'cache-put'; url: string; bytes: Uint8Array };

/**
 * The worker-side state machine, independent of `self`/`parentPort`: `post` is the
 * environment's postMessage and `env.fetch` is the shell's `fetch` (the core keeps `fetch` out;
 * without one, packages outside the vendored set are reported as not found). Package tarballs are
 * cached by the main thread through `cache-get`/`cache-put` messages. The PDF buffer is
 * transferred, not copied.
 */
export function createHandler(post: (msg: WorkerResponse, transfer?: Transferable[]) => void, env: { fetch?: typeof fetch } = {}) {
  let session: TypstSession | undefined;
  let nextReq = 1;
  const pending = new Map<number, (bytes: Uint8Array | undefined) => void>();
  const cache: TarballCache = {
    get: (url) =>
      new Promise((resolve) => {
        const reqId = nextReq++;
        const timer = setTimeout(() => {
          pending.delete(reqId);
          resolve(undefined);
        }, CACHE_REPLY_MS);
        pending.set(reqId, (bytes) => {
          clearTimeout(timer);
          resolve(bytes);
        });
        post({ type: 'cache-get', reqId, url });
      }),
    put: async (url, bytes) => {
      const copy = bytes.slice();
      post({ type: 'cache-put', url, bytes: copy }, [copy.buffer as ArrayBuffer]);
    },
  };
  const fetchPackage = env.fetch ? registryFetcher({ fetch: env.fetch, cache }) : undefined;
  return async (msg: WorkerRequest): Promise<void> => {
    if (msg.type === 'cache-reply') {
      pending.get(msg.reqId)?.(msg.bytes);
      pending.delete(msg.reqId);
      return;
    }
    if (msg.type === 'init') {
      try {
        session = await TypstSession.create({ ...msg.init, fetchPackage });
        post({ type: 'ready', fontFamilies: session.fontFamilies });
      } catch (e) {
        post({ type: 'init-failed', message: e instanceof Error ? e.message : String(e) });
      }
      return;
    }
    if (!session) {
      post({
        type: 'result',
        id: msg.id,
        result: { ok: false, kind: 'crash', diagnostics: [{ origin: 'host', kind: 'error', code: 'typst-crash', message: 'worker received a run before init', stage: 'typst' }] },
      });
      return;
    }
    post({ type: 'progress', id: msg.id, stage: 'compiling' });
    const result = await session.compile(msg.input);
    const transfer: Transferable[] = [];
    if (result.ok && result.pdf.buffer instanceof ArrayBuffer && result.pdf.byteLength === result.pdf.buffer.byteLength) transfer.push(result.pdf.buffer);
    post({ type: 'result', id: msg.id, result }, transfer);
  };
}
