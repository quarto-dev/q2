/**
 * Characterization test for H1 tier 1 in
 * claude-notes/research/2026-09-16-automerge-index-doc-staleness.md
 * (bd-6f21d4c6): does `CollectionSynchronizer.addPeer`'s dedup guard
 * (automerge-repo v2.5.6, `CollectionSynchronizer.ts:179-193`) —
 * `if (this.#peers.has(peerId)) return` — actually suppress `beginSync`
 * when a second `peer-candidate` for an already-known `peerId` arrives
 * with no intervening `peer-disconnected`?
 *
 * Answer, settled directly against the vendored source: **yes.** Nothing
 * upstream of `addPeer` covers for it either —
 * `NetworkSubsystem.addNetworkAdapter`'s `peer-candidate` handler
 * (`NetworkSubsystem.ts:52-66`) unconditionally re-emits `"peer"` on every
 * `peer-candidate`, with its own acknowledged TODO ("on reconnection, this
 * would create problems!"); the *only* dedup in this path is
 * `CollectionSynchronizer.addPeer`'s early return.
 *
 * A hand-written fake `NetworkAdapter` fully authors the event sequence
 * itself here, so there is nothing to race — this settles the factual
 * question mechanically, not probabilistically. It is not a fix-driving
 * red test: it demonstrates *existing, real* upstream behavior (expected
 * to pass against the current dependency version), proving the mechanism
 * is real before any fix is considered.
 */

import { describe, it, expect, vi } from 'vitest';
import {
  Repo,
  NetworkAdapter,
  type PeerId,
  type PeerMetadata,
} from '@automerge/automerge-repo';

/** Flush pending microtasks. */
async function flushMicrotasks(rounds = 10) {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

/**
 * Minimal fake NetworkAdapter: no real transport, just the event surface
 * CollectionSynchronizer/Repo actually consume (`peer-candidate` /
 * `peer-disconnected` / `message`), driven directly by the test.
 */
class FakeNetworkAdapter extends NetworkAdapter {
  isReady(): boolean {
    return true;
  }
  async whenReady(): Promise<void> {}
  connect(peerId: PeerId): void {
    this.peerId = peerId;
  }
  send(): void {}
  disconnect(): void {}

  emitPeerCandidate(peerId: PeerId, peerMetadata: PeerMetadata = {}): void {
    this.emit('peer-candidate', { peerId, peerMetadata });
  }
  emitPeerDisconnected(peerId: PeerId): void {
    this.emit('peer-disconnected', { peerId });
  }
}

describe('CollectionSynchronizer.addPeer peer-candidate dedup (H1 tier 1)', () => {
  it('suppresses beginSync on a repeat peer-candidate with no intervening peer-disconnected, but not when one intervenes', async () => {
    const adapter = new FakeNetworkAdapter();
    const repo = new Repo({
      network: [adapter],
      peerId: 'me' as PeerId,
      sharePolicy: async () => true,
    });

    const handle = repo.create<{ text: string }>({ text: 'hi' });
    await handle.whenReady();

    const docSynchronizer = repo.synchronizer.docSynchronizers[handle.documentId];
    expect(docSynchronizer).toBeDefined();
    const beginSyncSpy = vi.spyOn(docSynchronizer, 'beginSync');

    const peerX = 'peer-x' as PeerId;

    // First peer-candidate for X: a genuinely new peer, so addPeer must
    // start syncing every already-registered document with it.
    adapter.emitPeerCandidate(peerX);
    await flushMicrotasks();
    expect(beginSyncSpy).toHaveBeenCalledTimes(1);
    expect(beginSyncSpy).toHaveBeenLastCalledWith([peerX]);

    // The ordering violation H1 describes: a second peer-candidate for
    // the SAME peerId, with no peer-disconnected in between (e.g. two
    // socket lifecycles briefly overlapping across a reconnect).
    adapter.emitPeerCandidate(peerX);
    await flushMicrotasks();
    // The dedup guard (`if (this.#peers.has(peerId)) return`) treats X as
    // already connected and returns early — beginSync is NOT called again.
    expect(beginSyncSpy).toHaveBeenCalledTimes(1);

    // Control: with a peer-disconnected between the two candidates, the
    // guard's precondition (`#peers.has(peerId)`) no longer holds, and
    // beginSync fires again as expected.
    adapter.emitPeerDisconnected(peerX);
    adapter.emitPeerCandidate(peerX);
    await flushMicrotasks();
    expect(beginSyncSpy).toHaveBeenCalledTimes(2);
  });
});
