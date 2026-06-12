/**
 * D1 localization, Rust-hub variant (bd-10bdjmjb Phase 1).
 *
 * The JS-hub run (offline-creation.test.ts) PASSES — the client
 * announces created-while-offline documents fine to a JS
 * automerge-repo server. This test plays the identical scenario
 * against the real samod-based `hub` binary: client starts while the
 * port is dead (= the restart window), creates a project + file in
 * offline-fallback mode, then the hub comes up and the adapter's
 * retry loop reconnects. If documents don't land in the hub's
 * storage, the gap is hub-side (samod accept path) — hypothesis (b).
 *
 * Gated: skips unless target/debug/hub exists (cargo build -p
 * quarto-hub) on a unix host. Ground truth is the hub's storage
 * directory (the same check used on production during the incident:
 * /mnt/hub-data/automerge/<2-char shard>/<rest>).
 */

import { describe, it, expect, afterEach } from 'vitest';
import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import * as fs from 'node:fs';
import * as net from 'node:net';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import { createSyncClient } from './client.js';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const hubBin = path.join(repoRoot, 'target', 'debug', 'hub');
const runIt = fs.existsSync(hubBin) && process.platform !== 'win32';
if (!runIt) {
  // eslint-disable-next-line no-console
  console.error(`[offline-creation-rust-hub] SKIPPING: hub binary missing at ${hubBin}`);
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

function docInStorage(dataDir: string, docId: string): boolean {
  return fs.existsSync(path.join(dataDir, 'automerge', docId.slice(0, 2), docId.slice(2)));
}

async function waitForDocInStorage(dataDir: string, docId: string, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (docInStorage(dataDir, docId)) return true;
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

let hub: ChildProcess | undefined;
let tmpDir: string | undefined;

afterEach(() => {
  hub?.kill();
  hub = undefined;
  if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
  tmpDir = undefined;
});

describe.runIf(runIt)('created-while-offline documents vs the real hub', () => {
  it('reach the hub storage after the hub comes up and the client reconnects', async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'offline-creation-hub-'));
    const port = await freePort();

    // 1. Client starts while the hub port is dead — the restart window.
    const c = createSyncClient({
      onFileAdded: () => {},
      onFileChanged: () => {},
      onFileRemoved: () => {},
    });
    const created = await c.createNewProject({
      syncServer: `ws://127.0.0.1:${port}/ws`,
      files: [{ path: 'made-offline.qmd', content: 'created in the window\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 50,
      retryIntervalMs: 500, // fast reconnect loop for the test
    });
    const fileDocId = created.files[0]!.docId;

    // 2. The hub comes up on that port.
    hub = spawn(hubBin, ['--data-dir', tmpDir, '-P', String(port), '-H', '127.0.0.1'], {
      stdio: ['ignore', 'ignore', process.env['DEBUG_MCP'] ? 'inherit' : 'ignore'],
    });

    // 3. The retry loop reconnects; documents must land in storage.
    const indexArrived = await waitForDocInStorage(tmpDir, created.indexDocId, 15000);
    const fileArrived = await waitForDocInStorage(tmpDir, fileDocId, 15000);

    await c.disconnect();

    expect(indexArrived, `index doc ${created.indexDocId} must reach hub storage`).toBe(true);
    expect(fileArrived, `file doc ${fileDocId} must reach hub storage`).toBe(true);
  }, 60000);
});
