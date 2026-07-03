/**
 * getSyncDiagnostics() — introspection used by the e2e smoke-diag
 * classifier (claude-notes/research/2026-07-03-e2e-reliability-experiment-log.md,
 * lead 2).
 *
 * Contract: for every file the index references that has NOT loaded
 * (no ready handle), report the underlying automerge-repo DocHandle
 * state (`loading` / `requesting` / `unavailable` / null when no handle
 * was ever cached) plus whether the sync client's own unavailable
 * marker is set, alongside connected-peer count and the retry-poll's
 * tick counter / timer state. This is what lets a nightly failure log
 * distinguish "request lost in flight" from "terminal unavailable
 * verdict" from "storage load never finished" — mechanisms that are
 * indistinguishable in the render-timeout symptom.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
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

async function createProjectOnHub(
  files: Array<{ path: string; content: string }>,
): Promise<string> {
  const creator = client();
  const result = await creator.createNewProject({
    syncServer: hub.url,
    files: files.map((f) => ({ ...f, contentType: 'text' as const })),
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  // Wait for the hub to actually hold every doc before the creator
  // disconnects — creation syncs in the background (same discipline as
  // dangling-entries.test.ts).
  expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);
  for (const f of result.files) {
    expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
  }
  await creator.disconnect();
  return result.indexDocId;
}

/** Mint an index entry pointing at a doc no repo holds. */
async function mintGhostEntry(indexDocId: string, path: string): Promise<string> {
  const ghostId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
  const handle = await hub.repo.find<{ files: Record<string, string> }>(
    indexDocId as DocumentId,
  );
  handle.change((d) => {
    d.files[path] = ghostId;
  });
  return ghostId;
}

const FAST_RETRY = { attempts: 1, baseDelayMs: 10 };

describe('getSyncDiagnostics', () => {
  it('reports a stranded file with its DocHandle state and unavailable marker', async () => {
    const indexDocId = await createProjectOnHub([
      { path: 'main.qmd', content: 'loads fine\n' },
    ]);
    const ghostId = await mintGhostEntry(indexDocId, 'ghost.qmd');

    const reader = client();
    await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });

    const diag = reader.getSyncDiagnostics();

    // Healthy state around the stranded file.
    expect(diag.connectedPeers).toBeGreaterThanOrEqual(1);
    // The loaded file is NOT reported — only stranded entries are.
    expect(diag.stranded.map((s) => s.path)).toEqual(['ghost.qmd']);

    const ghost = diag.stranded[0];
    expect(ghost.docId).toContain(ghostId);
    // The sync client marked it unavailable at connect...
    expect(ghost.unavailableMarker).toBe(true);
    // ...and the repo-level handle verdict is visible. For a doc the hub
    // explicitly reports missing this is 'unavailable'; the field's job is
    // to expose whatever state the handle is actually in.
    expect(ghost.handleState).toBe('unavailable');

    // The bounded retry poll is scheduled (unavailable entries exist and a
    // peer is connected).
    expect(diag.retryTimerActive).toBe(true);
    expect(diag.unavailableRetryTicks).toBeGreaterThanOrEqual(0);
  }, 30000);

  it('reports no stranded files for a fully-loaded project', async () => {
    const indexDocId = await createProjectOnHub([
      { path: 'a.qmd', content: 'a\n' },
      { path: 'b.qmd', content: 'b\n' },
    ]);

    const reader = client();
    await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });

    const diag = reader.getSyncDiagnostics();
    expect(diag.stranded).toEqual([]);
    expect(diag.retryTimerActive).toBe(false);
    expect(diag.connectedPeers).toBeGreaterThanOrEqual(1);
  }, 30000);
});
