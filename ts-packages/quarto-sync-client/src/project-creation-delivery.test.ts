/**
 * Delivery of a freshly created project to the hub, under the schedule
 * that made doc-inventory.test.ts flaky (bd-c72wsugj, also filed as
 * bd-fuw5gcni, bd-5sbguner and bd-tg3vnfxg).
 *
 * automerge-repo's DocSynchronizer pushes local changes through a
 * trailing-edge throttle (100 ms per doc). createNewProject awaits
 * computeSHA256 between creating a text file and a binary file; when
 * that await outlasts the rest of the throttle window (true under the
 * CPU load of a full `cargo xtask verify`), the index doc's first flush
 * carries only the text file's entry, and the binary file's entry sits
 * in a second window that fires 100 ms later. A presence check on the
 * hub (`hubHasDoc`) is satisfied by the partial index, and a creator
 * that disconnects right after it takes the second flush with it. The
 * reader then sees an index with one file.
 *
 * The digest delay below forces that schedule on every run (five of
 * five locally, 2026-10-10). The test asserts that the shared helper
 * discipline (wait for the creator's *heads* on the hub, `hubHasHeadsOf`)
 * delivers every entry. Dropping that wait back to `hubHasDoc` fails
 * this test deterministically with the CI diff.
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

/** 1x1 transparent PNG, base64 (createNewProject takes binary as base64). */
const TINY_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk' +
  'YPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

/** Longer than the 100 ms sync throttle, so the two index changes land in different windows. */
const DIGEST_DELAY_MS = 150;

/** Slow the hash the creator awaits between main.qmd and logo.png. */
function slowDigest(delayMs: number): void {
  const real = crypto.subtle.digest.bind(crypto.subtle);
  vi.spyOn(crypto.subtle, 'digest').mockImplementation(async (alg, data) => {
    await new Promise((r) => setTimeout(r, delayMs));
    return real(alg, data);
  });
}

describe('project creation reaches the hub in full', () => {
  it('a slow hash between a text and a binary file does not lose the binary file’s index entry', async () => {
    slowDigest(DIGEST_DELAY_MS);

    const creator = client();
    const result = await creator.createNewProject({
      syncServer: hub.url,
      files: [
        { path: 'main.qmd', content: 'hello delivery\n', contentType: 'text' },
        { path: 'logo.png', content: TINY_PNG_B64, contentType: 'binary', mimeType: 'image/png' },
      ],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    expect(result.files.map((f) => f.path).sort()).toEqual(['logo.png', 'main.qmd']);

    // Presence is not delivery: wait for the creator's heads, then let it go.
    expect(await hub.hubHasHeadsOf(creator, 8000), 'the hub must hold every change the creator made').toBe(true);
    await creator.disconnect();

    const reader = client();
    await reader.connect(hub.url, result.indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: { attempts: 1, baseDelayMs: 10 },
    });
    expect(reader.getDocInventory().map((e) => [e.role, e.path])).toEqual([
      ['index', null],
      ['binary-file', 'logo.png'],
      ['file', 'main.qmd'],
    ]);
  });
});
