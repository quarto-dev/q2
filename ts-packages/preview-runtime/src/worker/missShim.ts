/**
 * The render worker's VFS miss handler (preview VFS contract D2, D3).
 *
 * When the engine's VFS cannot answer a read, an existence check or a
 * listing for a project path, it calls this handler synchronously; the
 * handler asks the preview server's miss routes with a synchronous
 * `XMLHttpRequest` (allowed in a worker) and answers from the reply. A
 * 404, a 403 (the exposure policy refused the path) and a transport
 * failure all answer "absent".
 *
 * Every call is counted per arm and status under the `vfs-pull` perf
 * gauge (with `?perf=1` or `localStorage.quartoPerfStats = '1'`), and in
 * `counts()` for tests.
 */

import { perfExtra, perfNow, perfSegment } from '../perfCounters';

/** A synchronous GET returning status and body bytes. */
export type SyncRequest = (url: string) => { status: number; body: Uint8Array };

/** The handler object `vfs_set_miss_handler` expects. */
export interface MissHandlerObject {
  read(path: string): Uint8Array | null;
  stat(path: string): { exists: boolean; isFile: boolean; isDir: boolean };
  list(path: string): Array<{ name: string; kind: 'file' | 'dir' }> | null;
  /** Calls so far, keyed `<arm>.<status>`. */
  counts(): Record<string, number>;
}

/** Synchronous XHR, the production transport. */
export const xhrRequest: SyncRequest = (url) => {
  const xhr = new XMLHttpRequest();
  xhr.open('GET', url, false);
  xhr.responseType = 'arraybuffer';
  xhr.send();
  const body = xhr.response instanceof ArrayBuffer ? new Uint8Array(xhr.response) : new Uint8Array();
  return { status: xhr.status, body };
};

const ABSENT = { exists: false, isFile: false, isDir: false };

export function createXhrMissHandler(
  /** The miss routes' base, ending in `/api/preview/`. */
  baseUrl: string,
  options: {
    request?: SyncRequest;
    /** Called with each file pulled, for the page's VFS mirror. */
    onPull?: (path: string, bytes: Uint8Array) => void;
  } = {},
): MissHandlerObject {
  const request = options.request ?? xhrRequest;
  const counts: Record<string, number> = {};

  const ask = (arm: 'read' | 'stat' | 'list', route: string, path: string) => {
    const t0 = perfNow();
    let status: string;
    let result: { status: number; body: Uint8Array } | null = null;
    try {
      result = request(`${baseUrl}${route}?path=${encodeURIComponent(path)}`);
      status = String(result.status);
    } catch {
      status = 'error';
    }
    counts[`${arm}.${status}`] = (counts[`${arm}.${status}`] ?? 0) + 1;
    perfSegment('vfs-pull', t0);
    perfExtra('vfs-pull', `${arm}.${status}`);
    return result?.status === 200 ? result.body : null;
  };

  const json = <T>(bytes: Uint8Array | null): T | null => {
    if (!bytes) return null;
    try {
      return JSON.parse(new TextDecoder().decode(bytes)) as T;
    } catch {
      return null;
    }
  };

  return {
    read(path) {
      const bytes = ask('read', 'file', path);
      if (bytes) options.onPull?.(path, bytes);
      return bytes;
    },
    stat(path) {
      return json<{ exists: boolean; isFile: boolean; isDir: boolean }>(ask('stat', 'stat', path)) ?? ABSENT;
    },
    list(path) {
      return (
        json<{ entries: Array<{ name: string; kind: 'file' | 'dir' }> }>(ask('list', 'list', path))?.entries ??
        null
      );
    },
    counts: () => ({ ...counts }),
  };
}
