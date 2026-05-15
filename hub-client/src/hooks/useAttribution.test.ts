/**
 * Unit tests for the pure half of `useAttribution.ts` —
 * `buildAttributionPayload` and the `buildIdentityMap` it delegates
 * to.
 *
 * The hook's stateful lifecycle (debounce, AbortController,
 * HistoryCompactedError recovery) is intentionally out of scope here.
 * Those paths live above the Automerge boundary and would need either
 * `vi.mock` of `attribution-runs` + `automergeSync` or a real
 * `@automerge/automerge-repo` fixture; see TODO follow-up.
 *
 * What we *do* pin:
 *
 *   * Char→byte translation through `buildCharToByteMap` for ASCII
 *     and multi-byte UTF-8 source text.
 *   * Per-actor identity resolution order: profile entry beats FNV
 *     fallback, fallback shape matches the `GitBlameProvider` formula
 *     used on the CLI side (`actor.slice(0, 8)` + `actorColor(...)`).
 *   * **Phase 6 producer invariant**: every actor that appears in
 *     `runs` has a matching entry in `identities` at the wire. If
 *     this regressed, the Rust writer would emit `<unknown>` /
 *     `#888888` placeholders in the rendered DOM (the warning-path
 *     fallback in `crates/pampa/src/writers/html.rs:747-750`).
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it } from 'vitest';

import { actorColor, fnv1aHex8 } from '../utils/palette';

import type { RunListAttribution } from '../services/attribution-runs';
import type { ActorIdentity } from '../services/automergeSync';

import { buildAttributionPayload } from './useAttribution';

interface ParsedPayload {
  runs: Array<{ start: number; end: number; actor: string; time: number }>;
  identities: Record<string, { name: string; color: string }>;
}

/** Convenience constructor for `RunListAttribution` — only `runs` is
 *  read by the function under test; the other fields are bookkeeping
 *  used by the live update path. */
function state(
  runs: Array<{ start: number; end: number; actor: string; time: number }>,
): RunListAttribution {
  return { runs, processedHeads: [], processedHistoryIndex: 0 };
}

function parse(json: string): ParsedPayload {
  return JSON.parse(json) as ParsedPayload;
}

describe('buildAttributionPayload', () => {
  it('returns a JSON string with the {runs, identities} shape', () => {
    const payload = parse(
      buildAttributionPayload(
        state([{ start: 0, end: 5, actor: 'alice', time: 1700000000 }]),
        'hello',
        {},
      ),
    );
    expect(payload).toHaveProperty('runs');
    expect(payload).toHaveProperty('identities');
    expect(Array.isArray(payload.runs)).toBe(true);
    expect(typeof payload.identities).toBe('object');
  });

  describe('char→byte translation', () => {
    it('is the identity for ASCII source text', () => {
      const payload = parse(
        buildAttributionPayload(
          state([
            { start: 0, end: 5, actor: 'alice', time: 1 },
            { start: 6, end: 11, actor: 'bob', time: 2 },
          ]),
          'hello world',
          {},
        ),
      );
      expect(payload.runs).toEqual([
        { start: 0, end: 5, actor: 'alice', time: 1 },
        { start: 6, end: 11, actor: 'bob', time: 2 },
      ]);
    });

    it('shifts run offsets according to multi-byte UTF-8 boundaries', () => {
      // "世" is 3 bytes (U+4E16, e4 b8 96). Source "a世b" is 5 bytes
      // (a, e4, b8, 96, b) but 3 chars. A char-range [1, 2) covering
      // the "世" should translate to byte-range [1, 4).
      const payload = parse(
        buildAttributionPayload(
          state([{ start: 1, end: 2, actor: 'alice', time: 1 }]),
          'a世b',
          {},
        ),
      );
      expect(payload.runs).toEqual([{ start: 1, end: 4, actor: 'alice', time: 1 }]);
    });
  });

  describe('identity resolution', () => {
    it('uses the profile entry when available', () => {
      const profile: Record<string, ActorIdentity> = {
        'alice@example.com': { name: 'Alice Cooper', color: '#c0392b' },
      };
      const payload = parse(
        buildAttributionPayload(
          state([{ start: 0, end: 1, actor: 'alice@example.com', time: 1 }]),
          'x',
          profile,
        ),
      );
      expect(payload.identities['alice@example.com']).toEqual({
        name: 'Alice Cooper',
        color: '#c0392b',
      });
    });

    it('falls back to (slice(0,8), actorColor(fnv1aHex8(actor))) when the profile is missing', () => {
      const actor = 'alice@example.com';
      const payload = parse(
        buildAttributionPayload(
          state([{ start: 0, end: 1, actor, time: 1 }]),
          'x',
          {}, // empty profile → fallback path
        ),
      );
      expect(payload.identities[actor]).toEqual({
        name: actor.slice(0, 8),
        color: actorColor(fnv1aHex8(actor)),
      });
    });

    it('mixes profile entries and fallbacks across distinct actors', () => {
      const profile: Record<string, ActorIdentity> = {
        known: { name: 'Known User', color: '#000000' },
      };
      const payload = parse(
        buildAttributionPayload(
          state([
            { start: 0, end: 1, actor: 'known', time: 1 },
            { start: 1, end: 2, actor: 'stranger', time: 2 },
          ]),
          'xy',
          profile,
        ),
      );
      expect(payload.identities.known).toEqual({ name: 'Known User', color: '#000000' });
      expect(payload.identities.stranger).toEqual({
        name: 'stranger', // slice(0,8) of an 8-char actor is the whole string
        color: actorColor(fnv1aHex8('stranger')),
      });
    });

    it('deduplicates identities when the same actor appears in multiple runs', () => {
      const payload = parse(
        buildAttributionPayload(
          state([
            { start: 0, end: 1, actor: 'alice', time: 1 },
            { start: 1, end: 2, actor: 'alice', time: 2 },
            { start: 2, end: 3, actor: 'alice', time: 3 },
          ]),
          'xyz',
          {},
        ),
      );
      // One entry per distinct actor, regardless of run count.
      expect(Object.keys(payload.identities)).toEqual(['alice']);
    });
  });

  it('satisfies the Phase 6 producer invariant: every run actor has an identity', () => {
    // A small population of mixed-known and unknown actors. The
    // contract is: forall r in runs, identities[r.actor] is defined.
    // If this breaks, the Rust writer falls through to the warning-
    // path placeholder (`<unknown>` / `#888888`) in html.rs:747-750.
    const profile: Record<string, ActorIdentity> = {
      profiled: { name: 'Profiled', color: '#abcdef' },
    };
    const payload = parse(
      buildAttributionPayload(
        state([
          { start: 0, end: 1, actor: 'profiled', time: 1 },
          { start: 1, end: 2, actor: 'unknown-a', time: 2 },
          { start: 2, end: 3, actor: 'unknown-b', time: 3 },
        ]),
        'xyz',
        profile,
      ),
    );
    for (const r of payload.runs) {
      expect(
        payload.identities,
        `actor ${r.actor} appears in runs but has no identity at the wire`,
      ).toHaveProperty(r.actor);
      expect(payload.identities[r.actor].name).toBeTruthy();
      expect(payload.identities[r.actor].color).toBeTruthy();
    }
  });
});
