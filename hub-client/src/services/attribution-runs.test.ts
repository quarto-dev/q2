/**
 * Tests for the producer half of the attribution pipeline.
 *
 * Focused on the bits that are new for the implementation branch
 * (char→byte translation, payload shape). The run-list invariants
 * proper are covered exhaustively on `feat/node-attribution` and pinned
 * cross-implementation by `crates/quarto-core/tests/attribution_types.rs`
 * (`query_byte_range` invariants — Phase 0 test #2).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect } from 'vitest';

import {
  buildCharToByteMap,
  runsCharToByteOffsets,
  type AttributionRun,
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
