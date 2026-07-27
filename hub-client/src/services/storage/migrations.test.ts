/**
 * Tests for the v5 migration: the singleton project-set pointer becomes a
 * collections array (old non-collection project list → collection-driven).
 *
 * See claude-notes/instructions/hub-client-storage.md and
 * claude-notes/plans/2026-07-10-collections-as-project-sets.md.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { openDB, type IDBPDatabase } from 'idb';
import {
  migrations,
  getMigrationsFrom,
  migratePointerToCollections,
  CURRENT_SCHEMA_VERSION,
} from './migrations';
import { STORES } from './types';

async function openWithProjectSetStore(): Promise<IDBPDatabase> {
  return openDB('migration-test', 1, {
    upgrade(db) {
      db.createObjectStore(STORES.PROJECT_SET, { keyPath: 'key' });
    },
  });
}

describe('v5 migration: pointer → collections', () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, 'indexedDB', { value: new IDBFactory(), writable: true });
  });
  afterEach(() => {
    // fresh factory each test
  });

  it('registers v5 as a transform-only migration', () => {
    expect(CURRENT_SCHEMA_VERSION).toBe(5);
    const v5 = migrations.find((m) => m.version === 5);
    expect(v5).toBeDefined();
    expect(typeof v5!.transform).toBe('function');
    // Transform-only: no structural change (DB version does not bump for v5)
    expect(v5!.structural).toBeUndefined();
    // getMigrationsFrom(4) must include v5 so an existing v4 browser runs it
    expect(getMigrationsFrom(4).map((m) => m.version)).toContain(5);
  });

  it('converts a singleton pointer into a one-element collections array', async () => {
    const db = await openWithProjectSetStore();
    await db.put(STORES.PROJECT_SET, {
      key: 'projectSet',
      projectSetDocId: 'automerge:abc123',
      syncServer: 'wss://sync.example.com',
    });

    await migratePointerToCollections(db);

    const collections = await db.get(STORES.PROJECT_SET, 'collections');
    expect(collections).toEqual({
      key: 'collections',
      collections: [{ projectSetDocId: 'automerge:abc123', syncServer: 'wss://sync.example.com' }],
    });
    // Legacy singleton is retained as a safety net
    expect(await db.get(STORES.PROJECT_SET, 'projectSet')).not.toBeUndefined();
    db.close();
  });

  it('is idempotent — does not clobber an existing collections array', async () => {
    const db = await openWithProjectSetStore();
    await db.put(STORES.PROJECT_SET, { key: 'projectSet', projectSetDocId: 'automerge:abc', syncServer: 'wss://s' });
    await migratePointerToCollections(db);
    // A later change to the collections array must survive a re-run
    await db.put(STORES.PROJECT_SET, {
      key: 'collections',
      collections: [
        { projectSetDocId: 'automerge:abc', syncServer: 'wss://s' },
        { projectSetDocId: 'automerge:joined', syncServer: 'wss://s' },
      ],
    });
    await migratePointerToCollections(db);
    const collections = await db.get(STORES.PROJECT_SET, 'collections');
    expect(collections.collections).toHaveLength(2);
    db.close();
  });

  it('no-ops for a fresh browser with no pointer', async () => {
    const db = await openWithProjectSetStore();
    await migratePointerToCollections(db);
    expect(await db.get(STORES.PROJECT_SET, 'collections')).toBeUndefined();
    db.close();
  });
});
