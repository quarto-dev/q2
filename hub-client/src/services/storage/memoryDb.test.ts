/**
 * Tests for the in-memory HubDatabase facade (bd-sw4xy1vw).
 *
 * The facade backs getDb() in ephemeral storage mode (q2 preview embed
 * build). It must support exactly the IDB surface the consumer modules
 * (projectStorage, userSettings, projectSetStorage) and the migration
 * helpers use — no more, no less — and behave like the real database
 * for those operations.
 */

import { describe, it, expect } from 'vitest';
import { createMemoryHubDatabase } from './memoryDb';
import { STORES, CURRENT_SCHEMA_VERSION } from './index';
import { getSchemaVersion } from './migrationRunner';
import { migratePointerToCollections } from './migrations';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import type { ProjectSetPointer, CollectionsPointer } from './types';

function makeEntry(id: string, indexDocId: string, lastAccessed: string): ProjectEntry {
  return {
    id,
    indexDocId,
    syncServer: '/ws',
    description: `Project ${id}`,
    createdAt: lastAccessed,
    lastAccessed,
  };
}

describe('createMemoryHubDatabase', () => {
  it('round-trips records keyed by the projects store keyPath (id)', async () => {
    const db = createMemoryHubDatabase();
    const entry = makeEntry('a', 'automerge:x', '2026-01-01T00:00:00.000Z');
    await db.put(STORES.PROJECTS, entry);
    expect(await db.get(STORES.PROJECTS, 'a')).toEqual(entry);
    expect(await db.get(STORES.PROJECTS, 'missing')).toBeUndefined();
  });

  it('round-trips records keyed by the key-keyed stores', async () => {
    const db = createMemoryHubDatabase();
    const pointer: ProjectSetPointer = {
      key: 'projectSet',
      projectSetDocId: 'automerge:root',
      syncServer: '/ws',
    };
    await db.put(STORES.PROJECT_SET, pointer);
    expect(await db.get(STORES.PROJECT_SET, 'projectSet')).toEqual(pointer);
  });

  it('overwrites on put with an existing key (IDB put semantics)', async () => {
    const db = createMemoryHubDatabase();
    await db.put(STORES.PROJECTS, makeEntry('a', 'automerge:x', '2026-01-01T00:00:00.000Z'));
    const updated = makeEntry('a', 'automerge:x', '2026-01-02T00:00:00.000Z');
    await db.put(STORES.PROJECTS, updated);
    expect(await db.get(STORES.PROJECTS, 'a')).toEqual(updated);
  });

  it('deletes records', async () => {
    const db = createMemoryHubDatabase();
    await db.put(STORES.PROJECTS, makeEntry('a', 'automerge:x', '2026-01-01T00:00:00.000Z'));
    await db.delete(STORES.PROJECTS, 'a');
    expect(await db.get(STORES.PROJECTS, 'a')).toBeUndefined();
  });

  it('supports index(indexDocId).get by field equality', async () => {
    const db = createMemoryHubDatabase();
    const entry = makeEntry('a', 'automerge:x', '2026-01-01T00:00:00.000Z');
    await db.put(STORES.PROJECTS, entry);
    await db.put(STORES.PROJECTS, makeEntry('b', 'automerge:y', '2026-01-02T00:00:00.000Z'));
    const found = await db
      .transaction(STORES.PROJECTS, 'readonly')
      .objectStore(STORES.PROJECTS)
      .index('indexDocId')
      .get('automerge:x');
    expect(found).toEqual(entry);
  });

  it('supports index(lastAccessed).getAll in ascending index order (IDB semantics)', async () => {
    const db = createMemoryHubDatabase();
    await db.put(STORES.PROJECTS, makeEntry('newest', 'automerge:c', '2026-01-03T00:00:00.000Z'));
    await db.put(STORES.PROJECTS, makeEntry('oldest', 'automerge:a', '2026-01-01T00:00:00.000Z'));
    await db.put(STORES.PROJECTS, makeEntry('middle', 'automerge:b', '2026-01-02T00:00:00.000Z'));
    const all = await db
      .transaction(STORES.PROJECTS, 'readonly')
      .objectStore(STORES.PROJECTS)
      .index('lastAccessed')
      .getAll();
    expect(all.map((e: ProjectEntry) => e.id)).toEqual(['oldest', 'middle', 'newest']);
  });

  it('reports all four stores as present and unknown stores as absent', () => {
    const db = createMemoryHubDatabase();
    for (const store of Object.values(STORES)) {
      expect(db.objectStoreNames.contains(store)).toBe(true);
    }
    expect(db.objectStoreNames.contains('bogus')).toBe(false);
  });

  it('is pre-seeded at the current schema version (migrations are skipped in ephemeral mode)', async () => {
    const db = createMemoryHubDatabase();
    expect(await getSchemaVersion(db)).toBe(CURRENT_SCHEMA_VERSION);
  });

  it('supports the migratePointerToCollections self-heal path', async () => {
    const db = createMemoryHubDatabase();
    const pointer: ProjectSetPointer = {
      key: 'projectSet',
      projectSetDocId: 'automerge:root',
      syncServer: '/ws',
    };
    await db.put(STORES.PROJECT_SET, pointer);
    await migratePointerToCollections(db);
    const migrated = await db.get(STORES.PROJECT_SET, 'collections');
    expect(migrated).toEqual({
      key: 'collections',
      collections: [{ projectSetDocId: 'automerge:root', syncServer: '/ws' }],
    });
    // Idempotent: a second run leaves the record untouched.
    await db.put(STORES.PROJECT_SET, {
      key: 'collections',
      collections: [],
    } satisfies CollectionsPointer);
    await migratePointerToCollections(db);
    expect(await db.get(STORES.PROJECT_SET, 'collections')).toEqual({
      key: 'collections',
      collections: [],
    });
  });

  it('close() is a safe no-op', async () => {
    const db = createMemoryHubDatabase();
    await db.put(STORES.PROJECTS, makeEntry('a', 'automerge:x', '2026-01-01T00:00:00.000Z'));
    db.close();
    // Data survives close(): the facade models a session-scoped store,
    // and closeDatabase() in projectStorage calls close() before reset.
    expect(await db.get(STORES.PROJECTS, 'a')).toBeDefined();
  });
});
