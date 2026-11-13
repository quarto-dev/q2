/**
 * Attribution at a point in history (replay).
 *
 * A replay step is one change hash; its document is that change plus its
 * ancestors. These tests pin that the runs built for a step are the runs of
 * the document as it was at that step, not those of the live heads.
 */

import { describe, expect, it } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';

import {
  buildRunListAttribution,
  createReplayIndex,
  TargetNotAheadError,
  updateRunListAttribution,
} from './attribution-runs';
import type { AttributionRun } from './attribution-runs';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);
const CAROL = 'c'.repeat(32);

function fakeHandle(doc: Doc<TDoc>): DocHandle<unknown> {
  return { doc: () => doc } as unknown as DocHandle<unknown>;
}

function insert(doc: Doc<TDoc>, at: number, value: string, time: number): Doc<TDoc> {
  return A.change(doc, { time }, d => A.splice(d, ['text'], at, 0, value));
}

/** Three authors; Bob and Carol edit concurrently, then Alice merges and deletes. */
function threeAuthorHistory(): Doc<TDoc> {
  let alice = A.from<TDoc>({ text: '' }, { actor: ALICE });
  alice = insert(alice, 0, 'alpha beta gamma', 1);

  let bob = A.clone(alice, { actor: BOB });
  bob = insert(bob, 6, 'bravo ', 2);
  bob = insert(bob, 0, 'B: ', 3);

  let carol = A.clone(alice, { actor: CAROL });
  carol = insert(carol, 16, ' charlie', 2);
  carol = insert(carol, 0, 'C: ', 4);

  alice = A.merge(A.merge(alice, bob), carol);
  alice = A.change(alice, { time: 5 }, d => A.splice(d, ['text'], 0, 3));
  alice = insert(alice, alice.text.length, ' end', 6);
  return alice;
}

/** The document as it was at `hash`, as an independent doc. */
function forkAt(doc: Doc<TDoc>, hash: string): Doc<TDoc> {
  return A.clone(A.view(doc, [hash]));
}

async function runsOfFork(doc: Doc<TDoc>, hash: string): Promise<AttributionRun[]> {
  return (await buildRunListAttribution(fakeHandle(forkAt(doc, hash)), 'text'))!.runs;
}

function expectTiles(runs: AttributionRun[], text: string): void {
  let at = 0;
  for (const r of runs) {
    expect(r.start).toBe(at);
    at = r.end;
  }
  expect(at).toBe(text.length);
}

describe('attribution at replay heads', () => {
  it('builds, for every step, the runs of the document as it was at that step', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const index = createReplayIndex();

    for (const hash of A.topoHistoryTraversal(doc)) {
      const state = await buildRunListAttribution(handle, 'text', undefined, { heads: [hash], index });
      expectTiles(state!.runs, A.view(doc, [hash]).text);
      expect(state!.runs).toEqual(await runsOfFork(doc, hash));
    }
  });

  it('differs from the live-head runs on an earlier step', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const first = A.topoHistoryTraversal(doc)[1];

    const live = await buildRunListAttribution(handle, 'text');
    const atStep = await buildRunListAttribution(handle, 'text', undefined, {
      heads: [first],
      index: createReplayIndex(),
    });

    expect(atStep!.runs).toEqual([{ start: 0, end: 16, actor: ALICE, time: 1 }]);
    expect(live!.runs).not.toEqual(atStep!.runs);
  });

  it('steps forward incrementally and matches a fresh build', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    doc = insert(doc, 0, 'one', 1);
    doc = A.clone(doc, { actor: BOB });
    doc = insert(doc, 3, ' two', 2);
    doc = A.clone(doc, { actor: CAROL });
    doc = insert(doc, 0, 'zero ', 3);
    const handle = fakeHandle(doc);
    const index = createReplayIndex();
    const topo = A.topoHistoryTraversal(doc);

    let state = (await buildRunListAttribution(handle, 'text', undefined, { heads: [topo[0]], index }))!;
    for (const hash of topo.slice(1)) {
      const next = updateRunListAttribution(state, handle, 'text', { heads: [hash], index });
      expect(next.runs).toEqual(await runsOfFork(doc, hash));
      // The earlier state is a snapshot: stepping forward must not change it.
      expect(next.runs).not.toBe(state.runs);
      state = next;
    }
    expect(state.runs).toEqual([
      { start: 0, end: 5, actor: CAROL, time: 3 },
      { start: 5, end: 8, actor: ALICE, time: 1 },
      { start: 8, end: 12, actor: BOB, time: 2 },
    ]);
  });

  it('returns the same state when the target is where the work doc already is', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const index = createReplayIndex();
    const hash = A.topoHistoryTraversal(doc)[3];

    const state = (await buildRunListAttribution(handle, 'text', undefined, { heads: [hash], index }))!;
    expect(updateRunListAttribution(state, handle, 'text', { heads: [hash], index })).toBe(state);
  });

  it('asks for a rebuild when the target is behind the work doc', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const index = createReplayIndex();
    const topo = A.topoHistoryTraversal(doc);

    const late = (await buildRunListAttribution(handle, 'text', undefined, { heads: [topo[topo.length - 1]], index }))!;
    expect(() => updateRunListAttribution(late, handle, 'text', { heads: [topo[1]], index }))
      .toThrow(TargetNotAheadError);
  });

  it('asks for a rebuild when the target is on a concurrent branch', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const index = createReplayIndex();
    const metas = A.getChangesMetaSince(doc, []);
    const lastOf = (actor: string) => metas.filter(m => m.actor === actor && m.time < 5).at(-1)!.hash;

    const onBob = (await buildRunListAttribution(handle, 'text', undefined, { heads: [lastOf(BOB)], index }))!;
    expect(() => updateRunListAttribution(onBob, handle, 'text', { heads: [lastOf(CAROL)], index }))
      .toThrow(TargetNotAheadError);
  });

  it('drains to the live heads after a replay target', async () => {
    const doc = threeAuthorHistory();
    const handle = fakeHandle(doc);
    const topo = A.topoHistoryTraversal(doc);

    const early = (await buildRunListAttribution(handle, 'text', undefined, {
      heads: [topo[1]],
      index: createReplayIndex(),
    }))!;
    const live = updateRunListAttribution(early, handle, 'text');

    expect(live.runs).toEqual((await buildRunListAttribution(handle, 'text'))!.runs);
  });

  it('picks up changes made after the index was filled', async () => {
    let doc = threeAuthorHistory();
    const index = createReplayIndex();
    const first = A.topoHistoryTraversal(doc)[1];
    let state = (await buildRunListAttribution(fakeHandle(doc), 'text', undefined, { heads: [first], index }))!;

    doc = insert(doc, 0, 'late ', 9);
    const head = A.getHeads(doc);
    state = updateRunListAttribution(state, fakeHandle(doc), 'text', { heads: head, index });

    expectTiles(state.runs, doc.text);
    expect(state.runs[0]).toEqual({ start: 0, end: 5, actor: ALICE, time: 9 });
  });
});
