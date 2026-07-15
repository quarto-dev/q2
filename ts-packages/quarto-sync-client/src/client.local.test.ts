/**
 * Local-first mode (A2, bd-e2qnvb4a): the sync client must be able to
 * create and edit a project with NO sync server at all — a storage-only
 * Repo with no network adapter. Authentication and a hub are required
 * only when the user later connects.
 *
 * These tests run with `storage: 'memory'` and never start a hub, so a
 * regression that reintroduces a mandatory network adapter shows up as a
 * hang or a connection error rather than a pass.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { describe, it, expect } from 'vitest';

import { createSyncClient } from './client.js';

function client() {
  return createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
}

describe('local-only project creation (no sync server)', () => {
  it('createNewProject with no syncServer builds a storage-only repo', async () => {
    const c = client();
    const result = await c.createNewProject({
      files: [{ path: 'notes.qmd', content: 'local first\n', contentType: 'text' }],
      storage: 'memory',
    });

    expect(result.indexDocId).toBeTruthy();
    expect(result.files).toHaveLength(1);
    // No server was contacted, so there is no peer.
    expect(c.getSyncDiagnostics().connectedPeers).toBe(0);
    // The document was authored locally and is readable with no network.
    expect(c.getFileContent('notes.qmd')).toBe('local first\n');
    expect(c.isConnected()).toBe(true);

    await c.disconnect();
  });

  it('does not hang or reject when no hub is reachable', async () => {
    const c = client();
    // If this path still built a WebSocket adapter, the 1 ms peer wait
    // would fire and we would fall into offline mode — here we assert the
    // create simply succeeds against local storage.
    await expect(
      c.createNewProject({
        files: [{ path: 'a.qmd', content: 'x\n', contentType: 'text' }],
        storage: 'memory',
      }),
    ).resolves.toMatchObject({ files: [{ path: 'a.qmd' }] });
    await c.disconnect();
  });
});

describe('local-only connect (no sync server)', () => {
  it('connect with an empty syncServer never contacts a network', async () => {
    // A doc that does not exist locally: in local mode this must fail fast
    // with the "not in local storage" verdict, NOT wait on a phantom peer.
    const c = client();
    const bogus = 'automerge:2j9knpCsexT8gPeMFXQCLbTQGKC';
    await expect(
      c.connect('', bogus, undefined, undefined, undefined, {
        storage: 'memory',
      }),
    ).rejects.toThrow();
    expect(c.getSyncDiagnostics().connectedPeers).toBe(0);
    await c.disconnect();
  });
});
