/**
 * Batching and scheduling of the attribution replay.
 *
 * One `applyChanges` call costs time proportional to the text length however
 * many changes it carries, so the producer applies consecutive changes with
 * the same attribution (key and time) in one call. These tests pin the call
 * pattern, prove the runs equal a change-by-change replay, and pin the
 * time-budgeted yields of the cold build.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Change, Doc, Patch } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';

const { batchSizes } = vi.hoisted(() => ({ batchSizes: [] as number[] }));
vi.mock('@automerge/automerge', async importOriginal => {
  const actual = await importOriginal<typeof import('@automerge/automerge')>();
  const applyChanges: typeof actual.applyChanges = (doc, changes, opts) => {
    batchSizes.push(changes.length);
    return actual.applyChanges(doc, changes, opts);
  };
  return { ...actual, applyChanges };
});

import {
  __internal,
  buildRunListAttribution,
  isTextPatch,
  MAX_BATCH_SIZE,
  SLICE_BUDGET_MS,
  updateRunListAttribution,
} from './attribution-runs';
import type { AttributionRun } from './attribution-runs';
import { stubAttributionClock } from '../test-utils/attributionColdBuild';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);

function fakeHandle(doc: Doc<TDoc>): DocHandle<unknown> {
  return { doc: () => doc } as unknown as DocHandle<unknown>;
}

function append(doc: Doc<TDoc>, value: string, time: number): Doc<TDoc> {
  return A.change(doc, { time }, d => A.splice(d, ['text'], d.text.length, 0, value));
}

/** The pre-batching algorithm: one `applyChanges` call per change. */
function replayOneByOne(doc: Doc<TDoc>): AttributionRun[] {
  const runs: AttributionRun[] = [];
  let work = A.init<TDoc>();
  for (const change of A.getChanges(A.init<TDoc>(), doc) as Change[]) {
    const { actor, time } = A.decodeChange(change);
    let patches: Patch[] = [];
    [work] = A.applyChanges(work, [change], { patchCallback: p => { patches = p; } });
    for (const patch of patches) {
      if (isTextPatch(patch, 'text')) __internal.applyPatchToRuns(runs, patch, { actor, time });
    }
  }
  return runs;
}

/** Count the cold build's yields; each idle callback runs at once. */
function countYields(): { readonly count: number } {
  let count = 0;
  vi.stubGlobal('requestIdleCallback', (callback: IdleRequestCallback) => {
    count++;
    callback({ didTimeout: false, timeRemaining: () => 50 });
    return count;
  });
  return { get count() { return count; } };
}

beforeEach(() => {
  batchSizes.length = 0;
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('attribution replay batching', () => {
  it('applies consecutive changes with the same author and second in one call', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    for (let i = 0; i < 5; i++) doc = append(doc, 'a', 10);
    for (let i = 0; i < 3; i++) doc = append(doc, 'b', 11);
    doc = A.clone(doc, { actor: BOB });
    for (let i = 0; i < 2; i++) doc = append(doc, 'c', 11);

    const state = await buildRunListAttribution(fakeHandle(doc), 'text');

    // The initial `A.from` change, then one call per (author, second) burst.
    expect(batchSizes).toEqual([1, 5, 3, 2]);
    expect(state!.runs).toEqual([
      { start: 0, end: 5, actor: ALICE, time: 10 },
      { start: 5, end: 8, actor: ALICE, time: 11 },
      { start: 8, end: 10, actor: BOB, time: 11 },
    ]);
  });

  it('batches an incremental update the same way', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    doc = append(doc, 'seed', 10);
    const before = await buildRunListAttribution(fakeHandle(doc), 'text');
    batchSizes.length = 0;

    for (let i = 0; i < 4; i++) doc = append(doc, 'x', 20);
    doc = append(doc, 'y', 21);
    const next = updateRunListAttribution(before!, fakeHandle(doc), 'text');

    expect(batchSizes).toEqual([4, 1]);
    expect(next.runs).toEqual([
      { start: 0, end: 4, actor: ALICE, time: 10 },
      { start: 4, end: 8, actor: ALICE, time: 20 },
      { start: 8, end: 9, actor: ALICE, time: 21 },
    ]);
  });

  it('caps a batch so a flood of same-second changes still has yield points', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    for (let i = 0; i < MAX_BATCH_SIZE + 1; i++) doc = append(doc, 'a', 10);

    const state = await buildRunListAttribution(fakeHandle(doc), 'text');

    expect(batchSizes).toEqual([1, MAX_BATCH_SIZE, 1]);
    expect(state!.runs).toEqual([{ start: 0, end: MAX_BATCH_SIZE + 1, actor: ALICE, time: 10 }]);
  });

  it('produces the same runs as a change-by-change replay on random histories', async () => {
    let seed = 20261110;
    const rnd = (n: number) => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed % n;
    };
    let calls = 0;
    let changes = 0;

    for (let trial = 0; trial < 60; trial++) {
      const base = A.from<TDoc>({ text: 'seed text' }, { actor: ALICE });
      const peers = [base, A.clone(base, { actor: BOB }), A.clone(base, { actor: 'c'.repeat(32) })];
      let time = 1000;
      let current = 0;
      for (let step = 0; step < 50; step++) {
        // Long single-peer streaks sharing a second, so most changes batch.
        if (rnd(6) === 0) current = rnd(3);
        if (rnd(5) === 0) time++;
        peers[current] = A.change(peers[current], { time }, d => {
          const length = d.text.length;
          const kind = rnd(10);
          if (kind === 0) {
            d.text = `reset${step}`;
          } else if (kind < 4 && length > 0) {
            const at = rnd(length);
            A.splice(d, ['text'], at, Math.min(1 + rnd(3), length - at), kind === 1 ? `R${step}` : '');
          } else {
            A.splice(d, ['text'], rnd(length + 1), 0, `i${step}`);
          }
        });
        if (rnd(5) === 0) {
          const other = rnd(3);
          if (other !== current) peers[other] = A.merge(peers[other], peers[current]);
        }
      }
      const merged = A.merge(A.merge(peers[0], peers[1]), peers[2]);

      batchSizes.length = 0;
      const state = await buildRunListAttribution(fakeHandle(merged), 'text');
      calls += batchSizes.length;
      changes += batchSizes.reduce((sum, size) => sum + size, 0);

      expect(state!.runs).toEqual(replayOneByOne(merged));
    }

    // The comparison is vacuous unless batching really happened.
    expect(calls).toBeLessThan(changes * 0.75);
  });
});

describe('cold-build yields', () => {
  it('yields between batches once the slice budget is spent', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    for (let i = 0; i < 3; i++) doc = append(doc, 'a', 10 + i);
    let clock = 0;
    stubAttributionClock(() => (clock += SLICE_BUDGET_MS));
    const yields = countYields();

    const state = await buildRunListAttribution(fakeHandle(doc), 'text');

    // Four batches, so three boundaries; nothing follows the last batch.
    expect(batchSizes).toEqual([1, 1, 1, 1]);
    expect(yields.count).toBe(3);
    expect(state!.runs).toHaveLength(3);
  });

  it('does not yield while the slice is within budget, however many changes it holds', async () => {
    let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
    for (let i = 0; i < 600; i++) doc = append(doc, 'a', 10 + i);
    stubAttributionClock(() => 0);
    const yields = countYields();

    const state = await buildRunListAttribution(fakeHandle(doc), 'text');

    expect(yields.count).toBe(0);
    expect(state!.runs).toHaveLength(600);
  });
});
