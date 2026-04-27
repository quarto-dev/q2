import type {
  StorageAdapterInterface,
  StorageKey,
  Chunk,
} from '@automerge/automerge-repo/slim'

/**
 * Wraps a `StorageAdapterInterface` and silently drops all writes.
 *
 * Used by the debug page when inspecting the main app's IndexedDB-backed
 * storage: we want to see exactly what's persisted on disk without letting
 * the debug tab's in-memory Repo accidentally save anything back to the
 * shared database. Concurrent writes from a second Repo are precisely the
 * kind of concurrency that could itself be a source of bugs, so this
 * wrapper eliminates that variable entirely.
 */
export class ReadOnlyStorageAdapter implements StorageAdapterInterface {
  #inner: StorageAdapterInterface

  constructor(inner: StorageAdapterInterface) {
    this.#inner = inner
  }

  load(key: StorageKey): Promise<Uint8Array | undefined> {
    return this.#inner.load(key)
  }

  loadRange(keyPrefix: StorageKey): Promise<Chunk[]> {
    return this.#inner.loadRange(keyPrefix)
  }

  async save(_key: StorageKey, _data: Uint8Array): Promise<void> {
    // Intentional no-op: debug page is read-only w.r.t. persistent storage.
  }

  async remove(_key: StorageKey): Promise<void> {
    // Intentional no-op.
  }

  async removeRange(_keyPrefix: StorageKey): Promise<void> {
    // Intentional no-op.
  }
}
