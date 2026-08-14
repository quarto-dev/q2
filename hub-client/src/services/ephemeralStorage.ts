/**
 * Ephemeral storage mode (bd-sw4xy1vw).
 *
 * The q2 preview embed build (`npm run build:preview-embed`) sets
 * VITE_EPHEMERAL_STORAGE=1. Every `q2 preview` session is a fresh origin
 * (a random loopback port), so nothing persisted to IndexedDB is ever
 * read again — the `automerge` document cache and the `quarto-hub`
 * records just accumulate across sessions. In this mode the app keeps
 * everything in memory: storage dies with the page, matching the
 * throwaway per-session server.
 */

import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import type { StorageAdapterInterface } from '@automerge/automerge-repo';
import { MemoryStorageAdapter } from '@quarto/quarto-sync-client';

/**
 * True when this build serves an ephemeral per-session preview hub.
 * Read lazily (not at module scope) so tests can `vi.stubEnv` without
 * resetting the module graph.
 */
export function isEphemeralStorage(): boolean {
  return import.meta.env.VITE_EPHEMERAL_STORAGE === '1';
}

/**
 * Storage adapter for the per-sync-server automerge Repo. Ephemeral
 * sessions use the same in-memory adapter as the q2-preview viewer SPA
 * (`storage: 'memory'`, PreviewApp.tsx) — document caches are useless
 * across sessions because the origin never recurs.
 */
export function repoStorageAdapter(): StorageAdapterInterface {
  return isEphemeralStorage()
    ? new MemoryStorageAdapter()
    : new IndexedDBStorageAdapter();
}
