/**
 * Local-only project set (A1, bd-uvtx8qux): a user must be able to create
 * and populate a project set with no sync server and no auth. The set doc is
 * minted client-side, lives in the local cache, and survives a reload — with
 * no network adapter ever constructed.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';

import * as projectSetService from './projectSetService';

function freshIndexedDb() {
  Object.defineProperty(globalThis, 'indexedDB', {
    value: new IDBFactory(),
    writable: true,
  });
}

describe('local-only project set', () => {
  beforeEach(() => {
    projectSetService._resetForTesting();
    freshIndexedDb();
  });

  afterEach(async () => {
    await projectSetService.disconnect();
  });

  it('creates a local project set offline and lists an added project after reload', async () => {
    const docId = await projectSetService.createLocalProjectSet();
    expect(docId).toBeTruthy();

    // Add a local project (no syncServer).
    projectSetService.addProject({
      indexDocId: 'automerge:localProjA',
      description: 'My local project',
    });
    await projectSetService.flush();

    const listed = projectSetService.listProjects();
    expect(listed.map((p) => p.description)).toContain('My local project');
    expect(listed[0]!.syncServer).toBeUndefined();

    // "Reload": tear everything down and re-open the same set doc, still
    // with no network. The project must still be there.
    await projectSetService.disconnect();
    projectSetService._resetForTesting();

    const reopened = await projectSetService.connectLocal(docId);
    expect(reopened.map((p) => p.description)).toContain('My local project');
    expect(reopened[0]!.syncServer).toBeUndefined();
  });
});
