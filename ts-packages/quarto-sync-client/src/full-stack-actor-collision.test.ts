/**
 * Full end-to-end repro AND fix verification through the real
 * SyncClient/Repo/DocSynchronizer stack (bd-6f21d4c6; see
 * claude-notes/plans/2026-09-17-index-doc-duplicate-seq-self-heal.md).
 * `actor-id-collision.test.ts` already proved the root-cause
 * mechanism — two document copies sharing one Automerge actor id, each
 * independently edited, throw `RangeError: duplicate seq N found for actor
 * <id>` on `receiveSyncMessage` — at the bare-automerge level (no
 * automerge-repo, no network, no hub). This test drives the same mechanism
 * through two real `SyncClient`s connected to a real `startTestHub()`, and
 * covers what `actor-id-collision.test.ts` deliberately didn't:
 *
 *   1. the actual swallowed catch in automerge-repo's own `Repo` fires
 *      against the real library code hub-client ships, reached via a real
 *      hub relay — not a hand-rolled harness. The exact shape of that catch
 *      has changed across automerge-repo versions (async, via
 *      `receiveMessage(...).catch(err => console.log("error receiving
 *      message", ...))`, in v2.5.6; synchronous, via a `try/catch` around
 *      inbound dispatch logging `console.error(..., "error handling
 *      inbound message", err)`, from v2.6.0-alpha.5) but the underlying
 *      behavior — log and drop, no recovery — hasn't;
 *   2. `SyncClient`'s own fix (`installDuplicateSeqRecovery` /
 *      `recoverIndexDocument` in client.ts) detects that swallowed error
 *      for its own index document and self-heals: `repo.delete()` (drops
 *      the wedged per-peer sync state and persisted storage for that one
 *      document — the scoped, automated analog of the manual "clear
 *      IndexedDB" workaround) followed by a fresh `repo.find()`. No
 *      automerge-repo source is touched; the fix only wraps a public
 *      method on the `Repo` instance `SyncClient` itself created. The
 *      affected connection's index document ends up caught back up rather
 *      than permanently stuck.
 *
 * Timing / determinism note: getting both clients to fork from the same
 * starting heads under the same actor id, with neither having seen the
 * other's edit, relies on committing both local edits *synchronously* —
 * no `await` between the two `createFile()` calls below — so Node's
 * run-to-completion semantics guarantee neither client's Repo can process
 * an incoming network message from the other before its own local commit
 * lands (`createFile`'s mutation to the index doc happens before its first
 * `await`). Which of the two clients ends up on the "losing" side of the
 * actor/seq race (i.e. which one the hub's document disagrees with) is
 * real network/scheduling timing and not forced — so this test doesn't
 * presume which client recovers, only that (a) the collision + swallowed
 * catch definitely happen and (b) BOTH clients end up caught up afterward.
 * This makes it a confirming test, not a fully-controlled one.
 */

import { describe, it, expect, vi } from 'vitest';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const SHARED_ACTOR = 'aaaaaaaaaaaaaaaa'; // stands in for actorIdFromUserId's output, reused across two sessions/tabs
const OTHER_ACTOR_1 = 'cccccccccccccccc'; // an uninvolved third client's own, non-shared actor
const OTHER_ACTOR_2 = 'dddddddddddddddd'; // an uninvolved fourth client's own, non-shared actor

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

describe('H4 full-stack repro + fix: reused actor id + real hub relay', () => {
  it(
    'a real duplicate-seq collision, swallowed by Repo.ts, is self-healed by installDuplicateSeqRecovery',
    async () => {
      const hub: TestHub = await startTestHub();
      const liveClients: SyncClient[] = [];
      try {
        const consoleLogSpy = vi.spyOn(console, 'log');
        const consoleErrorSpy = vi.spyOn(console, 'error');
        const consoleWarnSpy = vi.spyOn(console, 'warn');

        // 1. Client A creates the project under the shared actor id and
        // syncs it to the hub.
        const creator = client(liveClients);
        const result = await creator.createNewProject(
          {
            syncServer: hub.url,
            files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
            storage: 'memory',
            peerTimeoutMs: 10000,
            requireOnline: true,
          },
          SHARED_ACTOR,
        );
        expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);

        // 2. Client B connects to the SAME index doc under the SAME actor
        // id — a second session/tab reusing actorIdFromUserId's stable id.
        // At this point both clients' local index-doc copies share the
        // same starting heads.
        const reader = client(liveClients);
        await reader.connect(hub.url, result.indexDocId, SHARED_ACTOR, undefined, undefined, {
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
          findDocRetry: { attempts: 1, baseDelayMs: 10 },
        });

        // 3. Race: each client commits its own local edit to the index
        // doc, synchronously back to back (no await in between — see file
        // header). Both compute "my actor's next seq" independently,
        // unaware of the other's concurrent edit.
        const creatorEdit = creator.createFile('creator-edit.qmd', 'from creator\n');
        const readerEdit = reader.createFile('reader-edit.qmd', 'from reader\n');
        await Promise.all([creatorEdit, readerEdit]);

        // 4. Give the real hub relay time to actually exchange the
        // colliding sync messages and hit the swallowed catch somewhere
        // (hub-side or either client's Repo — all three run the identical
        // automerge-repo library code and the identical swallowed catch).
        // Checks both known log shapes (see file header) rather than one
        // hardcoded version's, so this doesn't need editing on the next
        // automerge-repo bump.
        const isDuplicateSeqSwallow = (args: unknown[]): boolean => {
          // v2.5.6: console.log("error receiving message", { err, message })
          if (args[0] === 'error receiving message') {
            const err = (args[1] as { err?: unknown } | undefined)?.err;
            return err instanceof RangeError && /duplicate seq \d+ found for actor/.test(err.message);
          }
          // v2.6.0-alpha.5+: logger.error(prefix, "error handling inbound message", err)
          if (args.includes('error handling inbound message')) {
            const err = args.find((a) => a instanceof RangeError) as RangeError | undefined;
            return Boolean(err && /duplicate seq \d+ found for actor/.test(err.message));
          }
          return false;
        };
        const sawSwallowedError = await pollUntil(
          () =>
            consoleLogSpy.mock.calls.some(isDuplicateSeqSwallow) ||
            consoleErrorSpy.mock.calls.some(isDuplicateSeqSwallow),
        );
        expect(sawSwallowedError).toBe(true);

        // 4b. The fix: installDuplicateSeqRecovery (wired into every
        // connect()/createNewProject() call) should have detected that
        // same swallowed error for its own index document and logged its
        // recovery — repo.delete() + a fresh repo.find() for indexDocId.
        const sawRecovery = await pollUntil(() =>
          consoleWarnSpy.mock.calls.some(
            (args) =>
              typeof args[0] === 'string' &&
              args[0].includes('recovered index document') &&
              args[0].includes(result.indexDocId),
          ),
        );
        expect(sawRecovery).toBe(true);

        // 5. The real test: a further, completely unrelated edit — made
        // by a THIRD client with its OWN, non-shared actor id, so it can't
        // itself collide with anything — must now reach BOTH previously
        // colliding-actor clients, not stay permanently stuck.
        const canary = client(liveClients);
        await canary.connect(hub.url, result.indexDocId, OTHER_ACTOR_1, undefined, undefined, {
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
          findDocRetry: { attempts: 1, baseDelayMs: 10 },
        });
        await canary.createFile('post-collision-canary.qmd', 'should reach everyone healthy\n');

        // Sanity: the canary edit really did land on the hub and is
        // fetchable by a brand-new, uninvolved connection — ruling out
        // "the whole test setup is broken" / "nobody ever gets it".
        const control = client(liveClients);
        await control.connect(hub.url, result.indexDocId, OTHER_ACTOR_2, undefined, undefined, {
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
          findDocRetry: { attempts: 1, baseDelayMs: 10 },
        });
        const controlGotIt = await pollUntil(
          () => Boolean(filesOf(control)?.['post-collision-canary.qmd']),
          5000,
        );
        expect(controlGotIt).toBe(true);

        // The fix in action: BOTH colliding-actor clients must now see
        // it too — recovered, not permanently stuck. A generous window
        // accounts for the recovery round trip (repo.delete + a fresh
        // repo.find) on top of ordinary sync latency.
        const creatorGotIt = await pollUntil(
          () => Boolean(filesOf(creator)?.['post-collision-canary.qmd']),
          8000,
        );
        const readerGotIt = await pollUntil(
          () => Boolean(filesOf(reader)?.['post-collision-canary.qmd']),
          8000,
        );
        expect(creatorGotIt).toBe(true);
        expect(readerGotIt).toBe(true);

        // 6. Confirm recovery is durable, not a one-off catch-up: a
        // second, later, independent edit reaches both clients normally
        // too — ordinary sync resumed, not just one lucky retry.
        await canary.createFile('post-collision-canary-2.qmd', 'a second, later edit\n');
        const creatorGotSecond = await pollUntil(
          () => Boolean(filesOf(creator)?.['post-collision-canary-2.qmd']),
          5000,
        );
        const readerGotSecond = await pollUntil(
          () => Boolean(filesOf(reader)?.['post-collision-canary-2.qmd']),
          5000,
        );
        expect(creatorGotSecond).toBe(true);
        expect(readerGotSecond).toBe(true);
      } finally {
        for (const c of liveClients.splice(0)) {
          await c.disconnect();
        }
        await hub.stop();
      }
    },
    45000,
  );
});
