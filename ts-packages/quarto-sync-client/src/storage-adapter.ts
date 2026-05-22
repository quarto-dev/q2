/**
 * Runtime-aware storage-adapter selector for automerge-repo.
 *
 * Browser (hub-client SPA) — uses IndexedDB so document caches
 * survive across page reloads. Node (hub-mcp and similar CLI
 * consumers) — uses an in-memory adapter because there is no
 * IndexedDB global and we don't want a filesystem-backed cache
 * outside the user's control.
 *
 * `IndexedDBStorageAdapter`'s constructor reads the `indexedDB`
 * global eagerly, so `new IndexedDBStorageAdapter()` throws
 * `indexedDB is not defined` in Node. This selector picks at
 * construction time based on whether the global exists.
 */

import {
  StorageAdapter,
  type Chunk,
  type StorageAdapterInterface,
  type StorageKey,
} from '@automerge/automerge-repo';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';

/**
 * Process-local in-memory storage. Matches automerge-repo's bundled
 * `DummyStorageAdapter` semantics but lives here so we can import it
 * via the supported top-level `@automerge/automerge-repo` entry
 * (the `helpers/*` subpath imports don't resolve their source-side
 * dependencies cleanly under our `tsc` setup).
 */
export class MemoryStorageAdapter extends StorageAdapter implements StorageAdapterInterface {
  private readonly data = new Map<string, Uint8Array>();

  private encode(key: StorageKey): string {
    return key.join('.');
  }

  private decode(key: string): StorageKey {
    return key.split('.');
  }

  async load(key: StorageKey): Promise<Uint8Array | undefined> {
    return this.data.get(this.encode(key));
  }

  async save(key: StorageKey, data: Uint8Array): Promise<void> {
    this.data.set(this.encode(key), data);
  }

  async remove(key: StorageKey): Promise<void> {
    this.data.delete(this.encode(key));
  }

  async loadRange(keyPrefix: StorageKey): Promise<Chunk[]> {
    const prefix = this.encode(keyPrefix);
    const out: Chunk[] = [];
    for (const [k, data] of this.data) {
      if (k.startsWith(prefix)) out.push({ key: this.decode(k), data });
    }
    return out;
  }

  async removeRange(keyPrefix: StorageKey): Promise<void> {
    const prefix = this.encode(keyPrefix);
    for (const k of this.data.keys()) {
      if (k.startsWith(prefix)) this.data.delete(k);
    }
  }
}

export function buildStorageAdapter(): StorageAdapterInterface {
  // `globalThis.indexedDB` only exists in browser-shaped runtimes.
  // Hub-client's tsconfig pulls in DOM lib so the lookup typechecks
  // cleanly there; hub-mcp's tsconfig doesn't, so we fall back to an
  // index lookup that any reasonable global object supports.
  const g = globalThis as Record<string, unknown>;
  if (typeof g['indexedDB'] === 'undefined') {
    return new MemoryStorageAdapter();
  }
  return new IndexedDBStorageAdapter();
}
