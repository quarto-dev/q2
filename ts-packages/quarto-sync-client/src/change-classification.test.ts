/**
 * Local-vs-remote change classification in trackPeers' `onHandleChange`.
 *
 * The classifier stamps `lastLocalChangeAt` for this client's own edits
 * and `lastRemoteChangeAt` for a collaborator's. It reads only change
 * *metadata* (getChangesMetaSince); decoding full changes made every
 * multi-MB create/upload in hub-mcp ~2x slower and turned the
 * exit-drain and upload tests red on main (Nov 2026).
 *
 * The sync-activity store is module-global, so the writer and the
 * reader below share it. The assertions still tell them apart: only
 * the reader can see a remote change.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { getDocSyncActivity, resetSyncActivity } from './sync-activity.js';
import { startTestHub, type TestHub } from './test-hub.js';

const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
  findDocRetry: { attempts: 1, baseDelayMs: 10 },
};

const liveClients: SyncClient[] = [];

function client(): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

describe('change classification', () => {
  let hub: TestHub;

  beforeEach(async () => {
    resetSyncActivity();
    hub = await startTestHub();
  });

  afterEach(async () => {
    for (const c of liveClients.splice(0)) await c.disconnect();
    await hub.stop();
  });

  it('records own edits as local and a collaborator\'s as remote', async () => {
    const writer = client();
    const result = await writer.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
      ...ONLINE,
    });
    const docId = result.files[0]!.docId;

    writer.updateFileContent('main.qmd', 'hello, local edit\n');
    await vi.waitFor(() => {
      expect(getDocSyncActivity(docId).lastLocalChangeAt).not.toBeNull();
    });
    // Nobody else has written yet: the writer's own edit is not remote.
    expect(getDocSyncActivity(docId).lastRemoteChangeAt).toBeNull();
    expect(await hub.hubHasHeadsOf(writer, 8000)).toBe(true);

    const reader = client();
    await reader.connect(hub.url, result.indexDocId, undefined, undefined, undefined, ONLINE);
    await vi.waitFor(() => {
      expect(reader.getFileContent('main.qmd')).toBe('hello, local edit\n');
    }, { timeout: 10000, interval: 50 });
    // The reader's initial load is not a fresh remote change.
    expect(getDocSyncActivity(docId).lastRemoteChangeAt).toBeNull();

    writer.updateFileContent('main.qmd', 'hello, collaborator edit\n');
    await vi.waitFor(() => {
      expect(reader.getFileContent('main.qmd')).toBe('hello, collaborator edit\n');
    }, { timeout: 10000, interval: 50 });
    await vi.waitFor(() => {
      expect(getDocSyncActivity(docId).lastRemoteChangeAt).not.toBeNull();
    });
  }, 30000);
});
