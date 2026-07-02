/**
 * Recovery of a stranded file doc while online — the dominant cause of the
 * nightly smoke-all E2E flakiness (2026-06/07 window).
 *
 * Production shape (from CI smoke-diag evidence): a project's sibling files
 * all sync, but ONE doc's initial request is lost under contention. The
 * DocHandle ends in automerge-repo's `unavailable` state, and from then on
 * `repo.find()` returns the cached verdict WITHOUT contacting the network
 * (Repo.findWithProgress short-circuits on `handle.state === UNAVAILABLE`).
 * The bounded retry poll added in a8c36d741 calls `repo.find()` per tick, so
 * it can never recover the doc: the render fails permanently with
 * "Path not found: /project/<target>.qmd" and the test burns its full
 * 75s budget on every attempt.
 *
 * These tests pin the recovery contract at the client level:
 *
 *  - a file marked unavailable at connect() MUST be delivered once the hub
 *    can serve it, while a peer stays connected (the retry poll must issue a
 *    FRESH network request, not replay the cached unavailable verdict);
 *  - the hub here does NOT announce (announce: false), modeling the samod
 *    hub's request-response behavior — with generous announce the doc would
 *    arrive spontaneously and mask the missing re-request.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as Automerge from '@automerge/automerge';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import type { SyncClientCallbacks } from './types.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  // announce: false — only an explicit client request can fetch a doc,
  // like the production samod hub. See file doc comment.
  hub = await startTestHub({ announce: false });
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

/** A valid-format document id that no repo anywhere holds (yet). */
function unknownDocId(): string {
  return parseAutomergeUrl(generateAutomergeUrl()).documentId;
}

/**
 * Mint an index entry pointing at a doc the hub cannot serve yet.
 * Returns the doc id, which {@link materializeDocOnHub} can later fill in.
 */
async function mintStrandedEntry(indexDocId: string, path: string): Promise<string> {
  const strandedId = unknownDocId();
  const handle = await hub.repo.find<{ files: Record<string, string> }>(
    indexDocId as DocumentId,
  );
  handle.change((d) => {
    d.files[path] = strandedId;
  });
  return strandedId;
}

/**
 * Make the hub able to serve `docId` as a text document — the moment the
 * stranded doc "arrives" server-side. Import (not create) so the id is
 * exactly the one the index already references.
 */
function materializeDocOnHub(docId: string, text: string): void {
  const doc = Automerge.from({ text });
  hub.repo.import(Automerge.save(doc), { docId: docId as DocumentId });
}

/**
 * Create a project on the hub with the given text files and wait until the
 * hub actually holds every document, then disconnect the creator.
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
async function until(cond: () => boolean, timeoutMs: number, what: string): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!cond()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** Fast findDoc retry policy so the unavailable verdict lands quickly. */
const FAST_RETRY = { attempts: 1, baseDelayMs: 10 };

describe('stranded file doc recovery while online', () => {
  it('delivers a file marked unavailable at connect once the hub can serve it', async () => {
    const indexDocId = await createProjectOnHub([
      { path: 'main.qmd', content: 'sibling that syncs fine\n' },
    ]);
    const strandedId = await mintStrandedEntry(indexDocId, 'target.qmd');

    const added: string[] = [];
    const unavailable: string[] = [];
    const reader = client({
      onFileAdded: (path) => {
        added.push(path);
      },
      onFileUnavailable: (path) => {
        unavailable.push(path);
      },
    });

    await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });

    // The sibling loads, the target is stranded — the exact CI shape
    // (vfs holds everything but the render target).
    expect(added).toEqual(['main.qmd']);
    expect(unavailable).toEqual(['target.qmd']);
    expect(reader.getFileContent('target.qmd')).toBeNull();

    // The doc becomes servable under the SAME id while the peer stays
    // connected (in production: the request was lost, not the doc).
    materializeDocOnHub(strandedId, 'late but here\n');

    // The bounded retry poll (2s tick) must fetch it with a FRESH network
    // request; the cached unavailable verdict must not be replayed forever.
    await until(() => added.includes('target.qmd'), 15000, 'stranded target.qmd recovery');
    expect(reader.getFileContent('target.qmd')).toBe('late but here\n');
  }, 30000);

  it('recovers a doc whose initial request was lost in transit (production CI shape)', async () => {
    // Both docs exist on the hub the whole time — production evidence shows
    // the render target is created and server-acknowledged before the client
    // ever connects. What fails is the REQUEST: it never reaches the hub.
    const indexDocId = await createProjectOnHub([
      { path: 'main.qmd', content: 'sibling that syncs fine\n' },
      { path: 'target.qmd', content: 'the render target\n' },
    ]);
    const targetEntry = await (async () => {
      const handle = await hub.repo.find<{ files: Record<string, string> }>(
        indexDocId as DocumentId,
      );
      return handle.doc()!.files['target.qmd'];
    })();

    // Every client frame concerning the target doc is swallowed before the
    // hub's repo sees it: the doc handle on the client strands in
    // `requesting`, exactly as when a frame is lost under CI contention.
    hub.dropMessagesFor(targetEntry);

    const added: string[] = [];
    const unavailable: string[] = [];
    const reader = client({
      onFileAdded: (path) => {
        added.push(path);
      },
      onFileUnavailable: (path) => {
        unavailable.push(path);
      },
    });

    // connect() rides out the findDoc attempts for the dropped doc
    // (attempts+1 × 5s per-attempt cap) and degrades it to unavailable.
    await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
      findDocRetry: FAST_RETRY,
    });

    expect(added).toEqual(['main.qmd']);
    expect(unavailable).toEqual(['target.qmd']);

    // Transient contention clears: frames flow again. Nothing arrives unless
    // the client actually RE-REQUESTS the doc — the hub will not announce
    // (announce: false) and holds no request on file (it was dropped).
    hub.stopDroppingMessages();

    // RED before the eviction fix: the retry poll's repo.find() replays the
    // poisoned cached handle (stuck `requesting`, later `unavailable`)
    // without ever re-contacting the network, so the doc never arrives.
    await until(() => added.includes('target.qmd'), 20000, 'lost-request target.qmd recovery');
    expect(reader.getFileContent('target.qmd')).toBe('the render target\n');
  }, 60000);
});
