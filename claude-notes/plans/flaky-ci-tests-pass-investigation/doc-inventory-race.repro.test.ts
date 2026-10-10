/**
 * Repro for bd-c72wsugj / bd-fuw5gcni / bd-5sbguner / bd-tg3vnfxg: the
 * doc-inventory test drops the binary-file entry under load.
 *
 * Mechanism under test: automerge-repo's DocSynchronizer pushes local
 * changes through a trailing-edge asyncThrottle (100 ms). createNewProject
 * awaits computeSHA256 between creating main.qmd and logo.png. When that
 * await takes longer than the remaining throttle window (true under CPU
 * load), the index doc's first flush carries only main.qmd, and the
 * logo.png index entry sits in a second window that fires 100 ms later.
 * The test's hubHasDoc() only checks presence, so it is satisfied by the
 * partial index, and creator.disconnect() (drainMs 0) closes the socket
 * before the second flush. The reader then sees an index with one file.
 *
 * Case A below forces that schedule by delaying crypto.subtle.digest.
 * Case B shows that draining the creator's disconnect closes the hole.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  vi.restoreAllMocks();
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

const TINY_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk' +
  'YPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

/** Slow the hash the creator awaits between main.qmd and logo.png. */
function slowDigest(delayMs: number): void {
  const real = crypto.subtle.digest.bind(crypto.subtle);
  vi.spyOn(crypto.subtle, 'digest').mockImplementation(async (alg, data) => {
    await new Promise((r) => setTimeout(r, delayMs));
    return real(alg, data);
  });
}

async function createProjectOnHub(drainMs?: number) {
  const creator = client();
  const result = await creator.createNewProject({
    syncServer: hub.url,
    files: [
      { path: 'main.qmd', content: 'hello inventory\n', contentType: 'text' },
      { path: 'logo.png', content: TINY_PNG_B64, contentType: 'binary', mimeType: 'image/png' },
    ],
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);
  for (const f of result.files) {
    expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
  }
  const report = await creator.disconnect(drainMs ? { drainMs } : undefined);
  return { indexDocId: result.indexDocId, report };
}

async function readerInventory(indexDocId: string) {
  const reader = client();
  await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
    findDocRetry: { attempts: 1, baseDelayMs: 10 },
  });
  return reader.getDocInventory().map((e) => [e.role, e.path]);
}

describe('doc-inventory race repro', () => {
  it('A: a slow hash between file creations loses the logo.png index entry (expected to FAIL)', async () => {
    slowDigest(150);
    const { indexDocId } = await createProjectOnHub();
    expect(await readerInventory(indexDocId)).toEqual([
      ['index', null],
      ['binary-file', 'logo.png'],
      ['file', 'main.qmd'],
    ]);
  });

  it('B: the same schedule with a drained disconnect keeps every entry', async () => {
    slowDigest(150);
    const { indexDocId, report } = await createProjectOnHub(5000);
    expect(report.drained).toBe(true);
    expect(await readerInventory(indexDocId)).toEqual([
      ['index', null],
      ['binary-file', 'logo.png'],
      ['file', 'main.qmd'],
    ]);
  });
});
