/**
 * Unit-level tests for the file-document duplicate-seq recovery
 * (bd-0h8crv6u; see
 * claude-notes/plans/2026-10-22-file-doc-duplicate-seq-recovery.md):
 * per-document backoff, the attempt-cap circuit breaker that routes the
 * file into the unavailable-files retry poll, and the routing
 * regressions (index document still recovers; untracked documents are
 * still ignored).
 *
 * Unlike the full-stack test (`full-stack-file-collision.test.ts`),
 * collisions here are driven deterministically: the victim's
 * per-document `DocSynchronizer.receiveMessage` is stubbed to throw the
 * exact `RangeError: duplicate seq N found for actor <id>` the real
 * stack throws, and the test invokes the (wrapped)
 * `repo.synchronizer.receiveMessage` — installed by
 * `installDuplicateSeqRecovery` at connect — directly. `getRepo()` is
 * the seam. The hub and websocket are real; only the poison is
 * injected. Backoff timing uses the injected `recoveryBackoff` config
 * (mirroring the `findDocRetry` precedent) with real timers — fake
 * timers would also drive the recovery refetch's own
 * `AbortSignal.timeout`.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { from as automergeFrom, save as automergeSave } from '@automerge/automerge';
import { Repo, type DocumentId } from '@automerge/automerge-repo';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';

import { createSyncClient, type SyncClient } from './client.js';
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
  vi.restoreAllMocks();
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

/** Poll until `cond()` is true or the deadline passes. */
async function until(cond: () => boolean, timeoutMs = 10000, what = 'condition'): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!cond()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((r) => setTimeout(r, 25));
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Fast findDoc retry policy so unavailable-doc probes don't dominate test time. */
const FAST_RETRY = { attempts: 1, baseDelayMs: 10 };

/**
 * Create a project on the hub with one text file and wait until the hub
 * actually holds every document, then disconnect the creator.
 */
async function createProjectOnHub(content: string): Promise<{ indexDocId: string }> {
  const creator = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  const result = await creator.createNewProject({
    syncServer: hub.url,
    files: [{ path: 'main.qmd', content, contentType: 'text' }],
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  expect(await hub.hubHasHeadsOf(creator, 8000), 'every doc must reach the hub in full').toBe(true);
  await creator.disconnect();
  return result;
}

type DocSyncStub = {
  receiveMessage: (message: never) => void;
  hasPeer: (peerId: string) => boolean;
};

/** The repo's live DocSynchronizer for a document, or undefined. */
function docSyncOf(repo: Repo, docId: string): DocSyncStub | undefined {
  return (repo.synchronizer.docSynchronizers as Record<string, DocSyncStub | undefined>)[docId];
}

/**
 * Replace the document's DocSynchronizer.receiveMessage with the exact
 * throw the real automerge stack produces for an actor/seq collision.
 * Must be re-applied after every successful recovery: the refetch
 * attaches a fresh (unpoisoned) DocSynchronizer.
 */
function poison(repo: Repo, docId: string): void {
  const docSync = docSyncOf(repo, docId);
  if (!docSync) throw new Error(`no doc synchronizer for ${docId}`);
  docSync.receiveMessage = () => {
    throw new RangeError('duplicate seq 2 found for actor aaaaaaaaaaaaaaaa');
  };
}

/**
 * Push one colliding inbound message for `docId` through the wrapped
 * `repo.synchronizer.receiveMessage`. The wrapper rethrows after
 * (maybe) triggering recovery; swallow the expected RangeError.
 */
function fireCollision(repo: Repo, docId: string): void {
  const message = {
    type: 'sync',
    documentId: docId as DocumentId,
    senderId: 'test-hub',
    targetId: 'some-peer',
    data: new Uint8Array([0]),
  };
  try {
    repo.synchronizer.receiveMessage(
      message as unknown as Parameters<Repo['synchronizer']['receiveMessage']>[0],
    );
  } catch (err) {
    if (!(err instanceof RangeError && /duplicate seq/.test(err.message))) throw err;
  }
}

/** Count console.warn calls whose first argument includes every fragment. */
function warnCount(warnSpy: ReturnType<typeof vi.spyOn>, ...fragments: string[]): number {
  return warnSpy.mock.calls.filter(
    (args) => typeof args[0] === 'string' && fragments.every((f) => args[0].includes(f)),
  ).length;
}

const fileDocIdOf = (c: SyncClient): string =>
  c.getDocInventory().find((e) => e.path === 'main.qmd')!.docId;

describe('file-document duplicate-seq recovery backoff', () => {
  it('spaces recoveries by the cooldown and suppresses collisions inside it', async () => {
    const { indexDocId } = await createProjectOnHub('present from the start\n');
    const warnSpy = vi.spyOn(console, 'warn');
    const victim = client();
    await victim.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
      recoveryBackoff: { maxAttempts: 10, cooldownMs: 100, quietWindowMs: 5000 },
    });
    const repo = victim.getRepo()!;
    expect(repo).not.toBeNull();
    const fileDocId = fileDocIdOf(victim);
    const findSpy = vi.spyOn(repo, 'find');
    const findCount = () =>
      findSpy.mock.calls.filter((args) => String(args[0]).includes(fileDocId)).length;

    // The recovery's refetch goes through repo.find; a collision that is
    // suppressed by the cooldown must not reach repo.find at all.
    await until(() => docSyncOf(repo, fileDocId)?.hasPeer('test-hub') === true, 10000, 'doc peer');
    poison(repo, fileDocId);
    fireCollision(repo, fileDocId);
    await until(() => findCount() >= 1, 5000, 'first recovery refetch');
    await until(
      () => warnCount(warnSpy, 'recovered file document', fileDocId, 'main.qmd') >= 1,
      5000,
      'first recovery to complete',
    );

    // Re-poison the freshly attached DocSynchronizer, then fire within
    // the 100 ms cooldown: no second recovery, no second refetch.
    await until(
      () => docSyncOf(repo, fileDocId)?.hasPeer('test-hub') === true,
      10000,
      'doc peer after recovery',
    );
    poison(repo, fileDocId);
    fireCollision(repo, fileDocId);
    await sleep(150);
    expect(findCount()).toBe(1);
    expect(warnCount(warnSpy, 'recovered file document', fileDocId)).toBe(1);

    // Once the cooldown has elapsed, the next collision recovers again.
    fireCollision(repo, fileDocId);
    await until(() => findCount() >= 2, 5000, 'second recovery refetch');
    await until(
      () => warnCount(warnSpy, 'recovered file document', fileDocId, 'main.qmd') >= 2,
      5000,
      'second recovery to complete',
    );
    expect(findCount()).toBe(2);
  }, 30000);
});

describe('file-document duplicate-seq circuit breaker', () => {
  it('at the attempt cap, deletes the local copy, marks the file unavailable, and heals via the retry poll', async () => {
    const { indexDocId } = await createProjectOnHub('present from the start\n');
    const warnSpy = vi.spyOn(console, 'warn');
    const added: Array<{ path: string; text?: string }> = [];
    const unavailable: Array<{ path: string; docId: string }> = [];
    const victim = client({
      onFileAdded: (path, file) => {
        added.push({ path, text: file.type === 'text' ? file.text : undefined });
      },
      onFileUnavailable: (path, docId) => {
        unavailable.push({ path, docId });
      },
    });
    await victim.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
      recoveryBackoff: { maxAttempts: 2, cooldownMs: 50, quietWindowMs: 5000 },
    });
    const repo = victim.getRepo()!;
    const fileDocId = fileDocIdOf(victim);
    const findSpy = vi.spyOn(repo, 'find');
    const deleteSpy = vi.spyOn(repo, 'delete');
    const findCount = () =>
      findSpy.mock.calls.filter((args) => String(args[0]).includes(fileDocId)).length;

    // Two capped recoveries: fire, wait for each to complete, re-poison.
    for (let n = 1; n <= 2; n++) {
      await until(
        () => docSyncOf(repo, fileDocId)?.hasPeer('test-hub') === true,
        10000,
        `doc peer before collision ${n}`,
      );
      poison(repo, fileDocId);
      fireCollision(repo, fileDocId);
      await until(
        () => warnCount(warnSpy, 'recovered file document', fileDocId, 'main.qmd') >= n,
        5000,
        `recovery ${n} to complete`,
      );
      await sleep(75); // past the 50 ms cooldown so the next collision is eligible
    }
    const findsAfterRecoveries = findCount();
    expect(findsAfterRecoveries).toBe(2);

    // The third collision hits the cap: the local copy is deleted FIRST
    // (so the retry poll's plain findDoc cannot return the wedged
    // handle), the file is marked unavailable, and no further recovery
    // is scheduled.
    const deleteCount = () =>
      deleteSpy.mock.calls.filter((args) => String(args[0]).includes(fileDocId)).length;
    const deletesBeforeCap = deleteCount(); // one per completed recovery
    await until(
      () => docSyncOf(repo, fileDocId)?.hasPeer('test-hub') === true,
      10000,
      'doc peer before the capped collision',
    );
    poison(repo, fileDocId);
    fireCollision(repo, fileDocId);
    await until(() => unavailable.length > 0, 5000, 'onFileUnavailable at the cap');
    expect(unavailable).toEqual([{ path: 'main.qmd', docId: fileDocId }]);
    expect(victim.getUnavailableFiles()).toEqual([{ path: 'main.qmd', docId: fileDocId }]);
    // Exactly one additional delete at the cap itself.
    expect(deleteCount()).toBe(deletesBeforeCap + 1);
    expect(
      warnCount(warnSpy, 'file document', fileDocId, 'main.qmd', 'recovery cap'),
    ).toBeGreaterThanOrEqual(1);
    expect(findCount()).toBe(findsAfterRecoveries);

    // While marked unavailable the file is out of fileHandles, so
    // further collisions can't route to a recovery (covered by the
    // untracked-document regression below — firing one here would only
    // recreate a real DocSynchronizer that rejects the synthetic
    // message bytes). Nothing moves until the retry poll runs.
    await sleep(300);
    expect(findCount()).toBe(findsAfterRecoveries);
    expect(deleteCount()).toBe(deletesBeforeCap + 1);
    expect(victim.getUnavailableFiles()).toEqual([{ path: 'main.qmd', docId: fileDocId }]);
    // repo.delete() is a no-op for a document the repo no longer holds
    // (repeat collisions while the file is marked are safe).
    expect(() => repo.delete(fileDocId as DocumentId)).not.toThrow();

    // With the divergent peer silent (its poison lived in the deleted
    // DocSynchronizer), the bounded retry poll re-fetches the document
    // and the file flows back through onFileAdded.
    const addsBefore = added.length;
    await until(() => added.length > addsBefore, 10000, 'retry poll to re-add the file');
    expect(added[added.length - 1]).toEqual({ path: 'main.qmd', text: 'present from the start\n' });
    expect(victim.getUnavailableFiles()).toEqual([]);
    expect(victim.getFileContent('main.qmd')).toBe('present from the start\n');
  }, 30000);
});

describe('duplicate-seq routing regressions', () => {
  it('a collision on the index document still routes to recoverIndexDocument', async () => {
    const { indexDocId } = await createProjectOnHub('present from the start\n');
    const warnSpy = vi.spyOn(console, 'warn');
    const victim = client();
    await victim.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
      recoveryBackoff: { maxAttempts: 3, cooldownMs: 50, quietWindowMs: 5000 },
    });
    const repo = victim.getRepo()!;

    await until(() => docSyncOf(repo, indexDocId)?.hasPeer('test-hub') === true, 10000, 'index peer');
    poison(repo, indexDocId);
    fireCollision(repo, indexDocId);
    await until(
      () => warnCount(warnSpy, 'recovered index document', indexDocId) >= 1,
      5000,
      'index recovery',
    );
    // The session still works: the file listing survived the recovery.
    expect(victim.getFilePaths()).toContain('main.qmd');
  }, 30000);

  it('a collision on a document that is neither the index nor a tracked file triggers no recovery', async () => {
    const { indexDocId } = await createProjectOnHub('present from the start\n');
    const warnSpy = vi.spyOn(console, 'warn');
    const victim = client();
    await victim.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
      recoveryBackoff: { maxAttempts: 2, cooldownMs: 50, quietWindowMs: 5000 },
    });
    const repo = victim.getRepo()!;
    const findSpy = vi.spyOn(repo, 'find');
    const deleteSpy = vi.spyOn(repo, 'delete');

    // A document the repo holds but the client does not track (e.g. a
    // capture binary fetched via getBinaryDocById has this shape).
    const stray = repo.import<TextDocumentContent>(
      automergeSave(automergeFrom({ text: 'stray, untracked\n' })),
    );
    const strayDocId = String(stray.documentId);
    expect(strayDocId).not.toBe(indexDocId);

    await until(
      () => docSyncOf(repo, strayDocId)?.hasPeer('test-hub') === true,
      10000,
      'stray doc peer',
    );
    poison(repo, strayDocId);
    fireCollision(repo, strayDocId);
    await sleep(300);

    expect(warnCount(warnSpy, 'duplicate-seq')).toBe(0);
    expect(warnCount(warnSpy, 'recovered')).toBe(0);
    expect(deleteSpy).not.toHaveBeenCalled();
    expect(findSpy.mock.calls.filter((args) => String(args[0]).includes(strayDocId))).toHaveLength(
      0,
    );
    // The project itself is undisturbed.
    expect(victim.getFileContent('main.qmd')).toBe('present from the start\n');
  }, 30000);
});
