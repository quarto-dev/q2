/**
 * Graceful degradation for dangling index entries (bd-vm5e5u10).
 *
 * A project's index maps paths → automerge doc ids. When one referenced
 * document does not exist on the hub (a "dangling entry" — production
 * evidence: /cscheid/q2-mcp-hello.qmd in the 2026-06-12 incident,
 * bd-p68lx71t), the project must stay usable:
 *
 *  - connect() succeeds if the INDEX loads; unavailable files are
 *    skipped, surfaced (status marker + onFileUnavailable), and do not
 *    affect other files;
 *  - a dangling entry appearing mid-session (the index-change path)
 *    must not throw / reject unhandled;
 *  - the index document remaining unavailable stays fatal, with an
 *    error message that says "index" (file-vs-index confusion misled
 *    the 2026-06-12 incident response).
 *
 * The production fixture no longer exists (the entry was surgically
 * removed), so tests mint their own ghost entries by mutating the
 * index through the test hub's own repo handle.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import {
  createSyncClient,
  fileUnavailableMessage,
  indexUnavailableMessage,
  type SyncClient,
} from './client.js';
import type { SyncClientCallbacks } from './types.js';
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

function client(callbacks: Partial<SyncClientCallbacks> = {}): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
    ...callbacks,
  });
  liveClients.push(c);
  return c;
}

/** A valid-format document id that no repo anywhere holds. */
function unknownDocId(): string {
  return parseAutomergeUrl(generateAutomergeUrl()).documentId;
}

/**
 * Mint a dangling entry: mutate the project index through the hub's
 * own repo handle, pointing `path` at a document that does not exist.
 * Returns the ghost doc id.
 */
async function mintGhostEntry(indexDocId: string, path: string): Promise<string> {
  const ghostId = unknownDocId();
  const handle = await hub.repo.find<{ files: Record<string, string> }>(
    indexDocId as DocumentId,
  );
  handle.change((d) => {
    d.files[path] = ghostId;
  });
  return ghostId;
}

/**
 * Create a project on the hub with the given text files and wait until
 * the hub actually holds every document (creation syncs in the
 * background), then disconnect the creator.
 */
async function createProjectOnHub(
  files: Array<{ path: string; content: string }>,
): Promise<string> {
  const creator = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  const result = await creator.createNewProject({
    syncServer: hub.url,
    files: files.map((f) => ({ ...f, contentType: 'text' as const })),
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  expect(await hub.hubHasDoc(result.indexDocId, 8000), 'index doc must reach the hub').toBe(true);
  for (const f of result.files) {
    expect(await hub.hubHasDoc(f.docId, 8000), `file doc for ${f.path} must reach the hub`).toBe(true);
  }
  await creator.disconnect();
  return result.indexDocId;
}

/** Poll until `cond()` is true or the deadline passes. */
async function until(cond: () => boolean, timeoutMs = 10000, what = 'condition'): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!cond()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** Fast findDoc retry policy so unavailable-doc probes don't dominate test time. */
const FAST_RETRY = { attempts: 1, baseDelayMs: 10 };

describe('connect with a dangling index entry', () => {
  it('succeeds, loads the real files, and surfaces the ghost as unavailable', async () => {
    const indexDocId = await createProjectOnHub([
      { path: 'one.qmd', content: 'first real file\n' },
      { path: 'two.qmd', content: 'second real file\n' },
    ]);
    const ghostId = await mintGhostEntry(indexDocId, 'ghost.qmd');

    const added: string[] = [];
    const unavailable: Array<{ path: string; docId: string }> = [];
    const reader = client({
      onFileAdded: (path) => {
        added.push(path);
      },
      onFileUnavailable: (path, docId) => {
        unavailable.push({ path, docId });
      },
    });

    // RED today: connect() rejects with `Document <ghost> is unavailable`.
    const entries = await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });

    // All three index entries are reported, the ghost marked unavailable.
    expect(entries.map((e) => e.path).sort()).toEqual(['ghost.qmd', 'one.qmd', 'two.qmd']);
    const ghostEntry = entries.find((e) => e.path === 'ghost.qmd')!;
    expect(ghostEntry.status).toBe('unavailable');
    expect(ghostEntry.docId).toBe(ghostId);
    for (const real of ['one.qmd', 'two.qmd']) {
      expect(entries.find((e) => e.path === real)!.status).not.toBe('unavailable');
    }

    // Real files loaded; the ghost was surfaced, not silently dropped.
    expect(added.sort()).toEqual(['one.qmd', 'two.qmd']);
    expect(unavailable).toEqual([{ path: 'ghost.qmd', docId: ghostId }]);
    expect(reader.getUnavailableFiles()).toEqual([{ path: 'ghost.qmd', docId: ghostId }]);

    // The project is usable: real content reads fine.
    expect(reader.getFileContent('one.qmd')).toBe('first real file\n');
    expect(reader.getFileContent('two.qmd')).toBe('second real file\n');
    // The ghost has no content (it is listed, not readable).
    expect(reader.getFileContent('ghost.qmd')).toBeNull();
  }, 30000);
});

describe('dangling entry appearing mid-session', () => {
  it('does not blow up an already-open session; existing files keep working', async () => {
    const indexDocId = await createProjectOnHub([
      { path: 'steady.qmd', content: 'present from the start\n' },
    ]);

    const unavailable: Array<{ path: string; docId: string }> = [];
    const reader = client({
      onFileUnavailable: (path, docId) => {
        unavailable.push({ path, docId });
      },
    });
    await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });
    expect(reader.getFileContent('steady.qmd')).toBe('present from the start\n');

    // The dangling entry appears mid-session (how already-open
    // colleagues got hit on 2026-06-12). RED today: the index-change
    // handler's fire-and-forget syncWithFiles rejects unhandled
    // (vitest fails the run) and onFileUnavailable never fires.
    const ghostId = await mintGhostEntry(indexDocId, 'ghost.qmd');

    await until(() => unavailable.length > 0, 10000, 'onFileUnavailable for the ghost');
    expect(unavailable).toEqual([{ path: 'ghost.qmd', docId: ghostId }]);

    // The session stays usable: read and write the existing file.
    expect(reader.getFileContent('steady.qmd')).toBe('present from the start\n');
    reader.updateFileContent('steady.qmd', 'edited after the ghost appeared\n');
    await until(
      () => reader.getFileContent('steady.qmd') === 'edited after the ghost appeared\n',
      10000,
      'edit to apply',
    );
  }, 30000);
});

describe('unavailable index document', () => {
  it('stays fatal, and the error says index (not file) and names the id', async () => {
    const bogusIndexId = unknownDocId();
    const reader = client();

    await expect(
      reader.connect(hub.url, bogusIndexId, undefined, undefined, undefined, {
        storage: 'memory',
        peerTimeoutMs: 10000,
        requireOnline: true,
        findDocRetry: FAST_RETRY,
      }),
    ).rejects.toThrow(/index/i);

    await expect(
      reader.connect(hub.url, bogusIndexId, undefined, undefined, undefined, {
        storage: 'memory',
        peerTimeoutMs: 10000,
        requireOnline: true,
        findDocRetry: FAST_RETRY,
      }),
    ).rejects.toThrow(new RegExp(bogusIndexId));
  }, 30000);
});

describe('error message clarity (locked wording, bd-vm5e5u10 requirement 5)', () => {
  it('the file-unavailable message names the path, the doc id, and the sync server', () => {
    const msg = fileUnavailableMessage('cscheid/q2-mcp-hello.qmd', '3HJoqsMxPYRDFumPVKDznYALmoVf');
    expect(msg).toContain("file document for 'cscheid/q2-mcp-hello.qmd'");
    expect(msg).toContain('automerge:3HJoqsMxPYRDFumPVKDznYALmoVf');
    expect(msg).toContain('unavailable on the sync server');
    // The likely cause, so incident responders aren't misled.
    expect(msg).toContain('never synced');
  });

  it('the index-unavailable message says index and names the id', () => {
    const msg = indexUnavailableMessage('3HJoqsMxPYRDFumPVKDznYALmoVf');
    expect(msg).toContain('project index document');
    expect(msg).toContain('automerge:3HJoqsMxPYRDFumPVKDznYALmoVf');
    expect(msg).toContain('unavailable on the sync server');
  });
});
