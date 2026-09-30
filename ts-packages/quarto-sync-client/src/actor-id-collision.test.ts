/**
 * Hazard documentation for the duplicate-seq wedge (bd-6f21d4c6; see
 * claude-notes/plans/2026-09-17-index-doc-duplicate-seq-self-heal.md):
 * reusing the same Automerge actor id across two independent,
 * concurrently-edited copies of a document deterministically produces
 * `RangeError: duplicate seq N found for actor <id>` on
 * `receiveSyncMessage` — no automerge-repo, no network, no hub needed.
 * Round 1 of the protocol exchanges heads/haves only (no change bytes
 * yet, per the sync protocol's own design); round 2 is where the actual
 * colliding change bytes cross the wire and the receiving side's
 * `apply_changes_batch_log_patches` rejects them.
 *
 * This was the production failure mode of the pre-transition stable
 * actor id (actorIdFromUserId / GET /auth/actor). Post-transition
 * (author-ID epic bd-o1yn1fqy) no q2 code chooses actor IDs: every
 * document instance gets automerge's random actor (D1), so this setup
 * is no longer reachable through the sync client — the two-tab
 * convergence proof lives in author-id.test.ts. The tests stay because
 * the hazard stays: throwing on a genuine actor-identity violation is
 * correct upstream behavior, and anyone tempted to reintroduce explicit
 * actor management (per-device actors, "stable" actor prefixes) should
 * watch exactly how it wedges. Note the parallel with automerge's own
 * author support: `set_author` re-randomizes the actor precisely
 * because authors and actors must never be conflated.
 */

import { describe, it, expect } from 'vitest';
import {
  init,
  change,
  clone,
  initSyncState,
  generateSyncMessage,
  receiveSyncMessage,
  getHeads,
} from '@automerge/automerge';

describe('reused actor id across two concurrently-edited copies (H4 root cause)', () => {
  it('throws duplicate-seq on receiveSyncMessage once both sides have made a colliding local edit', () => {
    const SHARED_ACTOR = 'aaaaaaaaaaaaaaaa'; // stands in for actorIdFromUserId's output, reused across two sessions

    // A common starting point, as if a project's index doc (or a file
    // doc) had already synced once.
    let common = init<{ text: string }>({ actor: SHARED_ACTOR });
    common = change(common, (d) => {
      d.text = '';
    });

    // Two "sessions" (two tabs, or a stale tab plus a fresh reconnect)
    // for the same user, forked from that point, both using the
    // identical actor id.
    let sessionA = common; // e.g. the hub's already-accepted copy
    let sessionB = clone(common, { actor: SHARED_ACTOR });

    // Each makes its own local edit before either has seen the other's.
    sessionA = change(sessionA, (d) => {
      d.text = 'edit from session A';
    });
    sessionB = change(sessionB, (d) => {
      d.text = 'edit from session B';
    });

    expect(getHeads(sessionA)).not.toEqual(getHeads(sessionB));

    // Drive the real sync protocol, exactly as DocSynchronizer does:
    // exchange messages in rounds until one side rejects the other's
    // changes or both converge.
    let ssA = initSyncState();
    let ssB = initSyncState();

    let thrown: unknown;
    for (let round = 0; round < 5 && !thrown; round++) {
      const [nextSsA, msgAtoB] = generateSyncMessage(sessionA, ssA);
      const [nextSsB, msgBtoA] = generateSyncMessage(sessionB, ssB);
      ssA = nextSsA;
      ssB = nextSsB;

      if (msgAtoB) {
        try {
          [sessionB, ssB] = receiveSyncMessage(sessionB, ssB, msgAtoB);
        } catch (err) {
          thrown = err;
          break;
        }
      }
      if (msgBtoA) {
        try {
          [sessionA, ssA] = receiveSyncMessage(sessionA, ssA, msgBtoA);
        } catch (err) {
          thrown = err;
          break;
        }
      }
      if (!msgAtoB && !msgBtoA) break; // converged cleanly — would mean the collision didn't reproduce
    }

    expect(thrown).toBeInstanceOf(RangeError);
    expect((thrown as RangeError).message).toMatch(
      /duplicate seq \d+ found for actor aaaaaaaaaaaaaaaa/,
    );
  });

  it('control: two DIFFERENT actor ids making the same concurrent edits converge normally', () => {
    let common = init<{ text: string }>({ actor: 'aaaaaaaaaaaaaaaa' });
    common = change(common, (d) => {
      d.text = '';
    });

    let sessionA = common;
    let sessionB = clone(common, { actor: 'bbbbbbbbbbbbbbbb' }); // different actor — the normal, safe case

    sessionA = change(sessionA, (d) => {
      d.text = 'edit from session A';
    });
    sessionB = change(sessionB, (d) => {
      d.text = 'edit from session B';
    });

    let ssA = initSyncState();
    let ssB = initSyncState();

    for (let round = 0; round < 5; round++) {
      const [nextSsA, msgAtoB] = generateSyncMessage(sessionA, ssA);
      const [nextSsB, msgBtoA] = generateSyncMessage(sessionB, ssB);
      ssA = nextSsA;
      ssB = nextSsB;

      if (msgAtoB) [sessionB, ssB] = receiveSyncMessage(sessionB, ssB, msgAtoB);
      if (msgBtoA) [sessionA, ssA] = receiveSyncMessage(sessionA, ssA, msgBtoA);
      if (!msgAtoB && !msgBtoA) break;
    }

    // Ordinary CRDT concurrent-edit convergence: no throw, both sides end up
    // with the same (merged/conflicted) heads.
    expect(getHeads(sessionA)).toEqual(getHeads(sessionB));
  });
});
