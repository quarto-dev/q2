/**
 * Tests for buildStorageAdapter — runtime-aware automerge-repo
 * storage selection. The browser SPA needs IndexedDB for cross-reload
 * caching; the Node (hub-mcp) path has no IndexedDB at all and was
 * blowing up at `new IndexedDBStorageAdapter()` construction time.
 */

import { describe, it, expect, afterEach, vi } from 'vitest';

import { buildStorageAdapter, MemoryStorageAdapter } from './storage-adapter.js';

afterEach(() => {
  // Restore any indexedDB stub installed by individual tests.
  // @ts-expect-error — test-only mutation of the global object.
  delete globalThis.indexedDB;
  vi.restoreAllMocks();
});

describe('buildStorageAdapter', () => {
  it('returns MemoryStorageAdapter when indexedDB is not defined (Node)', () => {
    expect(typeof globalThis.indexedDB).toBe('undefined');
    const adapter = buildStorageAdapter();
    expect(adapter).toBeInstanceOf(MemoryStorageAdapter);
  });

  it('returns MemoryStorageAdapter without throwing on a cold Node process', () => {
    // Regression: `new IndexedDBStorageAdapter()` references the
    // `indexedDB` global eagerly in its constructor, so Node would
    // throw `indexedDB is not defined` here. The selector must never
    // construct the IndexedDB adapter when the global is missing.
    expect(() => buildStorageAdapter()).not.toThrow();
  });

  it('MemoryStorageAdapter round-trips a save/load/remove cycle', async () => {
    const adapter = new MemoryStorageAdapter();
    const key = ['doc-1', 'snapshot', 'v1'];
    const bytes = new Uint8Array([1, 2, 3, 4, 5]);

    expect(await adapter.load(key)).toBeUndefined();

    await adapter.save(key, bytes);
    expect(await adapter.load(key)).toEqual(bytes);

    const range = await adapter.loadRange(['doc-1']);
    expect(range).toHaveLength(1);
    expect(range[0]!.data).toEqual(bytes);

    await adapter.remove(key);
    expect(await adapter.load(key)).toBeUndefined();
  });

  it('MemoryStorageAdapter removeRange deletes only the matching prefix', async () => {
    const adapter = new MemoryStorageAdapter();
    await adapter.save(['doc-A', 'x'], new Uint8Array([1]));
    await adapter.save(['doc-A', 'y'], new Uint8Array([2]));
    await adapter.save(['doc-B', 'z'], new Uint8Array([3]));

    await adapter.removeRange(['doc-A']);

    expect(await adapter.load(['doc-A', 'x'])).toBeUndefined();
    expect(await adapter.load(['doc-A', 'y'])).toBeUndefined();
    expect(await adapter.load(['doc-B', 'z'])).toEqual(new Uint8Array([3]));
  });

  it('returns IndexedDBStorageAdapter when indexedDB is defined (browser)', async () => {
    // Stub a minimal indexedDB so the selector picks the browser
    // path. IndexedDBStorageAdapter's constructor calls
    // `indexedDB.open(...)` synchronously and attaches lifecycle
    // callbacks to the result, so the fake request needs the three
    // event-handler slots.
    const fakeRequest = { onerror: null, onsuccess: null, onupgradeneeded: null };
    // @ts-expect-error — test-only stub of the global object.
    globalThis.indexedDB = { open: vi.fn(() => fakeRequest) };
    const adapter = buildStorageAdapter();
    const { IndexedDBStorageAdapter } = await import(
      '@automerge/automerge-repo-storage-indexeddb'
    );
    expect(adapter).toBeInstanceOf(IndexedDBStorageAdapter);
  });
});
