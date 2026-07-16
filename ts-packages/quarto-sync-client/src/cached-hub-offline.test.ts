/**
 * Offline-cached-hub-project timeline — sync-client baseline (B0,
 * bd-ysusqcb3; epic bd-xxjy9yfp).
 *
 * These are the *scaffolding* tests for the offline-cached feature. They
 * establish, at the sync-client layer, what already works today so the
 * B1–B3 app-layer work (actor resolution + reconnect display-bridge) is
 * localized — the same way offline-creation.test.ts localized D1.
 *
 * The green assertions below are the harness B1–B3 build on. The
 * `it.todo` entries enumerate the behavioral specs each later phase adds
 * (with its implementation, per the team's red-then-green-per-phase
 * cadence — a red test is never committed alone).
 *
 * Plan: claude-notes/plans/2026-07-15-hub-client-offline-cached-hub-projects.md
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { getAllChanges, decodeChange, type Doc } from '@automerge/automerge';

import { isDocCached } from './storage-adapter.js';
import { startTestHub, type TestHub } from './test-hub.js';
import {
  createSyncedHubProject,
  goOffline,
  goOnline,
  reopenCachedOffline,
  readIdentities,
  waitForHubFileText,
  waitForOnline,
} from './cached-hub.js';

// A stable per-browser local actor stand-in (32 hex = a valid Automerge
// actor id), plus a hub-side "HMAC" actor for the online create. Both
// must be valid hex — Automerge rejects a non-hex actor id.
const LOCAL_ACTOR = 'ffff0000ffff0000ffff0000ffff0000';
const HUB_ACTOR = 'aaaa1111bbbb2222cccc3333dddd4444';

function freshIndexedDb() {
  Object.defineProperty(globalThis, 'indexedDB', {
    value: new IDBFactory(),
    writable: true,
  });
}

let hub: TestHub;

beforeEach(async () => {
  freshIndexedDb();
  hub = await startTestHub();
});

afterEach(async () => {
  await hub.stop();
});

describe('offline-cached hub project (sync-client baseline)', () => {
  it('reopens a cached hub project offline read+write and flushes edits on reconnect', async () => {
    // The whole online → offline → reconnect timeline at the sync-client
    // layer. What passes here is what B1–B3 build on; it localizes the
    // remaining work to the app layer (actor resolution + display bridge).
    const project = await createSyncedHubProject(hub, {
      path: 'notes.qmd',
      content: 'online body\n',
      actor: HUB_ACTOR,
      screenName: 'Alice',
      color: '#3366cc',
    });

    // The connection is lost mid-session; the URL stays the same.
    goOffline(hub);

    // A fresh client (a browser reload) reopens the cached project while
    // the hub is unreachable. The sync client degrades to
    // offline-from-cache — no peer, but the file reads back.
    const reopened = await reopenCachedOffline(hub, project, {
      actor: LOCAL_ACTOR,
      screenName: 'You',
      color: '#cc6633',
    });
    expect(reopened.files.map((f) => f.path)).toContain('notes.qmd');
    expect(reopened.client.getFileContent('notes.qmd')).toBe('online body\n');
    expect(reopened.client.getSyncDiagnostics().connectedPeers).toBe(0);

    // Offline edit: it persists locally with no peer.
    reopened.client.updateFileContent('notes.qmd', 'edited offline\n');
    await reopened.client.flush();
    expect(reopened.client.getFileContent('notes.qmd')).toBe('edited offline\n');
    expect(reopened.client.getSyncDiagnostics().connectedPeers).toBe(0);

    // Reconnect: the adapter re-establishes the peer and the offline edit
    // syncs up. This answers B4's open question for *existing* docs — an
    // edit to an already-synced doc reaches the hub via normal automerge
    // sync, without the D1 announce-on-connect fix (bd-10bdjmjb, which was
    // about *newly created* docs).
    goOnline(hub);
    const reached = await waitForHubFileText(hub, project.fileDocId, 'edited offline\n', 15000);
    expect(reached, 'offline edit must reach the hub after reconnect').toBe(true);

    await reopened.client.disconnect();
  }, 30000);

  it('isDocCached distinguishes a synced project from a never-seen doc', async () => {
    // The probe the openActor seam uses to decide "open from cache" vs
    // "prompt sign-in / report offline-unopenable" (B1, bd-qklxdkwh).
    const project = await createSyncedHubProject(hub, {
      path: 'notes.qmd',
      content: 'online body\n',
      actor: HUB_ACTOR,
      screenName: 'Alice',
      color: '#3366cc',
    });

    expect(await isDocCached(project.indexDocId)).toBe(true);
    // A well-formed but never-seen doc id is not cached.
    expect(await isDocCached('automerge:2j9knpCsexT8gPeMFXQCLbTQGKC')).toBe(false);
    // A malformed id is not cached (and does not throw).
    expect(await isDocCached('not-an-automerge-url')).toBe(false);
  }, 30000);

  it('B2: offline edits author under the local actor with an identities row', async () => {
    // One human ("Charlie") editing their hub project offline. The edit
    // must attribute to the local actor with an identities row (so it shows
    // as Charlie, not an 8-hex stub) and persist across a reload.
    const project = await createSyncedHubProject(hub, {
      path: 'notes.qmd',
      content: 'online body\n',
      actor: HUB_ACTOR,
      screenName: 'Charlie',
      color: '#3366cc',
    });

    goOffline(hub);
    const c1 = await reopenCachedOffline(hub, project, {
      actor: LOCAL_ACTOR,
      screenName: 'Charlie',
      color: '#cc6633',
    });
    c1.client.updateFileContent('notes.qmd', 'offline edit by charlie\n');
    await c1.client.flush();

    // Authorship: the client authors under the local actor, and the latest
    // change on the file doc carries it.
    expect(c1.client.getActorId()).toBe(LOCAL_ACTOR);
    const fileDoc = c1.client.getFileHandle('notes.qmd')?.doc() as Doc<unknown>;
    const latest = getAllChanges(fileDoc).map((ch) => decodeChange(ch));
    expect(latest[latest.length - 1]!.actor).toBe(LOCAL_ACTOR);

    // Display: identities has a row for the local actor with the human name.
    expect(readIdentities(c1.client)[LOCAL_ACTOR]).toMatchObject({ name: 'Charlie' });

    // Disconnect the first session cleanly (one client at a time): connect
    // the held socket, wait for the peer, then close it.
    goOnline(hub);
    await waitForOnline(c1);
    await c1.client.disconnect();

    // Reload (a fresh offline client, NO screenName so nothing re-writes the
    // row): both the edit and the identity must come back from the cache.
    goOffline(hub);
    const c2 = await reopenCachedOffline(hub, project, { actor: LOCAL_ACTOR });
    expect(c2.client.getFileContent('notes.qmd')).toBe('offline edit by charlie\n');
    expect(readIdentities(c2.client)[LOCAL_ACTOR]).toMatchObject({ name: 'Charlie' });

    goOnline(hub);
    await waitForOnline(c2);
    await c2.client.disconnect();
  }, 30000);

  it('B3: reconnect switches to the HMAC actor and bridges authorship to one human', async () => {
    // One human ("Charlie"): create online (HMAC actor), edit offline (local
    // actor), reconnect → switchActor back to the HMAC actor and bridge the
    // identity, then edit again.
    const project = await createSyncedHubProject(hub, {
      path: 'notes.qmd',
      content: 'online body\n',
      actor: HUB_ACTOR,
      screenName: 'Charlie',
      color: '#3366cc',
    });

    goOffline(hub);
    const c = await reopenCachedOffline(hub, project, {
      actor: LOCAL_ACTOR,
      screenName: 'Charlie',
      color: '#3366cc',
    });
    c.client.updateFileContent('notes.qmd', 'edited offline by charlie\n');
    await c.client.flush();
    expect(c.client.getActorId()).toBe(LOCAL_ACTOR);

    // Reconnect: peer comes back, then the app resolves the HMAC actor and
    // switches to it (the App wires this on onConnectionChange).
    goOnline(hub);
    await waitForOnline(c);
    c.client.switchActor(HUB_ACTOR, 'Charlie', '#3366cc');
    expect(c.client.getActorId()).toBe(HUB_ACTOR);

    // Future edits carry the HMAC actor; the offline change still carries the
    // local actor (history is immutable, never rewritten).
    c.client.updateFileContent('notes.qmd', 'edited online after reconnect\n');
    await c.client.flush();
    const changes = getAllChanges(
      c.client.getFileHandle('notes.qmd')?.doc() as Doc<unknown>,
    ).map((ch) => decodeChange(ch));
    const actors = new Set(changes.map((ch) => ch.actor));
    expect(changes[changes.length - 1]!.actor).toBe(HUB_ACTOR);
    expect(actors.has(LOCAL_ACTOR)).toBe(true);
    expect(actors.has(HUB_ACTOR)).toBe(true);

    // Display bridge: both actors resolve to the same human, so the whole
    // timeline reads as one person.
    const ids = readIdentities(c.client);
    expect(ids[LOCAL_ACTOR]).toMatchObject({ name: 'Charlie' });
    expect(ids[HUB_ACTOR]).toMatchObject({ name: 'Charlie' });

    // Sync-up: the offline edit and the post-reconnect edit both reach the hub.
    expect(
      await waitForHubFileText(hub, project.fileDocId, 'edited online after reconnect\n', 15000),
      'edits must reach the hub after reconnect',
    ).toBe(true);

    await c.client.disconnect();
  }, 30000);

  it('B4: N offline edits to existing cached docs all reach the hub on reconnect', async () => {
    // Widens the baseline finding to N docs: editing several existing cached
    // docs offline and reconnecting flushes ALL of them via normal automerge
    // sync — no D1 announce-on-connect fix needed (D1, bd-10bdjmjb, is about
    // *newly created* offline docs, which this test deliberately does not do).
    const N = 4;
    const project = await createSyncedHubProject(hub, {
      files: Array.from({ length: N }, (_, i) => ({
        path: `file-${i}.qmd`,
        content: `online ${i}\n`,
      })),
      actor: HUB_ACTOR,
      screenName: 'Charlie',
      color: '#3366cc',
    });

    goOffline(hub);
    const c = await reopenCachedOffline(hub, project, {
      actor: LOCAL_ACTOR,
      screenName: 'Charlie',
    });
    // Edit every file offline.
    for (let i = 0; i < N; i++) {
      c.client.updateFileContent(`file-${i}.qmd`, `offline edit ${i}\n`);
    }
    await c.client.flush();

    goOnline(hub);
    // All N updated docs must reach the hub.
    for (const f of project.files) {
      const i = project.files.indexOf(f);
      expect(
        await waitForHubFileText(hub, f.fileDocId, `offline edit ${i}\n`, 15000),
        `${f.path} must reach the hub after reconnect`,
      ).toBe(true);
    }

    await waitForOnline(c);
    await c.client.disconnect();
  }, 40000);
});
