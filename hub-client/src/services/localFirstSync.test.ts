/**
 * Local-first durability (A2, bd-e2qnvb4a): a project created with no sync
 * server must persist to the local cache and be readable again after a
 * "reload" (a fresh sync client against the same IndexedDB), all with no
 * network adapter ever constructed.
 *
 * This test drives the real `createSyncClient` from quarto-sync-client
 * (aliased to source by hub-client's vitest config) over fake-indexeddb, so
 * it exercises the storage-only Repo path end to end.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { describe, it, expect, beforeEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';

import { createSyncClient } from '@quarto/quarto-sync-client';

function freshIndexedDb() {
  Object.defineProperty(globalThis, 'indexedDB', {
    value: new IDBFactory(),
    writable: true,
  });
}

function client() {
  return createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
}

describe('local-first document durability (no sync server)', () => {
  beforeEach(() => {
    freshIndexedDb();
  });

  it('persists a locally-created project across a reload with no network', async () => {
    // Create a project with NO syncServer → storage-only repo.
    const creator = client();
    const created = await creator.createNewProject({
      files: [{ path: 'hello.qmd', content: 'local durable\n', contentType: 'text' }],
      // default storage is 'indexeddb' — the fake global we installed above
    });
    expect(created.indexDocId).toBeTruthy();
    expect(creator.getSyncDiagnostics().connectedPeers).toBe(0);
    await creator.disconnect();

    // "Reload": a brand-new client re-opens the same doc id, still with no
    // sync server. It must read the file back out of the local cache.
    const reopened = client();
    const files = await reopened.connect('', created.indexDocId);
    expect(files.map((f) => f.path)).toContain('hello.qmd');
    expect(reopened.getFileContent('hello.qmd')).toBe('local durable\n');
    expect(reopened.getSyncDiagnostics().connectedPeers).toBe(0);
    await reopened.disconnect();
  });
});
