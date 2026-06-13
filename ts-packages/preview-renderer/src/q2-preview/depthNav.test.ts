/**
 * Tests for depthNav.ts — pure depth-cursor navigation utilities (Plan P3.3).
 *
 * All functions are pure (no DOM, no React). Default vitest environment (node).
 */

import { describe, it, expect } from 'vitest';
import {
  parseSiKey,
  buildDepthSurfaces,
  parentSurface,
  childSurfaceToward,
  classifyDepthKey,
  detectPlatform,
  buildDepthCommitDestination,
  type DepthSurface,
} from './depthNav';

// ── Fixture ───────────────────────────────────────────────────────────────────
//
// Nesting layout:
//   T:  r0=0   r1=6     (top-level, sibling of BQ)
//   BQ: r0=6   r1=40    (contains D, P1, P2)
//   D:  r0=8   r1=39    (inside BQ; contains P1, P2)
//   P1: r0=10  r1=22    (leaf)
//   P2: r0=24  r1=38    (leaf)

const T_KEY  = '0:0-6:0';
const BQ_KEY = '0:6-40:0';
const D_KEY  = '0:8-39:0';
const P1_KEY = '0:10-22:0';
const P2_KEY = '0:24-38:0';

function makeIndex(
  entries: Array<[string, 'TopLevel' | 'Descendable' | 'Opaque']>,
): Map<string, { reachabilityClass: string }> {
  return new Map(entries.map(([k, rc]) => [k, { reachabilityClass: rc }]));
}

const fixtureIndex = makeIndex([
  [T_KEY,  'TopLevel'],
  [BQ_KEY, 'Descendable'],
  [D_KEY,  'Descendable'],
  [P1_KEY, 'Descendable'],
  [P2_KEY, 'Descendable'],
]);

// Convenience surfaces that match the fixture
const sT:  DepthSurface = { r0: 0,  r1: 6  };
const sBQ: DepthSurface = { r0: 6,  r1: 40 };
const sD:  DepthSurface = { r0: 8,  r1: 39 };
const sP1: DepthSurface = { r0: 10, r1: 22 };
const sP2: DepthSurface = { r0: 24, r1: 38 };

// ── parseSiKey ────────────────────────────────────────────────────────────────

describe('parseSiKey', () => {
  it('parses a well-formed key', () => {
    expect(parseSiKey('0:42-87:0')).toEqual({ t: 0, r0: 42, r1: 87, d: 0 });
  });

  it('parses a key with non-zero t and d', () => {
    expect(parseSiKey('3:100-200:5')).toEqual({ t: 3, r0: 100, r1: 200, d: 5 });
  });

  it('returns null for "garbage"', () => {
    expect(parseSiKey('garbage')).toBeNull();
  });

  it('returns null for empty string', () => {
    expect(parseSiKey('')).toBeNull();
  });

  it('returns null for "0:1:0" (missing r1)', () => {
    expect(parseSiKey('0:1:0')).toBeNull();
  });

  it('returns null for a key with no colon separators', () => {
    expect(parseSiKey('012345')).toBeNull();
  });
});

// ── buildDepthSurfaces ────────────────────────────────────────────────────────

describe('buildDepthSurfaces', () => {
  it('builds surfaces from fixture index, excluding no Opaque entries', () => {
    const surfaces = buildDepthSurfaces(fixtureIndex);
    // All five surfaces should be present
    expect(surfaces).toHaveLength(5);
  });

  it('sorts by r0 ascending, then r1 descending (outer before inner)', () => {
    const surfaces = buildDepthSurfaces(fixtureIndex);
    // r0 order: 0, 6, 8, 10, 24
    expect(surfaces[0]).toEqual(sT);
    expect(surfaces[1]).toEqual(sBQ);
    expect(surfaces[2]).toEqual(sD);
    expect(surfaces[3]).toEqual(sP1);
    expect(surfaces[4]).toEqual(sP2);
  });

  it('excludes Opaque entries', () => {
    const idx = makeIndex([
      [T_KEY,  'TopLevel'],
      ['0:50-60:0', 'Opaque'],
      [BQ_KEY, 'Descendable'],
    ]);
    const surfaces = buildDepthSurfaces(idx);
    expect(surfaces.some(s => s.r0 === 50)).toBe(false);
    expect(surfaces).toHaveLength(2);
  });

  it('skips entries with malformed keys', () => {
    const idx = makeIndex([
      [T_KEY, 'TopLevel'],
      ['bad-key', 'TopLevel'],
    ]);
    const surfaces = buildDepthSurfaces(idx);
    expect(surfaces).toHaveLength(1);
    expect(surfaces[0]).toEqual(sT);
  });

  it('dedupes surfaces with identical (r0, r1)', () => {
    const idx = makeIndex([
      [T_KEY, 'TopLevel'],
      ['1:0-6:0', 'TopLevel'], // same r0=0, r1=6, different t
    ]);
    const surfaces = buildDepthSurfaces(idx);
    expect(surfaces).toHaveLength(1);
    expect(surfaces[0]).toEqual(sT);
  });

  it('returns [] for null input', () => {
    expect(buildDepthSurfaces(null)).toEqual([]);
  });

  it('returns [] for undefined input', () => {
    expect(buildDepthSurfaces(undefined)).toEqual([]);
  });

  it('outer before inner at same r0 (r1 descending)', () => {
    // Two surfaces with same r0, different r1: outer (larger r1) first
    const idx = makeIndex([
      ['0:5-20:0', 'TopLevel'],
      ['0:5-15:0', 'TopLevel'],
    ]);
    const surfaces = buildDepthSurfaces(idx);
    expect(surfaces[0]).toEqual({ r0: 5, r1: 20 });
    expect(surfaces[1]).toEqual({ r0: 5, r1: 15 });
  });
});

// ── parentSurface ─────────────────────────────────────────────────────────────

describe('parentSurface', () => {
  let surfaces: DepthSurface[];

  beforeAll(() => {
    surfaces = buildDepthSurfaces(fixtureIndex);
  });

  it('cursor=P1 → parent=D (tightest, not BQ)', () => {
    const result = parentSurface(surfaces, sP1.r0, sP1.r1);
    expect(result).toEqual(sD);
  });

  it('cursor=D → parent=BQ', () => {
    const result = parentSurface(surfaces, sD.r0, sD.r1);
    expect(result).toEqual(sBQ);
  });

  it('cursor=BQ → null (T does not contain it)', () => {
    const result = parentSurface(surfaces, sBQ.r0, sBQ.r1);
    expect(result).toBeNull();
  });

  it('cursor=T → null (already outermost)', () => {
    const result = parentSurface(surfaces, sT.r0, sT.r1);
    expect(result).toBeNull();
  });

  it('cursor=P2 → parent=D', () => {
    const result = parentSurface(surfaces, sP2.r0, sP2.r1);
    expect(result).toEqual(sD);
  });
});

// ── childSurfaceToward ────────────────────────────────────────────────────────

describe('childSurfaceToward', () => {
  let surfaces: DepthSurface[];

  beforeAll(() => {
    surfaces = buildDepthSurfaces(fixtureIndex);
  });

  it('cursor=BQ, leafAnchorR0=P2.r0(24) → child=D (direct child, one step)', () => {
    const result = childSurfaceToward(surfaces, sBQ.r0, sBQ.r1, sP2.r0);
    expect(result).toEqual(sD);
  });

  it('cursor=D, leafAnchorR0=24 → child=P2', () => {
    const result = childSurfaceToward(surfaces, sD.r0, sD.r1, 24);
    expect(result).toEqual(sP2);
  });

  it('cursor=D, leafAnchorR0=10 → child=P1', () => {
    const result = childSurfaceToward(surfaces, sD.r0, sD.r1, 10);
    expect(result).toEqual(sP1);
  });

  it('cursor=P1 (leaf) → null', () => {
    const result = childSurfaceToward(surfaces, sP1.r0, sP1.r1, sP1.r0);
    expect(result).toBeNull();
  });

  it('leaf-deleted fallback: cursor=D, leafAnchorR0=9999 → first direct child=P1', () => {
    const result = childSurfaceToward(surfaces, sD.r0, sD.r1, 9999);
    expect(result).toEqual(sP1);
  });

  it('cursor=BQ, leafAnchorR0=BQ.r0+1 (not inside any child) → fallback = D (first direct child of BQ)', () => {
    // BQ contains D; leafAnchorR0 = 7 is inside BQ but NOT inside D (r0=8).
    // So fall back to first direct child = D.
    const result = childSurfaceToward(surfaces, sBQ.r0, sBQ.r1, 7);
    expect(result).toEqual(sD);
  });
});

// ── classifyDepthKey ──────────────────────────────────────────────────────────

describe('classifyDepthKey', () => {
  function ev(
    key: string,
    meta: boolean,
    ctrl: boolean,
    alt: boolean,
    shift: boolean,
  ) {
    return { key, metaKey: meta, ctrlKey: ctrl, altKey: alt, shiftKey: shift };
  }

  // mac: metaKey && ctrlKey && !altKey && !shiftKey
  it('mac Cmd+Ctrl+ArrowLeft → out', () => {
    expect(classifyDepthKey(ev('ArrowLeft', true, true, false, false), 'mac')).toBe('out');
  });

  it('mac Cmd+Ctrl+ArrowRight → in', () => {
    expect(classifyDepthKey(ev('ArrowRight', true, true, false, false), 'mac')).toBe('in');
  });

  it('mac Cmd+Ctrl+non-arrow → null', () => {
    expect(classifyDepthKey(ev('ArrowUp', true, true, false, false), 'mac')).toBeNull();
  });

  it('mac bare ArrowLeft → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, false, false), 'mac')).toBeNull();
  });

  it('mac Alt+Shift+ArrowLeft (wrong platform chord on mac) → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, true, true), 'mac')).toBeNull();
  });

  it('mac Shift+ArrowLeft → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, false, true), 'mac')).toBeNull();
  });

  // other: altKey && shiftKey && !metaKey && !ctrlKey
  it('other Alt+Shift+ArrowLeft → out', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, true, true), 'other')).toBe('out');
  });

  it('other Alt+Shift+ArrowRight → in', () => {
    expect(classifyDepthKey(ev('ArrowRight', false, false, true, true), 'other')).toBe('in');
  });

  it('other Alt+Shift+non-arrow → null', () => {
    expect(classifyDepthKey(ev('ArrowDown', false, false, true, true), 'other')).toBeNull();
  });

  it('other bare ArrowLeft → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, false, false), 'other')).toBeNull();
  });

  it('other Cmd+Ctrl+ArrowLeft (mac chord on other) → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', true, true, false, false), 'other')).toBeNull();
  });

  it('other Shift+ArrowLeft → null', () => {
    expect(classifyDepthKey(ev('ArrowLeft', false, false, false, true), 'other')).toBeNull();
  });
});

// ── detectPlatform ────────────────────────────────────────────────────────────

describe('detectPlatform', () => {
  it('{platform:"MacIntel"} → mac', () => {
    expect(detectPlatform({ platform: 'MacIntel' })).toBe('mac');
  });

  it('{userAgent:"... Macintosh ..."} → mac', () => {
    expect(detectPlatform({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' })).toBe('mac');
  });

  it('{platform:"Win32"} → other', () => {
    expect(detectPlatform({ platform: 'Win32' })).toBe('other');
  });

  it('{platform:"Linux x86_64"} → other', () => {
    expect(detectPlatform({ platform: 'Linux x86_64' })).toBe('other');
  });

  it('{} (no fields) → other', () => {
    expect(detectPlatform({})).toBe('other');
  });
});

// ── buildDepthCommitDestination ───────────────────────────────────────────────

describe('buildDepthCommitDestination', () => {
  it('null input → null', () => {
    expect(buildDepthCommitDestination(null)).toBeNull();
  });

  it('undefined input → null', () => {
    expect(buildDepthCommitDestination(undefined)).toBeNull();
  });

  it('{anchorR0:6, anchorR1:40} → JSON with t=0, r=[6,40], d=0', () => {
    const result = buildDepthCommitDestination({ anchorR0: 6, anchorR1: 40 });
    expect(result).not.toBeNull();
    const parsed = JSON.parse(result!);
    expect(parsed).toEqual({ t: 0, r: [6, 40], d: 0 });
  });

  it('{anchorR0:0, anchorR1:0} → valid JSON', () => {
    const result = buildDepthCommitDestination({ anchorR0: 0, anchorR1: 0 });
    expect(result).not.toBeNull();
    const parsed = JSON.parse(result!);
    expect(parsed).toEqual({ t: 0, r: [0, 0], d: 0 });
  });
});

// ── Need to import beforeAll ──────────────────────────────────────────────────
import { beforeAll } from 'vitest';
