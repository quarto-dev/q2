/**
 * Exit-drain regression tests (bd-10deu8h4) — the 2026-06-12 incident,
 * miniaturized at the sync-client level.
 *
 * The accident: an MCP host created documents over a live connection
 * (requireOnline, memory storage) and tore the client down immediately
 * afterwards. `disconnect()` killed the adapter before outbound sync
 * finished; the documents' only copy died with the process, leaving a
 * dangling index entry on the hub.
 *
 * Contract under test: `disconnect({ drainMs })` gives outbound sync a
 * bounded window to reach the hub, returns early on confirmation, and
 * reports what it could not confirm. Ground truth is always the hub's
 * own repo (`hubHasDoc`), never client-side state.
 */

import { describe, it, expect, afterEach } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';
import type { SyncClientCallbacks } from './types.js';

const noopCallbacks: SyncClientCallbacks = {
  onFileAdded: () => {},
  onFileChanged: () => {},
  onBinaryChanged: () => {},
  onFileRemoved: () => {},
};

// Enough payload to keep delivery genuinely in flight when disconnect
// is called right after creation — a single tiny file can win the race
// by luck, which would mask the defect (see plan, Phase 1).
const FILE_COUNT = 8;
const FILE_BYTES = 64 * 1024;

function accidentFiles() {
  return Array.from({ length: FILE_COUNT }, (_, i) => ({
    path: `accident-${i}.qmd`,
    content: `file ${i}\n${'x'.repeat(FILE_BYTES)}\n`,
    contentType: 'text' as const,
  }));
}

let hub: TestHub | undefined;
let client: SyncClient | undefined;

afterEach(async () => {
  await client?.disconnect();
  client = undefined;
  await hub?.stop();
  hub = undefined;
});

describe('disconnect with a drain budget (bd-10deu8h4)', () => {
  it('create-then-disconnect loses nothing: created docs reach the hub', async () => {
    hub = await startTestHub();
    client = createSyncClient(noopCallbacks);

    const created = await client.createNewProject({
      syncServer: hub.url,
      files: accidentFiles(),
      storage: 'memory',
      requireOnline: true,
      peerTimeoutMs: 10000,
    });

    // The accident shape: teardown immediately after creation.
    const report = await client.disconnect({ drainMs: 5000 });

    expect(report.drained).toBe(true);
    expect(report.undelivered).toEqual([]);
    expect(
      await hub.hubHasDoc(created.indexDocId, 2000),
      'index doc must reach the hub',
    ).toBe(true);
    for (const f of created.files) {
      expect(
        await hub.hubHasDoc(f.docId, 2000),
        `file doc for ${f.path} must reach the hub — its only copy was in-process`,
      ).toBe(true);
    }
  }, 30000);

  it('reports undelivered docs when the hub is unreachable at disconnect', async () => {
    hub = await startTestHub();
    client = createSyncClient(noopCallbacks);

    await client.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'hello.qmd', content: 'survives\n', contentType: 'text' }],
      storage: 'memory',
      requireOnline: true,
      peerTimeoutMs: 10000,
      // Keep the reconnect loop from hammering the dead port during
      // the drain window below.
      retryIntervalMs: 60000,
    });

    // Hub goes away mid-session; a file is created during the outage.
    await hub.stop();
    hub = undefined;
    await client.createFile('doomed.qmd', 'created while the hub was down\n');

    const report = await client.disconnect({ drainMs: 400 });

    expect(report.drained, 'drain cannot succeed against a dead hub').toBe(false);
    expect(
      report.undelivered.map((u) => u.path),
      'the outage-created file must be named as possibly lost',
    ).toContain('doomed.qmd');
  }, 30000);
});
