/**
 * D1 localization red tests (bd-10bdjmjb Phase 1): documents created
 * while the peer connection hasn't been established yet MUST reach
 * the hub once the connection arrives. Today they don't (production
 * evidence: the hello-claude.qmd dangling index entry, bd-8x482xb0).
 *
 * The JS-hub run localizes the defect: if the loss reproduces here,
 * the gap is client-side (announce-on-connect); the Rust-hub variant
 * (offline-creation-rust-hub.test.ts, binary-gated) discriminates a
 * samod accept-policy gap. Verdict belongs in the plan:
 * claude-notes/plans/2026-06-12-sync-client-offline-race.md.
 *
 * The repro mirrors production: the client starts while the hub is
 * unreachable (upgrades held = the restart window / lost 1 ms race),
 * creates a project + file in the silent offline-fallback mode, and
 * the connection lands moments later.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';

import { createSyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;

beforeEach(async () => {
  hub = await startTestHub({ holdUpgrades: true });
});

afterEach(async () => {
  await hub.stop();
});

function client() {
  return createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
}

describe('documents created during the offline-fallback window', () => {
  it('reach the hub once the peer connection arrives', async () => {
    const c = client();
    // Hub holds upgrades: the 50ms peer wait loses, createNewProject
    // proceeds in offline-fallback mode (today's silent default).
    const result = await c.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'made-offline.qmd', content: 'created in the window\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 50,
    });
    expect(result.indexDocId).toBeTruthy();
    const fileDocId = result.files[0]!.docId;

    // The connection arrives "moments later" — the production
    // sequence behind 'Peer connected - switching to online mode'.
    hub.releaseUpgrades();

    // Ground truth: the hub must end up holding BOTH documents.
    expect(await hub.hubHasDoc(result.indexDocId, 8000), 'index doc must reach the hub').toBe(true);
    expect(await hub.hubHasDoc(fileDocId, 8000), 'file doc must reach the hub').toBe(true);

    await c.disconnect();
  }, 30000);

  it('a second client can then open the file (the hello-claude scenario)', async () => {
    const creator = client();
    const created = await creator.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'visible.qmd', content: 'second client should see this\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 50,
    });
    hub.releaseUpgrades();
    // Give the creator a window to sync up before the reader arrives.
    await hub.hubHasDoc(created.indexDocId, 8000);

    const readerFiles: string[] = [];
    const reader = createSyncClient({
      onFileAdded: (path) => readerFiles.push(path),
      onFileChanged: () => {},
      onFileRemoved: () => {},
    });
    const files = await reader.connect(hub.url, created.indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    expect(files.map((f) => f.path)).toContain('visible.qmd');

    await creator.disconnect();
    await reader.disconnect();
  }, 30000);
});
