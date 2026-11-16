/**
 * Live updates from the doc's own patches.
 *
 * Applying a change to the work doc costs time proportional to the document,
 * so an update whose new changes share one attribution reads their patches
 * from the live doc and leaves the work doc behind. These tests pin that the
 * runs stay those of a full build, and that the work doc is caught up before
 * anything replays on it.
 */

import { describe, expect, it } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';

import {
  buildRunListAttribution,
  createReplayIndex,
  HistoryCompactedError,
  updateRunListAttribution,
} from './attribution-runs';
import type { AttributionRun, RunListAttribution } from './attribution-runs';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);

function fakeHandle(doc: Doc<TDoc>): DocHandle<unknown> {
  return { doc: () => doc } as unknown as DocHandle<unknown>;
}

function insert(doc: Doc<TDoc>, at: number, value: string, time: number): Doc<TDoc> {
  return A.change(doc, { time }, d => A.splice(d, ['text'], at, 0, value));
}

async function build(doc: Doc<TDoc>): Promise<RunListAttribution> {
  return (await buildRunListAttribution(fakeHandle(doc), 'text'))!;
}

async function coldRuns(doc: Doc<TDoc>): Promise<AttributionRun[]> {
  return (await build(A.clone(doc))).runs;
}

function workHeads(state: RunListAttribution): string[] {
  return A.getHeads(state._workDoc as Doc<TDoc>);
}

describe('live updates from the doc patches', () => {
  it('folds an edit without applying it to the work doc', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha beta', 1);
    const built = await build(doc);
    const before = workHeads(built);

    doc = insert(doc, 5, ' NEW', 2);
    const next = updateRunListAttribution(built, fakeHandle(doc), 'text');

    expect(next.runs).toEqual(await coldRuns(doc));
    expect(workHeads(next)).toEqual(before);
  });

  it('follows a run of edits, deletions included', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha beta gamma', 1);
    let state = await build(doc);

    for (let i = 0; i < 6; i++) {
      doc = i % 3 === 2
        ? A.change(doc, { time: 10 + i }, d => A.splice(d, ['text'], 2, 3))
        : insert(doc, (i * 5) % doc.text.length, `<${i}>`, 10 + i);
      state = updateRunListAttribution(state, fakeHandle(doc), 'text');
      expect(state.runs).toEqual(await coldRuns(doc));
    }
  });

  it('folds several pending changes of one author and second', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha', 1);
    const built = await build(doc);
    doc = insert(doc, 5, ' b', 7);
    doc = insert(doc, 0, 'c ', 7);

    const next = updateRunListAttribution(built, fakeHandle(doc), 'text');
    expect(next.runs).toEqual(await coldRuns(doc));
  });

  it('keeps the state object when nothing is pending', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha', 1);
    const built = await build(doc);
    doc = insert(doc, 5, ' beta', 2);
    const next = updateRunListAttribution(built, fakeHandle(doc), 'text');

    expect(updateRunListAttribution(next, fakeHandle(doc), 'text')).toBe(next);
  });

  it('does not change the state it was given', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha', 1);
    const built = await build(doc);
    const runsBefore = structuredClone(built.runs);
    doc = insert(doc, 0, 'x', 2);
    updateRunListAttribution(built, fakeHandle(doc), 'text');

    expect(built.runs).toEqual(runsBefore);
  });

  it('catches the work doc up when changes of two authors arrive together', async () => {
    let alice = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha beta', 1);
    let state = await build(alice);

    // Edits that leave the work doc behind.
    alice = insert(alice, 0, 'one ', 2);
    state = updateRunListAttribution(state, fakeHandle(alice), 'text');
    alice = insert(alice, 0, 'two ', 3);
    state = updateRunListAttribution(state, fakeHandle(alice), 'text');

    // Bob's concurrent edit and one more of Alice's land in one update.
    let bob = A.clone(alice, { actor: BOB });
    bob = insert(bob, bob.text.length, ' bob', 4);
    alice = insert(alice, 4, 'three ', 5);
    alice = A.merge(alice, bob);

    state = updateRunListAttribution(state, fakeHandle(alice), 'text');
    expect(state.runs).toEqual(await coldRuns(alice));
    expect(workHeads(state)).toEqual(A.getHeads(alice));

    // And single edits fold again from there.
    alice = insert(alice, 1, '!', 6);
    state = updateRunListAttribution(state, fakeHandle(alice), 'text');
    expect(state.runs).toEqual(await coldRuns(alice));
  });

  it('catches the work doc up before a replay step', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha', 1);
    let state = await build(doc);
    doc = insert(doc, 5, ' beta', 2);
    state = updateRunListAttribution(state, fakeHandle(doc), 'text');
    const stepHeads = A.getHeads(doc);
    const atStep = A.clone(doc);

    // The live doc moves on; replay asks for the step the state is at.
    doc = insert(doc, 0, 'later ', 3);
    const target = { heads: stepHeads, index: createReplayIndex() };
    const next = updateRunListAttribution(state, fakeHandle(doc), 'text', target);

    expect(next.runs).toEqual(await coldRuns(atStep));
    expect(workHeads(next)).toEqual(stepHeads);
  });

  it('asks for a rebuild when the doc lacks the changes the runs describe', async () => {
    let doc = insert(A.from<TDoc>({ text: '' }, { actor: ALICE }), 0, 'alpha', 1);
    let state = await build(doc);
    doc = insert(doc, 5, ' beta', 2);
    state = updateRunListAttribution(state, fakeHandle(doc), 'text');

    const other = insert(A.from<TDoc>({ text: '' }, { actor: BOB }), 0, 'unrelated', 1);
    expect(() => updateRunListAttribution(state, fakeHandle(other), 'text'))
      .toThrow(HistoryCompactedError);
  });
});
