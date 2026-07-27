/**
 * Tests for projectSetStorage service.
 *
 * Verifies IndexedDB operations for the project set pointer —
 * the singleton that points to the Automerge-backed project set document.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import {
  getProjectSetPointer,
  setProjectSetPointer,
  clearProjectSetPointer,
  getCollectionPointers,
  setCollectionPointers,
  addCollectionPointer,
  removeCollectionPointer,
} from './projectSetStorage';
import { closeDatabase } from './projectStorage';

describe('projectSetStorage', () => {
  beforeEach(() => {
    closeDatabase();
    const idbFactory = new IDBFactory();
    Object.defineProperty(globalThis, 'indexedDB', {
      value: idbFactory,
      writable: true,
    });
  });

  afterEach(() => {
    closeDatabase();
  });

  it('should return null when no pointer is set', async () => {
    const pointer = await getProjectSetPointer();
    expect(pointer).toBeNull();
  });

  it('should store and retrieve a project set pointer', async () => {
    await setProjectSetPointer('automerge:abc123', 'wss://sync.example.com');

    const pointer = await getProjectSetPointer();
    expect(pointer).not.toBeNull();
    expect(pointer!.projectSetDocId).toBe('automerge:abc123');
    expect(pointer!.syncServer).toBe('wss://sync.example.com');
    expect(pointer!.key).toBe('projectSet');
  });

  it('should overwrite an existing pointer', async () => {
    await setProjectSetPointer('automerge:first', 'wss://server1');
    await setProjectSetPointer('automerge:second', 'wss://server2');

    const pointer = await getProjectSetPointer();
    expect(pointer!.projectSetDocId).toBe('automerge:second');
    expect(pointer!.syncServer).toBe('wss://server2');
  });

  it('should clear the pointer', async () => {
    await setProjectSetPointer('automerge:toDelete', 'wss://server');

    await clearProjectSetPointer();

    const pointer = await getProjectSetPointer();
    expect(pointer).toBeNull();
  });

  it('should handle clearing when no pointer exists', async () => {
    // Should not throw
    await clearProjectSetPointer();
    const pointer = await getProjectSetPointer();
    expect(pointer).toBeNull();
  });

  describe('collection pointers', () => {
    it('should return [] for a fresh browser', async () => {
      expect(await getCollectionPointers()).toEqual([]);
    });

    it('should self-heal from a legacy singleton pointer', async () => {
      await setProjectSetPointer('automerge:legacy1', 'wss://sync.example.com');
      const collections = await getCollectionPointers();
      expect(collections).toEqual([
        { projectSetDocId: 'automerge:legacy1', syncServer: 'wss://sync.example.com' },
      ]);
      // Legacy pointer is preserved as a safety net
      expect((await getProjectSetPointer())!.projectSetDocId).toBe('automerge:legacy1');
      // Conversion is stable across reads (idempotent)
      expect(await getCollectionPointers()).toEqual(collections);
    });

    it('should not re-convert once the collections record exists', async () => {
      await setProjectSetPointer('automerge:legacy1', 'wss://sync.example.com');
      await getCollectionPointers();
      // A later legacy-pointer change must not clobber the collections array
      await setProjectSetPointer('automerge:legacy2', 'wss://sync.example.com');
      const collections = await getCollectionPointers();
      expect(collections.map((c) => c.projectSetDocId)).toEqual(['automerge:legacy1']);
    });

    it('should add with dedupe and remove by doc id', async () => {
      await addCollectionPointer({ projectSetDocId: 'automerge:a', syncServer: 'wss://s1' });
      await addCollectionPointer({ projectSetDocId: 'automerge:b', syncServer: 'wss://s2' });
      await addCollectionPointer({ projectSetDocId: 'automerge:a', syncServer: 'wss://s1' });
      expect((await getCollectionPointers()).map((c) => c.projectSetDocId)).toEqual([
        'automerge:a',
        'automerge:b',
      ]);

      await removeCollectionPointer('automerge:a');
      expect((await getCollectionPointers()).map((c) => c.projectSetDocId)).toEqual([
        'automerge:b',
      ]);
    });

    it('pins the legacy-singleton (root) to the front when present but out of order', async () => {
      // Simulate a browser whose collections record was built by another path
      // first (e.g. the localStorage-collections migration), leaving the real
      // root — the legacy singleton — at a non-zero index. Root identity is
      // positional (collections[0]) elsewhere, so it must be normalized to front.
      await setProjectSetPointer('automerge:root', 'wss://s');
      await setCollectionPointers([
        { projectSetDocId: 'automerge:other1', syncServer: 'wss://s' },
        { projectSetDocId: 'automerge:root', syncServer: 'wss://s' },
        { projectSetDocId: 'automerge:other2', syncServer: 'wss://s' },
      ]);
      expect((await getCollectionPointers()).map((c) => c.projectSetDocId)).toEqual([
        'automerge:root',
        'automerge:other1',
        'automerge:other2',
      ]);
    });

    it('matches the root regardless of the automerge: prefix', async () => {
      // Real browsers store bare doc ids in the collections array but the
      // singleton may carry the prefix (or vice versa); matching normalizes it.
      await setProjectSetPointer('automerge:root', 'wss://s');
      await setCollectionPointers([
        { projectSetDocId: 'other1', syncServer: 'wss://s' },
        { projectSetDocId: 'root', syncServer: 'wss://s' },
      ]);
      expect((await getCollectionPointers()).map((c) => c.projectSetDocId)).toEqual([
        'root',
        'other1',
      ]);
    });

    it('leaves order unchanged when there is no legacy singleton', async () => {
      await setCollectionPointers([
        { projectSetDocId: 'automerge:a', syncServer: 'wss://s' },
        { projectSetDocId: 'automerge:b', syncServer: 'wss://s' },
      ]);
      expect((await getCollectionPointers()).map((c) => c.projectSetDocId)).toEqual([
        'automerge:a',
        'automerge:b',
      ]);
    });

    it('should replace the full array with setCollectionPointers', async () => {
      await setCollectionPointers([
        { projectSetDocId: 'automerge:x', syncServer: 'wss://s' },
      ]);
      expect((await getCollectionPointers()).length).toBe(1);
      await setCollectionPointers([]);
      expect(await getCollectionPointers()).toEqual([]);
    });
  });
});
