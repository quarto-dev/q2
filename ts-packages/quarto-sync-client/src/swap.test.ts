/**
 * GE-7: the sync client follows the project owner's flip of `files[path]` to a different doc (G26).
 *
 * Two real clients run through the in-process test hub. The "owner" is the hub's own repo: it creates doc B and writes
 * the index entry, which is what the preview does when it flips a pointer. The guest is a real SyncClient with the
 * swap coordinator wired in.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import type { FilePayload, ReplacedEvent, SyncClientCallbacks } from './types.js';
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

const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
  findDocRetry: { attempts: 1, baseDelayMs: 10 },
};

interface Observed {
  added: Array<{ path: string; file: FilePayload }>;
  replaced: ReplacedEvent[];
  unavailable: Array<{ path: string; docId: string }>;
}

function client(): { c: SyncClient; seen: Observed } {
  const seen: Observed = { added: [], replaced: [], unavailable: [] };
  const callbacks: SyncClientCallbacks = {
    onFileAdded: (path, file) => seen.added.push({ path, file }),
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
    onFileUnavailable: (path, docId) => seen.unavailable.push({ path, docId }),
    onFileReplaced: (event) => seen.replaced.push(event),
  };
  const c = createSyncClient(callbacks);
  liveClients.push(c);
  return { c, seen };
}

/** A project with one file, delivered to the hub, and a connected guest. */
async function projectWithGuest(text = 'version A\n') {
  const owner = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
  });
  const created = await owner.createNewProject({
    syncServer: hub.url,
    files: [{ path: 'index.qmd', content: text, contentType: 'text' }],
    ...ONLINE,
  });
  expect(await hub.hubHasHeadsOf(owner, 8000)).toBe(true);
  await owner.disconnect();

  const { c: guest, seen } = client();
  await guest.connect(hub.url, created.indexDocId, undefined, undefined, undefined, ONLINE);
  const oldDocId = guest.getFileHandle('index.qmd')!.documentId as string;
  return { indexDocId: created.indexDocId, guest, seen, oldDocId };
}

/** The owner side: a new doc on the hub's repo, and the flip of the pointer. */
function makeDocOnHub(text: string): string {
  const handle = hub.repo.create<{ text: string }>({ text });
  return handle.documentId as string;
}

async function flip(indexDocId: string, path: string, docId: string): Promise<void> {
  const handle = await hub.repo.find<{ files: Record<string, string> }>(indexDocId as DocumentId);
  handle.change((d) => {
    d.files[path] = docId;
  });
}

function ghostDocId(): string {
  return parseAutomergeUrl(generateAutomergeUrl()).documentId as string;
}

async function until(cond: () => boolean, timeoutMs = 10000, what = 'condition'): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!cond()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 25));
  }
}

const phases = (seen: Observed, path = 'index.qmd') =>
  seen.replaced.filter((e) => e.path === path).map((e) => e.phase);

describe('a flip of files[path] to a different doc', () => {
  it('goes pending then done, re-fires onFileAdded with the new text and docId, and rebinds the handle', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest();
    const docB = makeDocOnHub('version B\n');
    seen.added.length = 0;

    await flip(indexDocId, 'index.qmd', docB);

    await until(() => phases(seen).includes('done'), 10000, 'done');
    expect(phases(seen)).toEqual(['pending', 'done']);
    const done = seen.replaced.find((e) => e.phase === 'done')!;
    expect(done.oldDocId).toBe(oldDocId);
    expect(done.newDocId).toBe(docB);

    const last = seen.added[seen.added.length - 1];
    expect(last.path).toBe('index.qmd');
    expect(last.file).toMatchObject({ type: 'text', text: 'version B\n', docId: docB });
    expect(guest.getFileHandle('index.qmd')!.documentId).toBe(docB);
    expect(guest.getFileContent('index.qmd')).toBe('version B\n');
  }, 20000);

  it('reaches only the final doc when the pointer flips twice quickly', async () => {
    const { indexDocId, seen } = await projectWithGuest();
    const docB = makeDocOnHub('version B\n');
    const docC = makeDocOnHub('version C\n');

    await flip(indexDocId, 'index.qmd', docB);
    await flip(indexDocId, 'index.qmd', docC);

    await until(
      () => seen.replaced.some((e) => e.phase === 'done' && e.newDocId === docC),
      10000,
      'done at C',
    );
    expect(seen.replaced.filter((e) => e.phase === 'done').every((e) => e.newDocId === docC)).toBe(
      true,
    );
  }, 20000);

  it('fires unavailable after 5 s for a doc the hub does not have, keeps the old doc, and marks the path unavailable', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest();
    const ghost = ghostDocId();

    await flip(indexDocId, 'index.qmd', ghost);

    await until(() => phases(seen).includes('unavailable'), 9000, 'unavailable');
    expect(phases(seen)).toEqual(['pending', 'unavailable']);
    expect(guest.getFileHandle('index.qmd')!.documentId).toBe(oldDocId);
    expect(seen.unavailable.some((u) => u.path === 'index.qmd' && u.docId === ghost)).toBe(true);
  }, 20000);

  it('lands a write made with the old baseDocId during pending in doc A, not B', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest('A\n');
    // The target never loads, so the swap stays pending until the 5 s timeout.
    await flip(indexDocId, 'index.qmd', ghostDocId());
    await until(() => phases(seen).includes('pending'), 5000, 'pending');

    const oldHandle = guest.getFileHandle('index.qmd')!;
    guest.updateFileContent('index.qmd', 'edited during pending\n', oldDocId);

    expect(oldHandle.doc()!.text).toBe('edited during pending\n');
  }, 20000);

  it('keeps a write made with the old baseDocId after done in the retired doc A', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest('A\n');
    const oldHandle = guest.getFileHandle('index.qmd')!;
    const docB = makeDocOnHub('B\n');
    await flip(indexDocId, 'index.qmd', docB);
    await until(() => phases(seen).includes('done'), 10000, 'done');

    guest.updateFileContent('index.qmd', 'late edit\n', oldDocId);

    expect(oldHandle.doc()!.text).toBe('late edit\n');
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('B\n');
  }, 20000);

  it('reuses the retired handle when the pointer flips back', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest('A\n');
    const oldHandle = guest.getFileHandle('index.qmd')!;
    const docB = makeDocOnHub('B\n');
    await flip(indexDocId, 'index.qmd', docB);
    await until(() => phases(seen).includes('done'), 10000, 'done at B');

    await flip(indexDocId, 'index.qmd', oldDocId);
    await until(
      () => seen.replaced.some((e) => e.phase === 'done' && e.newDocId === oldDocId),
      10000,
      'done back at A',
    );

    expect(guest.getFileHandle('index.qmd')).toBe(oldHandle);
  }, 20000);
});

describe('write routing by baseDocId', () => {
  it('sends a stale baseDocId through applyEditorOperations to the retired doc', async () => {
    const { indexDocId, guest, seen, oldDocId } = await projectWithGuest('abc\n');
    const oldHandle = guest.getFileHandle('index.qmd')!;
    const docB = makeDocOnHub('xyz\n');
    await flip(indexDocId, 'index.qmd', docB);
    await until(() => phases(seen).includes('done'), 10000, 'done');

    guest.applyEditorOperations(
      'index.qmd',
      [{ rangeOffset: 0, rangeLength: 0, text: '>> ' }],
      oldDocId,
    );

    expect(oldHandle.doc()!.text).toBe('>> abc\n');
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('xyz\n');
  }, 20000);

  it('drops a write with an unknown baseDocId and warns', async () => {
    const { guest } = await projectWithGuest('abc\n');
    const handle = guest.getFileHandle('index.qmd')!;
    const warnings: string[] = [];
    const orig = console.warn;
    console.warn = (...args: unknown[]) => warnings.push(args.map(String).join(' '));
    try {
      guest.updateFileContent('index.qmd', 'nope\n', ghostDocId());
      guest.applyEditorOperations(
        'index.qmd',
        [{ rangeOffset: 0, rangeLength: 0, text: 'nope' }],
        ghostDocId(),
      );
    } finally {
      console.warn = orig;
    }
    expect(handle.doc()!.text).toBe('abc\n');
    expect(warnings.length).toBeGreaterThanOrEqual(2);
  }, 20000);

  it('behaves as before with no baseDocId', async () => {
    const { guest } = await projectWithGuest('abc\n');
    guest.updateFileContent('index.qmd', 'changed\n');
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('changed\n');
    guest.applyEditorOperations('index.qmd', [{ rangeOffset: 0, rangeLength: 0, text: '!' }]);
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('!changed\n');
  }, 20000);
});

describe('in a browser-like environment', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('works when the global timers refuse to run with any `this` but the global (browser "Illegal invocation")', async () => {
    // window.setTimeout throws when called as a method of another object. The coordinator receives its clock as an
    // object, so a host that hands over the bare globals breaks in a browser while passing in Node (found by the
    // end-to-end run of GE-7 against a real preview).
    const realSet = globalThis.setTimeout;
    const realClear = globalThis.clearTimeout;
    const strict = (name: string, impl: (...a: never[]) => unknown) =>
      function (this: unknown, ...args: never[]) {
        if (this !== undefined && this !== globalThis) throw new TypeError(`Illegal invocation (${name})`);
        return impl(...args);
      };
    vi.stubGlobal('setTimeout', strict('setTimeout', ((...a: Parameters<typeof setTimeout>) => realSet(...a)) as never));
    vi.stubGlobal('clearTimeout', strict('clearTimeout', ((...a: Parameters<typeof clearTimeout>) => realClear(...a)) as never));

    const { indexDocId, seen } = await projectWithGuest('A\n');
    await flip(indexDocId, 'index.qmd', makeDocOnHub('B\n'));
    await until(() => phases(seen).includes('done'), 10000, 'done');
    expect(phases(seen)).toEqual(['pending', 'done']);
  }, 30000);
});
