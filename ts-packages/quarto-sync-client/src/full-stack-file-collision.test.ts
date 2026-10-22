/**
 * Full end-to-end repro AND fix verification for a duplicate-seq actor
 * collision on a FILE document (bd-0h8crv6u; see
 * claude-notes/plans/2026-10-22-file-doc-duplicate-seq-recovery.md).
 * Sibling of the (since deleted) index-document full-stack collision
 * test: `actor-id-collision.test.ts` proves the root-cause mechanism at
 * the bare-automerge level; this test drives it through a real
 * `SyncClient` connected to a real `startTestHub()`, with a raw `Repo`
 * playing the legacy (pre-author-transition) client whose stable
 * per-user actor id collides with the victim's own.
 *
 * Collision geometry (why exactly one client wedges): the test hub's
 * websocket relay is hub-and-spoke — clients sync only with the hub's
 * repo, never directly with each other — and a JS automerge-repo can
 * never hold two divergent changes (it accepts the first, wedges on the
 * second). The permanently wedged client is therefore the one whose
 * LOCAL divergent edit loses the race to the hub: the hub holds and
 * re-offers the winning change, which collides with the loser's local
 * copy on every retry. This is the production shape observed on
 * 2026-10-22 ("wedged for that browser", singular).
 *
 * Determinism: which side loses the race to the hub would normally be
 * network timing. This test makes it deterministic by connecting the
 * victim through a traffic-gating NetworkAdapter wrapper (the existing
 * `setNetworkAdapterWrapper` seam): the gate is closed while the victim
 * commits its local edit (so the edit stays local), the legacy peer's
 * doctored divergent change lands on the hub first, and only then does
 * the gate open — the hub's offer of the legacy change and the victim's
 * flushed offer of its own change then collide at both ends.
 *
 * What this covers that the bare-automerge repro does not:
 *
 *   1. the actual swallowed catch in automerge-repo's own `Repo` fires
 *      against the real library code hub-client ships (both known log
 *      shapes are accepted — see the poll below);
 *   2. `SyncClient`'s fix (`installDuplicateSeqRecovery` routing to
 *      `recoverFileDocument` in client.ts) detects that swallowed error
 *      for a tracked file document and self-heals: `repo.delete()`
 *      followed by a fresh `repo.find()`, then re-fires `onFileAdded`;
 *   3. after recovery, later edits from an uninvolved third client reach
 *      BOTH the (formerly wedged) victim and a healthy witness — without
 *      the fix the victim never receives them (the hub re-offers the
 *      colliding change on every new sync message forever).
 */

import { describe, it, expect, vi } from 'vitest';
import {
  change as automergeChange,
  clone as automergeClone,
  decodeChange as automergeDecodeChange,
  getAllChanges as automergeGetAllChanges,
  getHeads as automergeGetHeads,
  save as automergeSave,
} from '@automerge/automerge';
import {
  NetworkAdapter,
  Repo,
  type DocumentId,
  type Message,
  type PeerId,
  type PeerMetadata,
} from '@automerge/automerge-repo';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';

import {
  createSyncClient,
  setNetworkAdapterWrapper,
  type SyncClient,
} from './client.js';
import { NodeWebSocketClientAdapter } from './NodeWebSocketClientAdapter.js';
import { startTestHub, type TestHub } from './test-hub.js';

/**
 * NetworkAdapter wrapper that queues ALL traffic — inbound 'message'
 * events and outbound send() calls — while closed, flushing in arrival
 * order when re-opened. Peer handshake and close events are never held:
 * the victim must finish its handshake with the hub before the gate
 * closes, and nothing about the collision depends on them.
 */
class TrafficGate extends NetworkAdapter {
  #wrapped: NetworkAdapter;
  #open = true;
  #queue: Array<{ direction: 'in' | 'out'; message: Message }> = [];

  constructor(wrapped: NetworkAdapter) {
    super();
    this.#wrapped = wrapped;
    this.#wrapped.on('message', (message: Message) => {
      if (this.#open) {
        this.emit('message', message);
      } else {
        this.#queue.push({ direction: 'in', message });
      }
    });
    this.#wrapped.on('peer-candidate', (payload) => this.emit('peer-candidate', payload));
    this.#wrapped.on('peer-disconnected', (payload) => this.emit('peer-disconnected', payload));
    this.#wrapped.on('close', () => this.emit('close'));
  }

  connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    this.#wrapped.connect(peerId, peerMetadata);
  }

  send(message: Message): void {
    if (this.#open) {
      this.#wrapped.send(message);
    } else {
      this.#queue.push({ direction: 'out', message });
    }
  }

  disconnect(): void {
    this.#wrapped.disconnect();
  }

  isReady(): boolean {
    return this.#wrapped.isReady();
  }

  whenReady(): Promise<void> {
    return this.#wrapped.whenReady();
  }

  close(): void {
    this.#open = false;
  }

  open(): void {
    this.#open = true;
    for (const item of this.#queue.splice(0)) {
      if (item.direction === 'out') {
        this.#wrapped.send(item.message);
      } else {
        this.emit('message', item.message);
      }
    }
  }
}

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

describe('H4-file full-stack repro + fix: legacy stable-actor peer + real hub relay', () => {
  it(
    'a duplicate-seq collision on a file document, swallowed by Repo.ts, is self-healed by recoverFileDocument',
    async () => {
      const hub: TestHub = await startTestHub();
      const liveClients: SyncClient[] = [];
      let legacyRepo: Repo | null = null;
      try {
        const consoleLogSpy = vi.spyOn(console, 'log');
        const consoleErrorSpy = vi.spyOn(console, 'error');
        const consoleWarnSpy = vi.spyOn(console, 'warn');

        // 1. The victim creates the project (gate OPEN so creation
        // syncs). Its file-document instance actor — automerge's random
        // per-instance actor — is the actor the legacy peer will steal.
        let gate: TrafficGate | null = null;
        setNetworkAdapterWrapper((adapter) => {
          gate = new TrafficGate(adapter);
          return gate;
        });
        const victim = client(liveClients);
        const result = await victim.createNewProject({
          syncServer: hub.url,
          files: [{ path: 'main.qmd', content: 'hello\n', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
        });
        setNetworkAdapterWrapper(null);
        expect(gate).not.toBeNull();
        expect(await hub.hubHasHeadsOf(victim, 8000)).toBe(true);

        const fileDocId = String(result.files[0]!.docId);
        expect(result.files[0]!.path).toBe('main.qmd');

        // 2. A witness client connects and loads the file. It makes no
        // divergent edit, so it accepts the legacy change cleanly and
        // stays healthy throughout — the control proving edits land on
        // the hub.
        const witness = client(liveClients);
        await witness.connect(hub.url, result.indexDocId, undefined, undefined, undefined, {
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
          findDocRetry: { attempts: 1, baseDelayMs: 10 },
        });
        expect(witness.getFileContent('main.qmd')).toBe('hello\n');

        // 3. Read the victim's file-document actor from the hub's copy
        // (the head change's actor) and build the legacy peer's
        // divergent copy: same document, same actor, one edit the
        // victim has not seen — the `clone(doc, { actor })` recipe from
        // actor-id-collision.test.ts.
        const hubHandle = await hub.repo.find<TextDocumentContent>(fileDocId as DocumentId);
        const hubDoc = hubHandle.doc();
        expect(hubDoc).toBeDefined();
        const headHashes = automergeGetHeads(hubDoc!);
        const headChange = automergeGetAllChanges(hubDoc!)
          .map((c) => automergeDecodeChange(c))
          .find((c) => c.hash === headHashes[0]);
        expect(headChange).toBeDefined();
        const victimActor = headChange!.actor;

        const legacyDoc = automergeChange(
          automergeClone(hubDoc!, { actor: victimActor }),
          (d: TextDocumentContent) => {
            d.text = 'edit from the legacy peer\n';
          },
        );
        legacyRepo = new Repo({ sharePolicy: async () => true });
        const legacyHandle = legacyRepo.import<TextDocumentContent>(automergeSave(legacyDoc), {
          docId: fileDocId as DocumentId,
        });
        const legacyHeads = legacyHandle.heads();

        // 4. Close the gate and let the victim commit its own divergent
        // edit: same actor, same seq as the legacy peer's edit, different
        // change — the collision. It stays local while the gate is shut.
        gate!.close();
        victim.updateFileContent('main.qmd', 'edit from the victim\n');
        expect(victim.getFileContent('main.qmd')).toBe('edit from the victim\n');

        // 5. The legacy peer joins and pushes its change to the hub.
        // The hub (still at the common base) accepts it — deterministically
        // first, because the victim's offer is stuck behind the gate. The
        // hub's offer of the legacy change to the victim queues at the gate.
        legacyRepo.networkSubsystem.addNetworkAdapter(
          new NodeWebSocketClientAdapter(hub.url, {
            getBearer: async () => 'unused-by-test-hub',
            retryInterval: 0,
          }),
        );
        expect(await hub.hubHasHeads(fileDocId, legacyHeads, 8000)).toBe(true);
        // The witness accepted the legacy change from the hub: healthy.
        expect(
          await pollUntil(() => witness.getFileContent('main.qmd') === 'edit from the legacy peer\n'),
        ).toBe(true);

        // 6. Open the gate: the hub's offer of the legacy change reaches
        // the victim (whose local edit collides with it — the wedge), and
        // the victim's queued offer reaches the hub (which wedges on it
        // the other way). From here the hub re-offers the colliding
        // change to the victim on every new sync round, forever — without
        // the fix the victim never processes another message for this doc.
        gate!.open();

        // 6a. The collision + swallowed catch really happened somewhere
        // in the real stack (victim and/or hub). Checks both known
        // automerge-repo log shapes rather than one hardcoded version's.
        const isDuplicateSeqSwallow = (args: unknown[]): boolean => {
          // v2.5.6: console.log("error receiving message", { err, message })
          if (args[0] === 'error receiving message') {
            const err = (args[1] as { err?: unknown } | undefined)?.err;
            return (
              err instanceof RangeError && /duplicate seq \d+ found for actor/.test(err.message)
            );
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

        // 6b. The fix: the victim detected the swallowed error for its
        // tracked file document and self-healed — repo.delete() + a fresh
        // repo.find(), then re-subscribe.
        const sawRecoveryStart = await pollUntil(() =>
          consoleWarnSpy.mock.calls.some(
            (args) =>
              typeof args[0] === 'string' &&
              args[0].includes('file document') &&
              args[0].includes(fileDocId) &&
              args[0].includes('main.qmd') &&
              args[0].includes('duplicate-seq'),
          ),
        );
        expect(sawRecoveryStart).toBe(true);
        const sawRecovered = await pollUntil(() =>
          consoleWarnSpy.mock.calls.some(
            (args) =>
              typeof args[0] === 'string' &&
              args[0].includes('recovered file document') &&
              args[0].includes(fileDocId) &&
              args[0].includes('main.qmd'),
          ),
        );
        expect(sawRecovered).toBe(true);

        // Recovery discards the victim's unwinnable local edit and
        // converges on the hub's (the legacy peer's) content.
        expect(
          await pollUntil(
            () => victim.getFileContent('main.qmd') === 'edit from the legacy peer\n',
          ),
        ).toBe(true);

        // 7. The real test: a further, unrelated edit — a THIRD client
        // with its own actor, so it cannot itself collide — must now
        // reach the formerly wedged victim (and, as control, the healthy
        // witness). Without the fix the victim's per-peer sync state with
        // the hub never advances and this poll consumes its full timeout.
        const editor = client(liveClients);
        await editor.connect(hub.url, result.indexDocId, undefined, undefined, undefined, {
          storage: 'memory',
          peerTimeoutMs: 10000,
          requireOnline: true,
          findDocRetry: { attempts: 1, baseDelayMs: 10 },
        });
        editor.updateFileContent('main.qmd', 'post-collision edit 1\n');

        const victimGotFirst = await pollUntil(
          () => victim.getFileContent('main.qmd') === 'post-collision edit 1\n',
          8000,
        );
        const witnessGotFirst = await pollUntil(
          () => witness.getFileContent('main.qmd') === 'post-collision edit 1\n',
          5000,
        );
        expect(witnessGotFirst).toBe(true);
        expect(victimGotFirst).toBe(true);

        // 8. Recovery is durable, not a one-off catch-up: a second,
        // later edit reaches everyone normally too.
        editor.updateFileContent('main.qmd', 'post-collision edit 2\n');
        const victimGotSecond = await pollUntil(
          () => victim.getFileContent('main.qmd') === 'post-collision edit 2\n',
          8000,
        );
        const witnessGotSecond = await pollUntil(
          () => witness.getFileContent('main.qmd') === 'post-collision edit 2\n',
          5000,
        );
        expect(witnessGotSecond).toBe(true);
        expect(victimGotSecond).toBe(true);
      } finally {
        setNetworkAdapterWrapper(null);
        for (const c of liveClients.splice(0)) {
          await c.disconnect();
        }
        if (legacyRepo) {
          await legacyRepo.shutdown().catch(() => {});
        }
        await hub.stop();
      }
    },
    60000,
  );
});
