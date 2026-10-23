/**
 * Live delivery of a remote file creation (the hub-client "files sidebar
 * doesn't show a collaborator's new document until reload" report).
 *
 * The sidebar is fed by onFilesChange: connect() attaches a 'change'
 * subscription to the shared index document (attachIndexSubscription →
 * reconcileIndexDoc), and a collaborator's createFile writes
 * `doc.files[path] = docId` into that same index. The reader's sidebar
 * should therefore update live, with no reload.
 *
 * No test covered that path: onFilesChange only ever appeared as a
 * vi.fn() stand-in. This one drives the real stack — two SyncClients on
 * one in-process hub — creates a file on the writer while the reader is
 * connected, and asserts the reader hears about it.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import * as fs from 'node:fs';
import * as net from 'node:net';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const liveClients: SyncClient[] = [];

afterEach(async () => {
  for (const c of liveClients.splice(0)) {
    await c.disconnect();
  }
});

const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
  findDocRetry: { attempts: 1, baseDelayMs: 10 },
};

function client(): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

describe('remote file creation reaches a connected client', () => {
  let hub: TestHub;

  beforeEach(async () => {
    hub = await startTestHub();
  });

  afterEach(async () => {
    await hub.stop();
  });

  it('fires onFilesChange with the new path, and the content becomes readable', async () => {
    const writer = client();
    const result = await writer.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
      ...ONLINE,
    });
    expect(await hub.hubHasHeadsOf(writer, 8000)).toBe(true);

    const onFilesChange = vi.fn();
    const reader = createSyncClient({
      onFileAdded: () => {},
      onFileChanged: () => {},
      onFileRemoved: () => {},
      onFilesChange,
    });
    liveClients.push(reader);
    const files = await reader.connect(
      hub.url,
      result.indexDocId,
      undefined,
      undefined,
      undefined,
      ONLINE,
    );
    expect(files.map((f) => f.path)).toEqual(['main.qmd']);
    onFilesChange.mockClear();

    // The collaborator creates the document mid-session.
    await writer.createFile('new-doc.qmd', 'fresh from the writer\n');

    // The reader's file list updates without any reconnect.
    await vi.waitFor(
      () => {
        const paths = onFilesChange.mock.calls.flatMap(([list]) =>
          (list as { path: string }[]).map((f) => f.path),
        );
        expect(paths).toContain('new-doc.qmd');
      },
      { timeout: 10000, interval: 50 },
    );

    // And the new document is subscribed, so its content is readable
    // (the path the editor's "open file" flow takes).
    await vi.waitFor(
      () => {
        expect(reader.getFileContent('new-doc.qmd')).toBe('fresh from the writer\n');
      },
      { timeout: 10000, interval: 50 },
    );
  }, 30000);
});

// ---------------------------------------------------------------------
// Same scenario against the real samod-based `hub` binary — the relay
// production uses. The JS hub above passes; if the Rust hub drops
// peer-to-peer relay of the index change, this is where it shows.
// ---------------------------------------------------------------------

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const hubBin = path.join(repoRoot, 'target', 'debug', 'hub');
const runRustHub = fs.existsSync(hubBin) && process.platform !== 'win32';

async function freePort(): Promise<number> {
  const srv = net.createServer();
  srv.listen(0, '127.0.0.1');
  await once(srv, 'listening');
  const port = (srv.address() as net.AddressInfo).port;
  srv.close();
  await once(srv, 'close');
  return port;
}

describe.runIf(runRustHub)('remote file creation via the real (Rust) hub', () => {
  let rustHub: ChildProcess | undefined;
  let dataDir: string | undefined;

  afterEach(() => {
    rustHub?.kill();
    rustHub = undefined;
    if (dataDir) fs.rmSync(dataDir, { recursive: true, force: true });
    dataDir = undefined;
  });

  it('fires onFilesChange on a connected peer when a collaborator creates a file', async () => {
    dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'remote-file-creation-hub-'));
    const port = await freePort();
    rustHub = spawn(hubBin, ['--data-dir', dataDir, '-P', String(port), '-H', '127.0.0.1'], {
      stdio: ['ignore', 'ignore', process.env['DEBUG_MCP'] ? 'inherit' : 'ignore'],
    });
    const url = `ws://127.0.0.1:${port}/ws`;

    const writer = client();
    const result = await writer.createNewProject({
      syncServer: url,
      files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
      ...ONLINE,
    });

    const onFilesChange = vi.fn();
    const reader = createSyncClient({
      onFileAdded: () => {},
      onFileChanged: () => {},
      onFileRemoved: () => {},
      onFilesChange,
    });
    liveClients.push(reader);
    const files = await reader.connect(
      url,
      result.indexDocId,
      undefined,
      undefined,
      undefined,
      ONLINE,
    );
    expect(files.map((f) => f.path)).toEqual(['main.qmd']);
    onFilesChange.mockClear();

    await writer.createFile('new-doc.qmd', 'fresh from the writer\n');

    await vi.waitFor(
      () => {
        const paths = onFilesChange.mock.calls.flatMap(([list]) =>
          (list as { path: string }[]).map((f) => f.path),
        );
        expect(paths).toContain('new-doc.qmd');
      },
      { timeout: 10000, interval: 50 },
    );

    await vi.waitFor(
      () => {
        expect(reader.getFileContent('new-doc.qmd')).toBe('fresh from the writer\n');
      },
      { timeout: 10000, interval: 50 },
    );
  }, 60000);
});
