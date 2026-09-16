/**
 * Characterization test for H2 in
 * claude-notes/research/2026-09-16-automerge-index-doc-staleness.md
 * (bd-6f21d4c6): `Repo#saveSyncState` (automerge-repo v2.5.6,
 * `Repo.ts:377-403`) throttles persisted sync-state writes with one
 * `asyncThrottle` instance **per remote storageId**, shared across every
 * document synced with that peer — not one throttle per document. Two
 * documents' `sync-state` events landing in the same 100ms window silently
 * drop the earlier document's persisted write: `asyncThrottle` cancels the
 * pending call and reschedules with only the latest args (`helpers/
 * throttle.ts:85-126`).
 *
 * This is not a fix-driving red test — it demonstrates *existing, real*
 * upstream behavior, so it is expected to pass against the current
 * dependency version. It exists to prove the mechanism is real (not just
 * plausible from reading the source) before any fix is considered.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { next as A } from '@automerge/automerge/slim';
import type { DocumentId, PeerId } from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';
import { MemoryStorageAdapter } from './storage-adapter.js';

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  vi.useRealTimers();
  for (const c of liveClients.splice(0)) {
    await c.disconnect();
  }
  await hub.stop();
});

function client(): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

async function pollUntil(check: () => Promise<boolean>, timeoutMs = 8000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await check()) return true;
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
}

describe('Repo#saveSyncState per-storageId throttle (H2)', () => {
  it("drops a document's persisted sync-state save when another document's fires in the same 100ms window, but the sync protocol still converges afterward", async () => {
    const creator = client();
    const result = await creator.createNewProject({
      syncServer: hub.url,
      files: [
        { path: 'a.qmd', content: 'A\n', contentType: 'text' },
        { path: 'b.qmd', content: 'B\n', contentType: 'text' },
      ],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);

    const repo = creator.getRepo();
    expect(repo).not.toBeNull();

    // The hub is this client's only peer; its storageId is what
    // Repo#saveSyncState throttles on.
    const peerId = Object.keys(repo!.peerMetadataByPeerId)[0] as PeerId | undefined;
    expect(peerId).toBeDefined();

    const docIdA = result.files.find((f) => f.path === 'a.qmd')!.docId as DocumentId;
    const docIdB = result.files.find((f) => f.path === 'b.qmd')!.docId as DocumentId;

    const saveSpy = vi.spyOn(MemoryStorageAdapter.prototype, 'save');
    saveSpy.mockClear();

    vi.useFakeTimers();
    try {
      // Doc A's sync-state changes first.
      repo!.synchronizer.emit('sync-state', {
        peerId: peerId!,
        documentId: docIdA,
        syncState: A.initSyncState(),
      });
      // Still inside the 100ms debounce window (Repo's default
      // #saveDebounceRate) when doc B's sync-state changes.
      await vi.advanceTimersByTimeAsync(50);
      repo!.synchronizer.emit('sync-state', {
        peerId: peerId!,
        documentId: docIdB,
        syncState: A.initSyncState(),
      });
      // Past the debounce window: the throttle handler should have run
      // exactly once, for the latest (doc B) args only.
      await vi.advanceTimersByTimeAsync(200);
    } finally {
      vi.useRealTimers();
    }

    const syncStateSaveDocIds = saveSpy.mock.calls
      .filter(([key]) => (key as string[])[1] === 'sync-state')
      .map(([key]) => (key as string[])[0]);

    // The mechanism: doc A's persisted sync-state write never happened;
    // only doc B's did, because both shared one throttle handler keyed by
    // the hub's storageId (Repo.ts's `#throttledSaveSyncStateHandlers`).
    expect(syncStateSaveDocIds).not.toContain(docIdA);
    expect(syncStateSaveDocIds).toContain(docIdB);

    // Compounding-factor framing, not standalone cause: the dropped
    // *persisted* write does not touch DocSynchronizer's own live
    // in-memory sync state, so real message exchange for doc A still
    // converges normally afterward.
    creator.updateFileContent('a.qmd', 'A edited after the throttle drop\n');

    const converged = await pollUntil(async () => {
      const handle = await hub.repo.find<{ text: string }>(docIdA);
      return handle.doc()?.text === 'A edited after the throttle drop\n';
    });
    expect(converged).toBe(true);
  });
});
