/**
 * Persisted per-browser local actor (A3, bd-gxz6tqbk).
 *
 * Local edits must attribute to one coherent author across reloads instead of
 * a fresh random 8-hex stub each time (today's null-actor behaviour). A stable
 * 32-hex Automerge actor is minted once per browser, persisted in IndexedDB,
 * and used to author every local document — with an `identities` row so the
 * author displays a name, not a hex prefix.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { describe, it, expect, beforeEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';

import { getOrCreateLocalActor } from './userSettings';
import { closeDatabase } from './projectStorage';
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

describe('persisted local actor', () => {
  beforeEach(() => {
    closeDatabase();
    freshIndexedDb();
  });

  it('mints a valid 32-hex actor that is stable across calls and reloads', async () => {
    const first = await getOrCreateLocalActor();
    // 16 random bytes → 32 lowercase hex chars: a valid Automerge actor id.
    expect(first).toMatch(/^[0-9a-f]{32}$/);

    // Same value within a session.
    expect(await getOrCreateLocalActor()).toBe(first);

    // "Reload": close and re-open the DB — must read back the same actor,
    // not re-randomize.
    closeDatabase();
    expect(await getOrCreateLocalActor()).toBe(first);
  });

  it('authors local documents under the local actor with an identities row', async () => {
    const localActor = await getOrCreateLocalActor();

    // Create a local project authored under the local actor.
    const creator = client();
    const created = await creator.createNewProject(
      { files: [{ path: 'me.qmd', content: 'mine\n', contentType: 'text' }] },
      localActor,
      'You',
      '#E91E63',
    );
    expect(creator.getActorId()).toBe(localActor);
    // The index identities row maps the local actor → display name.
    const idx = creator.getIndexHandle()?.doc();
    expect(idx?.identities?.[localActor]).toEqual({ name: 'You', color: '#E91E63' });
    await creator.disconnect();

    // "Reload": re-open the same project under the same local actor. The
    // author does not re-randomize.
    const reopened = client();
    await reopened.connect('', created.indexDocId, localActor, 'You', '#E91E63');
    expect(reopened.getActorId()).toBe(localActor);
    const idx2 = reopened.getIndexHandle()?.doc();
    expect(idx2?.identities?.[localActor]).toEqual({ name: 'You', color: '#E91E63' });
    await reopened.disconnect();
  });
});
