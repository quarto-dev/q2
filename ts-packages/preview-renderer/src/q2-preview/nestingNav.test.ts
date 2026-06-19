/**
 * Tests for nestingNav.ts — pure nesting-cursor navigation utilities (Plan P3.3).
 *
 * All functions are pure (no DOM, no React). Default vitest environment (node).
 */

import { describe, it, expect } from 'vitest';
import {
  parseSiKey,
  buildNestingSurfaces,
  parentSurface,
  topBlockR0,
  childSurfaceToward,
  childSurfaceTowardLine,
  surfaceLineSpan,
  classifyNestingKey,
  detectPlatform,
  buildNestingCommitDestination,
  depthOfSurface,
  relocateSurface,
  type NestingSurface,
} from './nestingNav';
import { buildByteLineMap } from '../utils/byteLineMap';

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
const sT:  NestingSurface = { r0: 0,  r1: 6  };
const sBQ: NestingSurface = { r0: 6,  r1: 40 };
const sD:  NestingSurface = { r0: 8,  r1: 39 };
const sP1: NestingSurface = { r0: 10, r1: 22 };
const sP2: NestingSurface = { r0: 24, r1: 38 };

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

// ── buildNestingSurfaces ────────────────────────────────────────────────────────

describe('buildNestingSurfaces', () => {
  it('builds surfaces from fixture index, excluding no Opaque entries', () => {
    const surfaces = buildNestingSurfaces(fixtureIndex);
    // All five surfaces should be present
    expect(surfaces).toHaveLength(5);
  });

  it('sorts by r0 ascending, then r1 descending (outer before inner)', () => {
    const surfaces = buildNestingSurfaces(fixtureIndex);
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
    const surfaces = buildNestingSurfaces(idx);
    expect(surfaces.some(s => s.r0 === 50)).toBe(false);
    expect(surfaces).toHaveLength(2);
  });

  it('skips entries with malformed keys', () => {
    const idx = makeIndex([
      [T_KEY, 'TopLevel'],
      ['bad-key', 'TopLevel'],
    ]);
    const surfaces = buildNestingSurfaces(idx);
    expect(surfaces).toHaveLength(1);
    expect(surfaces[0]).toEqual(sT);
  });

  it('dedupes surfaces with identical (r0, r1)', () => {
    const idx = makeIndex([
      [T_KEY, 'TopLevel'],
      ['1:0-6:0', 'TopLevel'], // same r0=0, r1=6, different t
    ]);
    const surfaces = buildNestingSurfaces(idx);
    expect(surfaces).toHaveLength(1);
    expect(surfaces[0]).toEqual(sT);
  });

  it('returns [] for null input', () => {
    expect(buildNestingSurfaces(null)).toEqual([]);
  });

  it('returns [] for undefined input', () => {
    expect(buildNestingSurfaces(undefined)).toEqual([]);
  });

  it('outer before inner at same r0 (r1 descending)', () => {
    // Two surfaces with same r0, different r1: outer (larger r1) first
    const idx = makeIndex([
      ['0:5-20:0', 'TopLevel'],
      ['0:5-15:0', 'TopLevel'],
    ]);
    const surfaces = buildNestingSurfaces(idx);
    expect(surfaces[0]).toEqual({ r0: 5, r1: 20 });
    expect(surfaces[1]).toEqual({ r0: 5, r1: 15 });
  });
});

// ── parentSurface ─────────────────────────────────────────────────────────────

describe('parentSurface', () => {
  let surfaces: NestingSurface[];

  beforeAll(() => {
    surfaces = buildNestingSurfaces(fixtureIndex);
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

// ── topBlockR0 ────────────────────────────────────────────────────────────────

describe('topBlockR0', () => {
  let surfaces: NestingSurface[];

  beforeAll(() => {
    surfaces = buildNestingSurfaces(fixtureIndex);
  });

  it('leaf P1 climbs P1→D→BQ → outermost r0 = BQ.r0 (6)', () => {
    // Multi-level climb: P1 ⊂ D ⊂ BQ; BQ has no parent (T does not contain it).
    expect(topBlockR0(surfaces, sP1.r0, sP1.r1)).toBe(sBQ.r0);
  });

  it('leaf P2 climbs P2→D→BQ → outermost r0 = BQ.r0 (6)', () => {
    expect(topBlockR0(surfaces, sP2.r0, sP2.r1)).toBe(sBQ.r0);
  });

  it('mid-level D climbs D→BQ → outermost r0 = BQ.r0 (6)', () => {
    expect(topBlockR0(surfaces, sD.r0, sD.r1)).toBe(sBQ.r0);
  });

  it('already-outermost BQ → its own r0 (6) [last non-null, not null]', () => {
    expect(topBlockR0(surfaces, sBQ.r0, sBQ.r1)).toBe(sBQ.r0);
  });

  it('top-level T (no parent at all) → its own r0 (0)', () => {
    expect(topBlockR0(surfaces, sT.r0, sT.r1)).toBe(sT.r0);
  });
});

// ── childSurfaceToward ────────────────────────────────────────────────────────

describe('childSurfaceToward', () => {
  let surfaces: NestingSurface[];

  beforeAll(() => {
    surfaces = buildNestingSurfaces(fixtureIndex);
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

// ── classifyNestingKey ──────────────────────────────────────────────────────────

describe('classifyNestingKey', () => {
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
    expect(classifyNestingKey(ev('ArrowLeft', true, true, false, false), 'mac')).toBe('out');
  });

  it('mac Cmd+Ctrl+ArrowRight → in', () => {
    expect(classifyNestingKey(ev('ArrowRight', true, true, false, false), 'mac')).toBe('in');
  });

  it('mac Cmd+Ctrl+non-arrow → null', () => {
    expect(classifyNestingKey(ev('ArrowUp', true, true, false, false), 'mac')).toBeNull();
  });

  it('mac bare ArrowLeft → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, false, false), 'mac')).toBeNull();
  });

  it('mac Alt+Shift+ArrowLeft (wrong platform chord on mac) → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, true, true), 'mac')).toBeNull();
  });

  it('mac Shift+ArrowLeft → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, false, true), 'mac')).toBeNull();
  });

  // other: altKey && shiftKey && !metaKey && !ctrlKey
  it('other Alt+Shift+ArrowLeft → out', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, true, true), 'other')).toBe('out');
  });

  it('other Alt+Shift+ArrowRight → in', () => {
    expect(classifyNestingKey(ev('ArrowRight', false, false, true, true), 'other')).toBe('in');
  });

  it('other Alt+Shift+non-arrow → null', () => {
    expect(classifyNestingKey(ev('ArrowDown', false, false, true, true), 'other')).toBeNull();
  });

  it('other bare ArrowLeft → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, false, false), 'other')).toBeNull();
  });

  it('other Cmd+Ctrl+ArrowLeft (mac chord on other) → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', true, true, false, false), 'other')).toBeNull();
  });

  it('other Shift+ArrowLeft → null', () => {
    expect(classifyNestingKey(ev('ArrowLeft', false, false, false, true), 'other')).toBeNull();
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

// ── buildNestingCommitDestination ───────────────────────────────────────────────

describe('buildNestingCommitDestination', () => {
  it('null input → null', () => {
    expect(buildNestingCommitDestination(null)).toBeNull();
  });

  it('undefined input → null', () => {
    expect(buildNestingCommitDestination(undefined)).toBeNull();
  });

  it('{anchorR0:6, anchorR1:40} → JSON with t=0, r=[6,40], d=0', () => {
    const result = buildNestingCommitDestination({ anchorR0: 6, anchorR1: 40 });
    expect(result).not.toBeNull();
    const parsed = JSON.parse(result!);
    expect(parsed).toEqual({ t: 0, r: [6, 40], d: 0 });
  });

  it('{anchorR0:0, anchorR1:0} → valid JSON', () => {
    const result = buildNestingCommitDestination({ anchorR0: 0, anchorR1: 0 });
    expect(result).not.toBeNull();
    const parsed = JSON.parse(result!);
    expect(parsed).toEqual({ t: 0, r: [0, 0], d: 0 });
  });
});

// ── Need to import beforeAll ──────────────────────────────────────────────────
import { beforeAll } from 'vitest';

// ── buildAncestorPath / labelForSourceNode ────────────────────────────────────

import { buildSourceIndex } from './sourceIndex';
import { buildAncestorPath, labelForSourceNode } from './nestingNav';

// Fixture AST: Div#sec ⊃ BlockQuote ⊃ Para
// Pool ranges are strictly nested: Div[0,40] ⊃ BlockQuote[10,38] ⊃ Para[12,36]
const ANCESTOR_AST = {
  'pandoc-api-version': [1, 23, 0],
  meta: {},
  blocks: [
    {
      t: 'Div',
      c: [['sec', [], []], [   // Attr id="sec"
        {
          t: 'BlockQuote',
          c: [
            { t: 'Para', c: [{ t: 'Str', c: 'hello' }], s: 2 },
          ],
          s: 1,
        },
      ]],
      s: 0,
    },
  ],
  astContext: {
    p: [
      { t: 0, r: [0, 40], d: 0 },   // pool[0] Div         siKey 0:0-40:0
      { t: 0, r: [10, 38], d: 0 },  // pool[1] BlockQuote  siKey 0:10-38:0
      { t: 0, r: [12, 36], d: 0 },  // pool[2] Para        siKey 0:12-36:0
    ],
  },
};
const SI = buildSourceIndex(JSON.stringify(ANCESTOR_AST))!;

describe('buildAncestorPath', () => {
  it('cursor=Para(12,36) → full path [Div#sec, BlockQuote, Para]', () => {
    expect(buildAncestorPath(SI, 12, 36)).toEqual([
      { label: 'Div#sec',    r0: 0,  r1: 40, isCurrent: false },
      { label: 'BlockQuote', r0: 10, r1: 38, isCurrent: false },
      { label: 'Para',       r0: 12, r1: 36, isCurrent: true  },
    ]);
  });

  it('cursor=Div(0,40) (outermost) → single crumb [Div#sec]', () => {
    expect(buildAncestorPath(SI, 0, 40)).toEqual([
      { label: 'Div#sec', r0: 0, r1: 40, isCurrent: true },
    ]);
  });

  it('cursor=BlockQuote(10,38) → [Div#sec(false), BlockQuote(true)]', () => {
    expect(buildAncestorPath(SI, 10, 38)).toEqual([
      { label: 'Div#sec',    r0: 0,  r1: 40, isCurrent: false },
      { label: 'BlockQuote', r0: 10, r1: 38, isCurrent: true  },
    ]);
  });

  it('null sourceIndex → []', () => {
    expect(buildAncestorPath(null, 12, 36)).toEqual([]);
  });

  it('undefined sourceIndex → []', () => {
    expect(buildAncestorPath(undefined, 12, 36)).toEqual([]);
  });
});

describe('labelForSourceNode', () => {
  it('Para with no Attr slot → "Para"', () => {
    expect(labelForSourceNode({ t: 'Para', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('Para');
  });

  it('Div with id="myid" and class → id wins, returns "Div#myid"', () => {
    expect(labelForSourceNode({ t: 'Div', c: [['myid', ['note'], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('Div#myid');
  });

  it('Div with no id but classes → first class, returns "Div.note"', () => {
    expect(labelForSourceNode({ t: 'Div', c: [['', ['note', 'tip'], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('Div.note');
  });

  it('Div with empty Attr (no id, no classes) → "Div"', () => {
    expect(labelForSourceNode({ t: 'Div', c: [['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('Div');
  });

  it('Header with Attr at c[1] with id → "Header#h-id"', () => {
    expect(labelForSourceNode({ t: 'Header', c: [2, ['h-id', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('Header#h-id');
  });

  it('BlockQuote with no Attr slot → "BlockQuote"', () => {
    expect(labelForSourceNode({ t: 'BlockQuote', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('BlockQuote');
  });
});

// ── §2 caret-aware nest-in: surfaceLineSpan + childSurfaceTowardLine ────────────
//
// Ground-truth fixture (parsed with the real binary, `pampa -t json`, then run
// through the real buildSourceIndex → buildNestingSurfaces; see plan §Verification
// fixture, 2026-06-15 re-verified). 3-level bullet list:
//
//   * another                 line 0  bytes 0-9
//   * hello                    line 1  bytes 10-17
//       * sub-item             line 2  bytes 18-32
//           * sub-sub-item     line 3  bytes 33-55
//       * nother               line 4  bytes 56-68
//
// Surfaces (r0,r1) with reachability, and their RAW vs TRIMMED line spans:
//   [0,69]   top BulletList      raw[0,4] trim[0,4]
//   [2,10]   "another"           raw[0,0] trim[0,0]
//   [12,20]  "hello"             raw[1,2] trim[1,1]   ← raw absorbs next line's indent
//   [20,69]  level-1 BulletList  raw[2,4] trim[2,4]
//   [24,39]  "sub-item"          raw[2,3] trim[2,2]
//   [39,60]  level-2 BulletList  raw[3,4] trim[3,3]   ← raw end byte 60 reaches into the nother line
//   [43,60]  "sub-sub-item"      raw[3,4] trim[3,3]
//   [62,69]  "nother"            raw[4,4] trim[4,4]
//
// The raw-span overlaps ([12,20] over line 2; [39,60] over line 4) are exactly
// the sibling-overlap hazard (Reflection #17): childSurfaceTowardLine must use
// the TRIMMED span so the caret on the nother line (4) descends to "nother"
// [62,69], not the sub-sub-item list [39,60].

const NEST3_CONTENT =
  '* another\n* hello\n    * sub-item\n        * sub-sub-item\n    * nother\n';

const nest3Index = makeIndex([
  ['0:0-69:0',  'TopLevel'],
  ['0:2-10:0',  'Descendable'],
  ['0:12-20:0', 'Descendable'],
  ['0:20-69:0', 'Descendable'],
  ['0:24-39:0', 'Descendable'],
  ['0:39-60:0', 'Descendable'],
  ['0:43-60:0', 'Descendable'],
  ['0:62-69:0', 'Descendable'],
]);

describe('surfaceLineSpan (trimmed content range, Reflection #17)', () => {
  const map = buildByteLineMap(NEST3_CONTENT);

  it('trims the trailing-whitespace overflow so the sub-sub-item list spans [3,3], not raw [3,4]', () => {
    expect(surfaceLineSpan({ r0: 39, r1: 60 }, NEST3_CONTENT, map)).toEqual([3, 3]);
  });

  it('trims the hello item to [1,1], not raw [1,2]', () => {
    expect(surfaceLineSpan({ r0: 12, r1: 20 }, NEST3_CONTENT, map)).toEqual([1, 1]);
  });

  it('single-line leaf surfaces span one line', () => {
    expect(surfaceLineSpan({ r0: 2, r1: 10 }, NEST3_CONTENT, map)).toEqual([0, 0]);
    expect(surfaceLineSpan({ r0: 24, r1: 39 }, NEST3_CONTENT, map)).toEqual([2, 2]);
    expect(surfaceLineSpan({ r0: 62, r1: 69 }, NEST3_CONTENT, map)).toEqual([4, 4]);
  });

  it('multi-line container surfaces span their full trimmed range', () => {
    expect(surfaceLineSpan({ r0: 0, r1: 69 }, NEST3_CONTENT, map)).toEqual([0, 4]);
    expect(surfaceLineSpan({ r0: 20, r1: 69 }, NEST3_CONTENT, map)).toEqual([2, 4]);
  });

  it('uses byte-correct slicing (non-ASCII before the surface does not shift the span)', () => {
    // "é" is 2 UTF-8 bytes but 1 UTF-16 unit. Para "x" lives at bytes [3,4].
    //   bytes: 0xC3 0xA9 '\n' 'x' '\n'  → "é\nx\n"
    // A UTF-16 .slice(3,4) would read "\n" (wrong line); a byte slice reads "x".
    const content = 'é\nx\n';
    const m = buildByteLineMap(content);
    // line 0 = "é" (bytes 0-1), line 1 = "x" (byte 3)
    expect(surfaceLineSpan({ r0: 3, r1: 4 }, content, m)).toEqual([1, 1]);
  });
});

describe('childSurfaceTowardLine (caret-aware descent, Reflection #17)', () => {
  const surfaces = buildNestingSurfaces(nest3Index);
  const map = buildByteLineMap(NEST3_CONTENT);
  const at = (s: NestingSurface | null) => (s ? [s.r0, s.r1] : null);

  it('descends from the level-1 list toward the nother line (4) → "nother" [62,69], NOT the sub-sub-item list [39,60]', () => {
    // Pins caret-on-line-4 → nother. (The trimmed span itself is fail-on-revert-
    // proven by the surfaceLineSpan tests above; here the start-line==Ls tiebreak
    // also picks [62,69] even under raw spans, so this is a descent-correctness
    // test, not the trim lever.)
    expect(at(childSurfaceTowardLine(surfaces, 20, 69, 4, map, NEST3_CONTENT))).toEqual([62, 69]);
  });

  it('descends from the level-1 list toward sub-item (line 2) → "sub-item" [24,39]', () => {
    expect(at(childSurfaceTowardLine(surfaces, 20, 69, 2, map, NEST3_CONTENT))).toEqual([24, 39]);
  });

  it('descends from the level-1 list toward line 3 → the sub-sub-item LIST [39,60] (direct child, not the grandchild item)', () => {
    expect(at(childSurfaceTowardLine(surfaces, 20, 69, 3, map, NEST3_CONTENT))).toEqual([39, 60]);
  });

  it('descends from the top list toward a deep line (2) into the level-1 list [20,69]', () => {
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 2, map, NEST3_CONTENT))).toEqual([20, 69]);
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 3, map, NEST3_CONTENT))).toEqual([20, 69]);
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 4, map, NEST3_CONTENT))).toEqual([20, 69]);
  });

  it('descends from the top list toward each top-level item line', () => {
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 0, map, NEST3_CONTENT))).toEqual([2, 10]);
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 1, map, NEST3_CONTENT))).toEqual([12, 20]);
  });

  it('returns null at a leaf (no contained children)', () => {
    expect(childSurfaceTowardLine(surfaces, 62, 69, 4, map, NEST3_CONTENT)).toBeNull();
  });

  it('no direct child contains the line → nearest direct child by line distance', () => {
    // Ls beyond every child span → nearest is the level-1 list [20,69] (span [2,4]).
    expect(at(childSurfaceTowardLine(surfaces, 0, 69, 99, map, NEST3_CONTENT))).toEqual([20, 69]);
  });

  it('tiebreak: when two direct (byte-disjoint, sibling) children contain Ls, prefer the one whose start line == Ls', () => {
    // Realistic disjoint siblings spanning a shared line.
    //   content: "a\nbb cc\nd\n"  → line0 "a" [0,1], line1 "bb cc" [2,7], line2 "d" [8,9]
    //   childA [0,5]  = "a\nbb " → trim "a\nbb" → span [0,1]
    //   childB [5,8]  = "cc\n"   → trim "cc"    → span [1,1]  (start line == Ls)
    const content = 'a\nbb cc\nd\n';
    const m = buildByteLineMap(content);
    const surf: NestingSurface[] = [
      { r0: 0, r1: 10 },  // cursor/container (lines 0-2)
      { r0: 0, r1: 5 },   // childA: trim span [0,1]
      { r0: 5, r1: 8 },   // childB: trim span [1,1] — start line == Ls
    ];
    // Both spans contain line 1; childB begins on line 1 → start-line tiebreak picks it.
    expect(at(childSurfaceTowardLine(surf, 0, 10, 1, m, content))).toEqual([5, 8]);
  });
});

// ── depthOfSurface / relocateSurface (§2 commit-stable relocation) ──────────────

describe('depthOfSurface', () => {
  // Nested: outer [0,30] ⊃ mid [0,20] ⊃ inner [2,18]. (Note: outer and mid share r0=0.)
  const surfaces: NestingSurface[] = [
    { r0: 0, r1: 30 },
    { r0: 0, r1: 20 },
    { r0: 2, r1: 18 },
    { r0: 22, r1: 28 },
  ];

  it('counts strict containers (a top-level surface has depth 0)', () => {
    expect(depthOfSurface(surfaces, 0, 30)).toBe(0);  // outer — nothing contains it
    expect(depthOfSurface(surfaces, 0, 20)).toBe(1);  // mid — contained by outer (same r0, larger r1)
    expect(depthOfSurface(surfaces, 2, 18)).toBe(2);  // inner — contained by outer + mid
    expect(depthOfSurface(surfaces, 22, 28)).toBe(1); // sibling — contained by outer only
  });
});

describe('relocateSurface (commit-stable (startLine, depth) lookup)', () => {
  // A container and its FIRST CHILD share a start line — depth disambiguates them.
  // content: "::: d\nAAA\n:::\n"  → Div [0,12] starts line 0; its first child Para
  // [6,9] starts line 1; a second container Outer [0,12]... here we model the real
  // shared-start-line hazard: Outer [0,18] ⊃ Inner [0,12], both starting at line 0.
  const content = 'AAAA\nBBBB\nCCCC\n';
  const map = buildByteLineMap(content);
  const surfaces: NestingSurface[] = [
    { r0: 0, r1: 15 },  // outer container, start line 0, depth 0
    { r0: 0, r1: 9 },   // inner container, start line 0, depth 1 (shares start line!)
    { r0: 5, r1: 9 },   // a leaf inside inner, start line 1
  ];

  it('disambiguates a container and its same-start-line child by depth', () => {
    // start line 0 has TWO surfaces ([0,15] depth 0 and [0,9] depth 1).
    expect(relocateSurface(surfaces, map, 0, 0)).toEqual({ r0: 0, r1: 15 });
    expect(relocateSurface(surfaces, map, 0, 1)).toEqual({ r0: 0, r1: 9 });
  });

  it('relocates a surface by its start line when unambiguous', () => {
    expect(relocateSurface(surfaces, map, 1, 2)).toEqual({ r0: 5, r1: 9 });
  });

  it('returns null when no surface matches (startLine, depth)', () => {
    expect(relocateSurface(surfaces, map, 0, 5)).toBeNull(); // no depth-5 surface at line 0
    expect(relocateSurface(surfaces, map, 9, 0)).toBeNull(); // no surface starts at line 9
  });
});
