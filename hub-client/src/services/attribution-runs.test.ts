/**
 * Tests for the producer half of the attribution pipeline.
 *
 * Focused on the bits that are new for the implementation branch
 * (char→byte translation, payload shape). The run-list invariants
 * proper are covered exhaustively on `feat/node-attribution` and pinned
 * cross-implementation by `crates/quarto-core/tests/integration/attribution_types.rs`
 * (`query_byte_range` invariants — Phase 0 test #2).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';
import { encodeHeads } from '@automerge/automerge-repo';
import type { DocHandle } from '@automerge/automerge-repo';

import {
  buildCharToByteMap,
  buildRunListAttribution,
  runsCharToByteOffsets,
  updateRunListAttribution,
  type AttributionRun,
  type ViewableHandle,
} from './attribution-runs';

describe('buildCharToByteMap', () => {
  it('is the identity for ASCII text', () => {
    const map = buildCharToByteMap('hello world');
    expect(map.length).toBe(12); // 11 chars + terminator
    for (let i = 0; i <= 11; i++) expect(map[i]).toBe(i);
  });

  it('counts 2-byte UTF-8 sequences correctly', () => {
    // "é" is U+00E9, 2 bytes in UTF-8 (0xc3 0xa9).
    const map = buildCharToByteMap('aéb');
    // 'a' at char 0 → byte 0; 'é' at char 1 → byte 1; 'b' at char 2 → byte 3.
    expect(Array.from(map)).toEqual([0, 1, 3, 4]);
  });

  it('counts 3-byte UTF-8 sequences correctly (CJK)', () => {
    // "中" is U+4E2D, 3 bytes in UTF-8 (0xe4 0xb8 0xad).
    const map = buildCharToByteMap('a中b');
    expect(Array.from(map)).toEqual([0, 1, 4, 5]);
  });

  it('handles surrogate-pair (4-byte) codepoints', () => {
    // "𝕏" is U+1D54F, 4 bytes in UTF-8 — JS represents it as 2 UTF-16
    // code units (surrogate pair). Char 0 and char 1 are the two halves.
    const map = buildCharToByteMap('a𝕏b');
    expect(map.length).toBe(5);
    expect(map[0]).toBe(0); // 'a'
    expect(map[1]).toBe(1); // high surrogate of '𝕏'
    expect(map[2]).toBe(5); // low surrogate — past the 4-byte sequence
    expect(map[3]).toBe(5); // 'b'
    expect(map[4]).toBe(6); // EOS
  });
});

describe('runsCharToByteOffsets', () => {
  it('translates char-indexed runs through a non-ASCII map', () => {
    // Document text: "a中b" (3 chars, 5 bytes). One run spans the
    // whole text in char offsets [0..3).
    const sourceText = 'a中b';
    const map = buildCharToByteMap(sourceText);
    const runs: AttributionRun[] = [
      { start: 0, end: 3, actor: 'alice', time: 1 },
    ];
    const out = runsCharToByteOffsets(runs, map);
    expect(out).toEqual([{ start: 0, end: 5, actor: 'alice', time: 1 }]);
  });

  it('is the identity for ASCII inputs', () => {
    const sourceText = 'hello world';
    const map = buildCharToByteMap(sourceText);
    const runs: AttributionRun[] = [
      { start: 0, end: 5, actor: 'alice', time: 1 },
      { start: 6, end: 11, actor: 'bob', time: 2 },
    ];
    const out = runsCharToByteOffsets(runs, map);
    expect(out).toEqual(runs);
  });
});

// ---------------------------------------------------------------------------
// Incremental ≡ from-scratch invariant
// ---------------------------------------------------------------------------

// Anchor for the incremental path: whatever shortcut `updateRunListAttribution`
// uses to skip work, the final run list must agree character-for-character with
// what `buildRunListAttribution` produces from `init()` on the same final doc.
// If a future refactor breaks this — including via something subtle at the
// Automerge boundary (history-traversal ordering, getChanges semantics, etc.)
// — this is the test that should catch it.

interface TDoc { text: string }

function fakeHandle(doc: Doc<TDoc>): DocHandle<unknown> {
  const view: ViewableHandle = {
    history: () => A.topoHistoryTraversal(doc).map(h => encodeHeads([h])),
    metadata: () => undefined,
    doc: () => doc,
  };
  return view as unknown as DocHandle<unknown>;
}

describe('updateRunListAttribution invariant', () => {
  it('matches a from-scratch rebuild after a concurrent merge', async () => {
    const aliceActor = 'f'.repeat(32);
    const bobActor = '0'.repeat(32);

    let alice = A.from<TDoc>({ text: '' }, { actor: aliceActor });
    let bob = A.load<TDoc>(A.save(alice), { actor: bobActor });

    alice = A.change(alice, d => A.splice(d, ['text'], 0, 0, 'Hello'));
    alice = A.change(alice, d => A.splice(d, ['text'], 5, 0, ' World'));
    alice = A.change(alice, d => A.splice(d, ['text'], 11, 0, '!'));

    const stateBefore = await buildRunListAttribution(fakeHandle(alice), 'text');
    expect(stateBefore).toBeTruthy();

    bob = A.change(bob, d => A.splice(d, ['text'], 0, 0, 'X'));
    bob = A.change(bob, d => A.splice(d, ['text'], 1, 0, 'Y'));
    bob = A.change(bob, d => A.splice(d, ['text'], 2, 0, 'Z'));
    alice = A.merge(alice, bob);

    const incremental = updateRunListAttribution(stateBefore!, fakeHandle(alice), 'text');
    const fromScratch = await buildRunListAttribution(fakeHandle(alice), 'text');

    expect(fromScratch).toBeTruthy();
    expect(incremental.runs).toEqual(fromScratch!.runs);
  });

  it('matches a from-scratch rebuild across interleaved local and merged remote edits', async () => {
    const aliceActor = 'f'.repeat(32);
    const bobActor = '0'.repeat(32);

    let alice = A.from<TDoc>({ text: '' }, { actor: aliceActor });
    let bob = A.load<TDoc>(A.save(alice), { actor: bobActor });

    alice = A.change(alice, d => A.splice(d, ['text'], 0, 0, 'A1'));
    const stateBefore = await buildRunListAttribution(fakeHandle(alice), 'text');
    expect(stateBefore).toBeTruthy();

    bob = A.change(bob, d => A.splice(d, ['text'], 0, 0, 'B1'));
    bob = A.change(bob, d => A.splice(d, ['text'], 2, 0, 'B2'));
    alice = A.merge(alice, bob);
    alice = A.change(alice, d => A.splice(d, ['text'], 0, 0, 'A2'));
    alice = A.change(alice, d => A.splice(d, ['text'], 0, 0, 'A3'));

    const incremental = updateRunListAttribution(stateBefore!, fakeHandle(alice), 'text');
    const fromScratch = await buildRunListAttribution(fakeHandle(alice), 'text');

    expect(fromScratch).toBeTruthy();
    expect(incremental.runs).toEqual(fromScratch!.runs);
  });

  it('returns state unchanged when no new changes are present', async () => {
    const aliceActor = 'f'.repeat(32);
    let alice = A.from<TDoc>({ text: '' }, { actor: aliceActor });
    alice = A.change(alice, d => A.splice(d, ['text'], 0, 0, 'hello'));

    const stateBefore = await buildRunListAttribution(fakeHandle(alice), 'text');
    expect(stateBefore).toBeTruthy();

    // No edits between build and update — the incremental path should
    // short-circuit and return the same runs.
    const incremental = updateRunListAttribution(stateBefore!, fakeHandle(alice), 'text');
    expect(incremental.runs).toEqual(stateBefore!.runs);
  });
});
