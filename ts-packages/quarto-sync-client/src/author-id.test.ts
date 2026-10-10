/**
 * Phase 2 tests for the author-ID transition (strand bd-x3b1e0t9; plan:
 * claude-notes/plans/2026-09-30-automerge-author-id-transition.md).
 *
 * The sync client no longer accepts or applies actor IDs: every document
 * instance gets automerge's random actor (D1), and the stable per-user
 * identity travels as automerge change-level author metadata (D9 applies
 * it in place on the handle's backend). Per the Phase 0 spike, the author
 * footer lands on each actor's seq-1 change only; later changes resolve
 * through the actor→author index, so the resolution rule asserted
 * throughout is:
 *
 *   change.author ?? getAuthorForActor(doc, change.actor) ?? change.actor
 *
 * The last test is the convergence proof that replaces
 * full-stack-actor-collision.test.ts: the literal bd-6f21d4c6 scenario
 * (one user, two tabs, simultaneous index-doc edits) can no longer wedge,
 * because the two tabs never share an actor.
 */

import { describe, it, expect, vi } from 'vitest';
import {
  decodeChange,
  getActorId as automergeGetActorId,
  getAllChanges,
  getAuthorForActor,
  type Doc,
} from '@automerge/automerge';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const AUTHOR = 'a'.repeat(64);
const AUTHOR_B = 'b'.repeat(64);

function client(liveClients: SyncClient[]): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

async function pollUntil(
  check: () => boolean | Promise<boolean>,
  timeoutMs = 5000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await check()) return true;
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
}

function filesOf(c: SyncClient): Record<string, string> | undefined {
  return (c.getIndexHandle()?.doc() as IndexDocument | undefined)?.files;
}

/** The attribution key a reader must compute for a decoded change. */
function attributionKey(
  doc: Doc<unknown>,
  change: { actor: string; author?: string | null },
): string | undefined {
  return (
    change.author ??
    getAuthorForActor(doc, change.actor as never) ??
    change.actor
  );
}

/** Every change by the client's current actor on `doc`, decoded. */
function localChangesOf(doc: Doc<unknown>) {
  const me = automergeGetActorId(doc);
  return getAllChanges(doc)
    .map((c) => decodeChange(c))
    .filter((c) => c.actor === me);
}

const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
  findDocRetry: { attempts: 1, baseDelayMs: 10 },
};

describe('author IDs through the real SyncClient', () => {
  it('connect with an authorId produces changes that all resolve to that author', async () => {
    const hub: TestHub = await startTestHub();
    const liveClients: SyncClient[] = [];
    try {
      const creator = client(liveClients);
      const result = await creator.createNewProject(
        {
          syncServer: hub.url,
          files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        },
        AUTHOR,
        'Creator',
      );
      expect(await hub.hubHasHeadsOf(creator, 8000)).toBe(true);

      // A second client connects with its own author (findDoc path
      // applies it to the index handle).
      const editor = client(liveClients);
      await editor.connect(hub.url, result.indexDocId, AUTHOR_B, 'Editor', undefined, ONLINE);
      await editor.createFile('editor-file.qmd', 'from editor\n');

      const doc = editor.getIndexHandle()!.doc()!;
      const mine = localChangesOf(doc as Doc<unknown>);
      expect(mine.length).toBeGreaterThan(0);
      for (const c of mine) {
        expect(attributionKey(doc as Doc<unknown>, c)).toBe(AUTHOR_B);
      }
      // The editor's first change on this doc instance carries the footer
      // (seq == 1 of its fresh actor).
      expect(mine.some((c) => c.author === AUTHOR_B)).toBe(true);

      // The created file document is likewise authored from its first
      // change (createDoc's automergeFrom(init, { author })).
      const fileDoc = editor.getFileHandle('editor-file.qmd')!.doc()!;
      const fileChanges = getAllChanges(fileDoc as Doc<unknown>).map((c) => decodeChange(c));
      expect(fileChanges.length).toBeGreaterThan(0);
      for (const c of fileChanges) {
        expect(attributionKey(fileDoc as Doc<unknown>, c)).toBe(AUTHOR_B);
      }
      expect(fileChanges[0].author).toBe(AUTHOR_B);
    } finally {
      for (const c of liveClients.splice(0)) await c.disconnect();
      await hub.stop();
    }
  }, 30000);

  it('createNewProject stamps the author from the index doc’s very first change', async () => {
    const hub: TestHub = await startTestHub();
    const liveClients: SyncClient[] = [];
    try {
      const creator = client(liveClients);
      const result = await creator.createNewProject(
        {
          syncServer: hub.url,
          files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        },
        AUTHOR,
        'Creator',
      );

      const indexDoc = creator.getIndexHandle()!.doc()!;
      const all = getAllChanges(indexDoc as Doc<unknown>).map((c) => decodeChange(c));
      expect(all.length).toBeGreaterThan(0);
      expect(all[0].author).toBe(AUTHOR);
      for (const c of all) {
        expect(attributionKey(indexDoc as Doc<unknown>, c)).toBe(AUTHOR);
      }

      // The initial file document is authored from its first change too.
      const fileDoc = creator.getFileHandle('main.qmd')!.doc()!;
      const fileChanges = getAllChanges(fileDoc as Doc<unknown>).map((c) => decodeChange(c));
      expect(fileChanges[0].author).toBe(AUTHOR);
    } finally {
      for (const c of liveClients.splice(0)) await c.disconnect();
      await hub.stop();
    }
  }, 30000);

  it('two documents opened by the same client have different actors', async () => {
    const hub: TestHub = await startTestHub();
    const liveClients: SyncClient[] = [];
    try {
      const c = client(liveClients);
      await c.createNewProject(
        {
          syncServer: hub.url,
          files: [
            { path: 'a.qmd', content: 'a\n', contentType: 'text' },
            { path: 'b.qmd', content: 'b\n', contentType: 'text' },
          ],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        },
        AUTHOR,
        'Creator',
      );

      const actors = new Set([
        automergeGetActorId(c.getIndexHandle()!.doc()!),
        automergeGetActorId(c.getFileHandle('a.qmd')!.doc()!),
        automergeGetActorId(c.getFileHandle('b.qmd')!.doc()!),
      ]);
      expect(actors.size).toBe(3);
    } finally {
      for (const c of liveClients.splice(0)) await c.disconnect();
      await hub.stop();
    }
  }, 30000);

  it('the same document opened by two client instances has different actors (the bd-6f21d4c6 guarantee)', async () => {
    const hub: TestHub = await startTestHub();
    const liveClients: SyncClient[] = [];
    try {
      const creator = client(liveClients);
      const result = await creator.createNewProject(
        {
          syncServer: hub.url,
          files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        },
        AUTHOR,
        'Creator',
      );
      expect(await hub.hubHasHeadsOf(creator, 8000)).toBe(true);

      // Same user (same author), second tab.
      const reader = client(liveClients);
      await reader.connect(hub.url, result.indexDocId, AUTHOR, 'Creator', undefined, ONLINE);

      const actorA = automergeGetActorId(creator.getIndexHandle()!.doc()!);
      const actorB = automergeGetActorId(reader.getIndexHandle()!.doc()!);
      expect(actorA).not.toBe(actorB);
    } finally {
      for (const c of liveClients.splice(0)) await c.disconnect();
      await hub.stop();
    }
  }, 30000);

  it('convergence proof: two same-author clients edit simultaneously — no duplicate-seq, no self-heal, both converge', async () => {
    const hub: TestHub = await startTestHub();
    const liveClients: SyncClient[] = [];
    try {
      const consoleLogSpy = vi.spyOn(console, 'log');
      const consoleErrorSpy = vi.spyOn(console, 'error');
      const consoleWarnSpy = vi.spyOn(console, 'warn');

      const creator = client(liveClients);
      const result = await creator.createNewProject(
        {
          syncServer: hub.url,
          files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        },
        AUTHOR,
        'Creator',
      );
      expect(await hub.hubHasHeadsOf(creator, 8000)).toBe(true);

      const reader = client(liveClients);
      await reader.connect(hub.url, result.indexDocId, AUTHOR, 'Creator', undefined, ONLINE);

      // The literal bd-6f21d4c6 race: both tabs commit index-doc edits
      // synchronously back to back, neither having seen the other's.
      const creatorEdit = creator.createFile('creator-edit.qmd', 'from creator\n');
      const readerEdit = reader.createFile('reader-edit.qmd', 'from reader\n');
      await Promise.all([creatorEdit, readerEdit]);

      // Positive condition first: both tabs converge on both files. A
      // wedged (duplicate-seq) client never converges — this is what the
      // pre-transition stack failed to do.
      const converged = await pollUntil(
        () =>
          Boolean(filesOf(creator)?.['creator-edit.qmd']) &&
          Boolean(filesOf(creator)?.['reader-edit.qmd']) &&
          Boolean(filesOf(reader)?.['creator-edit.qmd']) &&
          Boolean(filesOf(reader)?.['reader-edit.qmd']),
        15000,
      );
      expect(converged).toBe(true);

      // No duplicate-seq error crossed anyone's sync path...
      const sawDuplicateSeq = [...consoleLogSpy.mock.calls, ...consoleErrorSpy.mock.calls].some(
        (args) =>
          args.some(
            (a) => a instanceof RangeError && /duplicate seq/.test(a.message),
          ),
      );
      expect(sawDuplicateSeq).toBe(false);

      // ...and the self-heal safety net never fired.
      const sawRecovery = consoleWarnSpy.mock.calls.some(
        (args) => typeof args[0] === 'string' && args[0].includes('recovered index document'),
      );
      expect(sawRecovery).toBe(false);

      // Both tabs' edits resolve to the same author despite their
      // distinct actors.
      for (const c of [creator, reader]) {
        const doc = c.getIndexHandle()!.doc()!;
        for (const change of localChangesOf(doc as Doc<unknown>)) {
          expect(attributionKey(doc as Doc<unknown>, change)).toBe(AUTHOR);
        }
      }
    } finally {
      for (const c of liveClients.splice(0)) await c.disconnect();
      await hub.stop();
    }
  }, 45000);
});
