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
import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import * as fs from 'node:fs';
import * as net from 'node:net';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

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

// ---------------------------------------------------------------------------
// Real-Rust-hub variant (samod). The drain's delivery signal keys off the
// hub's handshake storageId + remote-heads sync info; the JS test hub above
// was *made* to mimic samod's shape, so this gated test is the proof against
// the production implementation itself (plan § Phase 1 verdict, point 2).
// Gated on target/debug/hub (cargo build -p quarto-hub), unix only — same
// pattern as restart-window-creation.test.ts.
// ---------------------------------------------------------------------------

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const hubBin = path.join(repoRoot, 'target', 'debug', 'hub');
const runRustHub = fs.existsSync(hubBin) && process.platform !== 'win32';
if (!runRustHub) {
  // eslint-disable-next-line no-console
  console.error(`[exit-drain] SKIPPING rust-hub variant: hub binary missing at ${hubBin}`);
}

async function freePort(): Promise<number> {
  const srv = net.createServer();
  srv.listen(0, '127.0.0.1');
  await once(srv, 'listening');
  const port = (srv.address() as net.AddressInfo).port;
  srv.close();
  await once(srv, 'close');
  return port;
}

async function waitForHealth(port: number, timeoutMs = 15000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastErr: unknown;
  while (Date.now() < deadline) {
    try {
      const resp = await fetch(`http://127.0.0.1:${port}/health`);
      if (resp.status < 500) return;
      lastErr = resp.status;
    } catch (err) {
      lastErr = err;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`hub not healthy on :${port}: ${lastErr}`);
}

function docInStorage(dataDir: string, docId: string): boolean {
  return fs.existsSync(path.join(dataDir, 'automerge', docId.slice(0, 2), docId.slice(2)));
}

describe.runIf(runRustHub)('drain against the real samod hub (bd-10deu8h4)', () => {
  let rustHub: ChildProcess | undefined;
  let tmpDir: string | undefined;

  afterEach(async () => {
    rustHub?.kill('SIGKILL');
    if (rustHub) await once(rustHub, 'exit').catch(() => {});
    rustHub = undefined;
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
  });

  it('create-then-disconnect delivers every doc into samod storage', async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'exit-drain-rust-'));
    const port = await freePort();
    rustHub = spawn(hubBin, ['--data-dir', tmpDir, '-P', String(port), '-H', '127.0.0.1'], {
      stdio: ['ignore', 'ignore', process.env['DEBUG_MCP'] ? 'inherit' : 'ignore'],
    });
    await waitForHealth(port);

    client = createSyncClient(noopCallbacks);
    const created = await client.createNewProject({
      syncServer: `ws://127.0.0.1:${port}/ws`,
      files: accidentFiles(),
      storage: 'memory',
      requireOnline: true,
      peerTimeoutMs: 10000,
    });

    const report = await client.disconnect({ drainMs: 5000 });
    expect(report.drained, 'samod must confirm delivery within the budget').toBe(true);

    // Hub-side ground truth: the document files exist in samod's
    // on-disk storage. No settle wait — delivery was confirmed
    // *before* disconnect returned, modulo samod's own disk flush.
    const deadline = Date.now() + 10000;
    const allDocIds = [created.indexDocId, ...created.files.map((f) => f.docId)];
    for (const docId of allDocIds) {
      while (!docInStorage(tmpDir, docId) && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 100));
      }
      expect(
        docInStorage(tmpDir, docId),
        `doc ${docId} must be in samod storage after a drained disconnect`,
      ).toBe(true);
    }
  }, 60000);
});
