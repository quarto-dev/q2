/**
 * Live attribution under concurrent editing: several authors editing the
 * same sentence at once, merged in arbitrary order.
 *
 * The oracle is independent of the producer's patch replay: Automerge's
 * cursor for a character names the operation that inserted it
 * (`<counter>@<actor>`), so it gives each character's author directly.
 */

import { describe, expect, it } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';

import { buildRunListAttribution, updateRunListAttribution } from './attribution-runs';
import type { AttributionRun } from './attribution-runs';

interface TDoc { text: string }

const ACTORS = ['a', 'b', 'c', 'd'].map(c => c.repeat(32));

function fakeHandle(get: () => Doc<TDoc>): DocHandle<unknown> {
  return { doc: get } as unknown as DocHandle<unknown>;
}

/** Each character's author, read from the id of the op that inserted it. */
function authorsByCursor(doc: Doc<TDoc>): string[] {
  return Array.from({ length: doc.text.length }, (_, i) => {
    const cursor = A.getCursor(doc, ['text'], i);
    return cursor.slice(cursor.indexOf('@') + 1);
  });
}

function authorsByRuns(runs: AttributionRun[], length: number): string[] {
  const out: string[] = new Array(length).fill('?');
  let at = 0;
  for (const r of runs) {
    expect(r.start).toBe(at);
    for (let i = r.start; i < r.end; i++) out[i] = r.actor;
    at = r.end;
  }
  expect(at).toBe(length);
  return out;
}

function rng(seed: number) {
  let s = seed;
  return (n: number) => {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    return s % n;
  };
}

describe('live attribution with concurrent authors', () => {
  it('attributes every character to the author who typed it', async () => {
    const rnd = rng(20261113);

    for (let trial = 0; trial < 40; trial++) {
      // One shared sentence, then each author edits their own replica of it.
      let base = A.from<TDoc>({ text: '' }, { actor: ACTORS[0] });
      base = A.change(base, { time: 1 }, d => A.splice(d, ['text'], 0, 0, 'the quick brown fox jumps'));
      const replicas = ACTORS.map((actor, i) => (i === 0 ? base : A.clone(base, { actor })));

      // The observer's doc, and an attribution state kept up incrementally.
      let live = A.clone(base, { actor: 'e'.repeat(32) });
      let state = (await buildRunListAttribution(fakeHandle(() => live), 'text'))!;
      let time = 2;

      for (let round = 0; round < 12; round++) {
        // Concurrent edits: every author types and deletes inside the sentence.
        for (let i = 0; i < replicas.length; i++) {
          for (let k = rnd(4); k > 0; k--) {
            replicas[i] = A.change(replicas[i], { time: time + rnd(2) }, d => {
              const len = d.text.length;
              if (len > 4 && rnd(4) === 0) {
                A.splice(d, ['text'], rnd(len - 2), 1 + rnd(2));
              } else {
                A.splice(d, ['text'], rnd(len + 1), 0, 'xyz '.slice(0, 1 + rnd(4)));
              }
            });
          }
        }
        time += 2;

        // The observer hears from the authors in a random order; some
        // authors also hear from each other.
        for (const i of [0, 1, 2, 3].sort(() => rnd(3) - 1)) {
          if (rnd(3) === 0) continue;
          live = A.merge(live, replicas[i]);
          state = updateRunListAttribution(state, fakeHandle(() => live), 'text');
          expect(authorsByRuns(state.runs, live.text.length)).toEqual(authorsByCursor(live));
        }
        const from = rnd(replicas.length);
        const to = rnd(replicas.length);
        if (from !== to) replicas[to] = A.merge(replicas[to], replicas[from]);
      }

      // Everyone converges; incremental and cold builds agree with the oracle.
      for (const r of replicas) live = A.merge(live, r);
      state = updateRunListAttribution(state, fakeHandle(() => live), 'text');
      const truth = authorsByCursor(live);
      expect(authorsByRuns(state.runs, live.text.length)).toEqual(truth);
      const cold = (await buildRunListAttribution(fakeHandle(() => live), 'text'))!;
      expect(authorsByRuns(cold.runs, live.text.length)).toEqual(truth);
    }
  });
});
