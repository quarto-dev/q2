/**
 * D1 localization, restart-window variant (bd-10bdjmjb Phase 1).
 *
 * Both fresh-creation repros pass (JS hub and Rust hub:
 * offline-creation*.test.ts) — first-time connects deliver
 * created-while-offline docs fine. Production's hello-claude.qmd was
 * different in one specific way: the creating client had ALREADY been
 * connected to the hub and was in the gap of a hub restart when the
 * file was created. On reconnect the remote peer id is stable, so any
 * cached per-peer sync state can convince the synchronizer the peer
 * already knows everything — the prime remaining suspect.
 *
 * Repro: connect online → SIGKILL the hub mid-session → createFile
 * during the gap → restart the hub (same port, same data dir) →
 * assert the new document lands in hub storage.
 *
 * Gated on target/debug/hub (cargo build -p quarto-hub), unix only.
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
  console.error(`[restart-window-creation] SKIPPING: hub binary missing at ${hubBin}`);
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

function spawnHub(dataDir: string, port: number): ChildProcess {
  return spawn(hubBin, ['--data-dir', dataDir, '-P', String(port), '-H', '127.0.0.1'], {
    stdio: ['ignore', 'ignore', process.env['DEBUG_MCP'] ? 'inherit' : 'ignore'],
  });
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
  hub?.kill('SIGKILL');
  hub = undefined;
  if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
  tmpDir = undefined;
});

describe.runIf(runIt)('file created during a hub-restart gap (the hello-claude shape)', () => {
  it('reaches hub storage after the hub comes back and the client reconnects', async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'restart-window-'));
    const port = await freePort();

    // 1. Hub up; client connected ONLINE; project synced.
    hub = spawnHub(tmpDir, port);
    await waitForHealth(port);

    let online = false;
    const c = createSyncClient({
      onFileAdded: () => {},
      onFileChanged: () => {},
      onFileRemoved: () => {},
      onConnectionChange: (isOnline: boolean) => {
        online = isOnline;
      },
    });
    const created = await c.createNewProject({
      syncServer: `ws://127.0.0.1:${port}/ws`,
      files: [{ path: 'pre-existing.qmd', content: 'here before the restart\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      retryIntervalMs: 500,
    });
    expect(await waitForDocInStorage(tmpDir, created.indexDocId, 10000)).toBe(true);

    // 2. Hub dies mid-session (the restart window).
    hub.kill('SIGKILL');
    await once(hub, 'exit');
    hub = undefined;
    // Wait until the client notices it is offline.
    const offlineDeadline = Date.now() + 10000;
    while (online && Date.now() < offlineDeadline) {
      await new Promise((r) => setTimeout(r, 100));
    }
    expect(online, 'client should observe the disconnect').toBe(false);

    // 3. The user creates a file during the gap.
    await c.createFile('during-gap.qmd', 'created while the hub was down\n');
    const gapDocId = (await (async () => {
      // The index maps path -> docId; read it via the client's file list.
      const files = c.getFiles?.() as Array<{ path: string; docId: string }> | undefined;
      return files?.find((f) => f.path === 'during-gap.qmd')?.docId;
    })());

    // 4. Hub comes back on the same port with the same data dir.
    hub = spawnHub(tmpDir, port);
    await waitForHealth(port);

    // 5. Wait for the CREATOR to actually reconnect (its retry loop is
    // 500 ms) — only then is "the hub never receives the doc" a real
    // defect rather than a race in this test.
    const onlineDeadline = Date.now() + 20000;
    while (!online && Date.now() < onlineDeadline) {
      await new Promise((r) => setTimeout(r, 100));
    }
    expect(online, 'creator must reconnect after the hub returns').toBe(true);
    // Generous settle window for announce/sync to do its thing.
    await new Promise((r) => setTimeout(r, 3000));

    if (gapDocId) {
      expect(
        await waitForDocInStorage(tmpDir, gapDocId, 20000),
        `gap-created doc ${gapDocId} must reach hub storage`,
      ).toBe(true);
    } else {
      // Fallback assertion when the client API exposes no doc id:
      // a fresh second client must be able to read the file.
      const reader = createSyncClient({
        onFileAdded: () => {},
        onFileChanged: () => {},
        onFileRemoved: () => {},
      });
      // Poll with fresh readers for up to 15 s — eliminates any
      // remaining propagation-timing doubt before declaring red.
      let seen: string[] = [];
      let content: string | null | undefined;
      const deadline = Date.now() + 15000;
      while (Date.now() < deadline) {
        const reader = createSyncClient({
          onFileAdded: () => {},
          onFileChanged: () => {},
          onFileRemoved: () => {},
        });
        const files = await reader.connect(
          `ws://127.0.0.1:${port}/ws`,
          created.indexDocId,
          undefined,
          undefined,
          undefined,
          { storage: 'memory', peerTimeoutMs: 10000, requireOnline: true },
        );
        seen = files.map((f) => f.path);
        content = reader.getFileContent?.('during-gap.qmd');
        await reader.disconnect();
        if (seen.includes('during-gap.qmd') && content) break;
        await new Promise((r) => setTimeout(r, 1000));
      }
      expect(seen, 'second client must see the gap-created file').toContain('during-gap.qmd');
      expect(content, 'gap-created file must be readable, not a dangling entry').toBeTruthy();
    }

    await c.disconnect();
  }, 90000);
});
