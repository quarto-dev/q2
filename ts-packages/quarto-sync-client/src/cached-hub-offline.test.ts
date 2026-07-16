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

import { startTestHub, type TestHub } from './test-hub.js';
import {
  createSyncedHubProject,
  goOffline,
  goOnline,
  reopenCachedOffline,
  waitForHubFileText,
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

  // ── Behavioral specs the later phases implement (red→green with impl) ──

  // B1 (bd-qklxdkwh) — the app-level open path currently prompts sign-in
  // for a logged-off hub open (openActor → onNeedsSignIn). B1 makes a
  // *cached* hub project open under the local actor instead; only a
  // genuinely never-cached + offline project reports a precise
  // "can't open" reason. (Unit-tested at the openActor seam; see
  // openActor.test.ts.)
  it.todo('B1: cached hub project opens offline under the local actor (no sign-in prompt)');
  it.todo('B1: never-cached hub project offline reports a precise "not cached" reason');

  // B2 (bd-ab44wv07) — offline edits author under the local actor and
  // write identities[localActor] so they display as this human, not an
  // 8-hex stub, and persist across reload.
  it.todo('B2: offline edits author under the local actor with an identities row');

  // B3 (bd-g5apu5bm) — on reconnect: fetch the HMAC stableActor,
  // applyActorId to all handles, bridge identities[stableActor] =
  // identities[localActor], so the offline window and online edits
  // display as one human.
  it.todo('B3: reconnect switches to the HMAC actor and bridges identities to one human');
});
