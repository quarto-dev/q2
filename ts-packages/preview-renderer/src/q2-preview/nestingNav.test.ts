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
  surfaceAtLine,
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
import { buildAncestorPath, labelForSourceNode, abbrevForSourceNode, categoryForSourceNode } from './nestingNav';

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
      { label: 'Div#sec',    r0: 0,  r1: 40, isCurrent: false, abbrev: 'Dv', category: 'container' },
      { label: 'BlockQuote', r0: 10, r1: 38, isCurrent: false, abbrev: '❝', category: 'quote' },
      { label: 'Para',       r0: 12, r1: 36, isCurrent: true,  abbrev: '¶', category: 'leaf-text' },
    ]);
  });

  it('cursor=Div(0,40) (outermost) → single crumb [Div#sec]', () => {
    expect(buildAncestorPath(SI, 0, 40)).toEqual([
      { label: 'Div#sec', r0: 0, r1: 40, isCurrent: true, abbrev: 'Dv', category: 'container' },
    ]);
  });

  it('cursor=BlockQuote(10,38) → [Div#sec(false), BlockQuote(true)]', () => {
    expect(buildAncestorPath(SI, 10, 38)).toEqual([
      { label: 'Div#sec',    r0: 0,  r1: 40, isCurrent: false, abbrev: 'Dv', category: 'container' },
      { label: 'BlockQuote', r0: 10, r1: 38, isCurrent: true,  abbrev: '❝', category: 'quote' },
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

// ── abbrevForSourceNode ────────────────────────────────────────────────────────

describe('abbrevForSourceNode', () => {
  it('Header level 1 → "H1"', () => {
    expect(abbrevForSourceNode({ t: 'Header', c: [1, ['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('H1');
  });

  it('Header level 2 → "H2"', () => {
    expect(abbrevForSourceNode({ t: 'Header', c: [2, ['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('H2');
  });

  it('Header level 6 → "H6"', () => {
    expect(abbrevForSourceNode({ t: 'Header', c: [6, ['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('H6');
  });

  it('Div → "Dv"', () => {
    expect(abbrevForSourceNode({ t: 'Div', c: [['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('Dv');
  });

  it('BlockQuote → "❝"', () => {
    expect(abbrevForSourceNode({ t: 'BlockQuote', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('❝');
  });

  it('BulletList → "•"', () => {
    expect(abbrevForSourceNode({ t: 'BulletList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('•');
  });

  it('OrderedList → "1."', () => {
    expect(abbrevForSourceNode({ t: 'OrderedList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('1.');
  });

  it('DefinitionList → "DL"', () => {
    expect(abbrevForSourceNode({ t: 'DefinitionList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('DL');
  });

  it('CodeBlock → "Cd"', () => {
    expect(abbrevForSourceNode({ t: 'CodeBlock', c: [['', [], []], ''] } as unknown as import('../framework/types').BlockNode)).toBe('Cd');
  });

  it('Figure → "Fg"', () => {
    expect(abbrevForSourceNode({ t: 'Figure', c: [['', [], []], null, []] } as unknown as import('../framework/types').BlockNode)).toBe('Fg');
  });

  it('Table → "Tb"', () => {
    expect(abbrevForSourceNode({ t: 'Table', c: [['', [], []], [], null, null, []] } as unknown as import('../framework/types').BlockNode)).toBe('Tb');
  });

  it('Para → "¶"', () => {
    expect(abbrevForSourceNode({ t: 'Para', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('¶');
  });

  it('Plain → "Pl" (distinct from Para — surfaces tight vs loose list items)', () => {
    expect(abbrevForSourceNode({ t: 'Plain', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('Pl');
  });

  it('unknown type "RawBlock" → first 2 chars "Ra"', () => {
    expect(abbrevForSourceNode({ t: 'RawBlock', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('Ra');
  });
});

// ── categoryForSourceNode ──────────────────────────────────────────────────────

describe('categoryForSourceNode', () => {
  it('Div → "container"', () => {
    expect(categoryForSourceNode({ t: 'Div', c: [['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('container');
  });

  it('BulletList → "list"', () => {
    expect(categoryForSourceNode({ t: 'BulletList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('list');
  });

  it('OrderedList → "list"', () => {
    expect(categoryForSourceNode({ t: 'OrderedList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('list');
  });

  it('DefinitionList → "list"', () => {
    expect(categoryForSourceNode({ t: 'DefinitionList', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('list');
  });

  it('BlockQuote → "quote"', () => {
    expect(categoryForSourceNode({ t: 'BlockQuote', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('quote');
  });

  it('Para → "leaf-text"', () => {
    expect(categoryForSourceNode({ t: 'Para', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('leaf-text');
  });

  it('Plain → "leaf-text"', () => {
    expect(categoryForSourceNode({ t: 'Plain', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('leaf-text');
  });

  it('Header → "leaf-text"', () => {
    expect(categoryForSourceNode({ t: 'Header', c: [2, ['', [], []], []] } as unknown as import('../framework/types').BlockNode)).toBe('leaf-text');
  });

  it('CodeBlock → "embed"', () => {
    expect(categoryForSourceNode({ t: 'CodeBlock', c: [['', [], []], ''] } as unknown as import('../framework/types').BlockNode)).toBe('embed');
  });

  it('Figure → "embed"', () => {
    expect(categoryForSourceNode({ t: 'Figure', c: [['', [], []], null, []] } as unknown as import('../framework/types').BlockNode)).toBe('embed');
  });

  it('Table → "embed"', () => {
    expect(categoryForSourceNode({ t: 'Table', c: [['', [], []], [], null, null, []] } as unknown as import('../framework/types').BlockNode)).toBe('embed');
  });

  it('unlisted type "RawBlock" → "leaf-text" (default fallback)', () => {
    expect(categoryForSourceNode({ t: 'RawBlock', c: [] } as unknown as import('../framework/types').BlockNode)).toBe('leaf-text');
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

  // §5 regression guard — one-level descent (Q1 preference, must never regress)
  //
  // NEST3_CONTENT is a 3-level structure:
  //   depth 0: top-level list [0,69]   (the outermost surface)
  //   depth 1: level-1 sub-list [20,69] (direct child of [0,69])
  //   depth 2: sub-sub-item list [39,60] (grandchild of [0,69])
  //   depth 3: "sub-sub-item" leaf [43,60]
  //
  // Descending from the depth-0 surface [0,69] toward line 3 (which has
  // "sub-sub-item" deep inside) must land on the DIRECT child [20,69] (depth 1),
  // NOT on [39,60] (depth 2) or [43,60] (depth 3).
  // The wrong answer — depth-2 surface [39,60] — would mean multi-level descent.
  it('descends exactly one level, never to the deepest leaf (§5 regression guard)', () => {
    // From the outermost surface [0,69], heading toward line 3 ("* sub-sub-item"):
    // correct: depth-1 direct child [20,69]
    // wrong:   depth-2 grandchild [39,60] or depth-3 leaf [43,60]
    const result3 = at(childSurfaceTowardLine(surfaces, 0, 69, 3, map, NEST3_CONTENT));
    expect(result3).toEqual([20, 69]);
    // Verify the wrong answer ([39,60]) is not returned:
    expect(result3).not.toEqual([39, 60]);
    expect(result3).not.toEqual([43, 60]);
    // Same holds for lines 2 and 4 (other deep targets):
    // line 2 → depth-1 [20,69], NOT depth-2 surface [24,39]
    const result2 = at(childSurfaceTowardLine(surfaces, 0, 69, 2, map, NEST3_CONTENT));
    expect(result2).toEqual([20, 69]);
    expect(result2).not.toEqual([24, 39]);
    // line 4 → depth-1 [20,69], NOT depth-2 surface [62,69]
    const result4 = at(childSurfaceTowardLine(surfaces, 0, 69, 4, map, NEST3_CONTENT));
    expect(result4).toEqual([20, 69]);
    expect(result4).not.toEqual([62, 69]);
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

// ── §3 trailing-whitespace descent consistency ──────────────────────────────────
//
// Symptom: nest-in gives different results from the beginning of a line vs inside
// it (or vs the same logical position with trailing whitespace). Candidate causes:
//   1. Two positions are on different source lines → expected divergence, not a bug.
//   2. Ls overshoots past the container's trimmed end → falls to nearest-child fallback
//      which may pick a different child than an in-range Ls.
//   3. Byte-space fallback (childSurfaceToward) disagrees with trim-aware path.
//
// These tests build the exact scenario and identify which cause fires.
//
// Fixture: two-paragraph blockquote, whose buffer has a trailing newline
// (anchorSlice includes the trailing \n from the source).
//
//   content = '> alpha\n>\n> gamma\n'  (18 bytes)
//   line 0: '> alpha'  bytes 0-6   (\n @7)
//   line 1: '>'        byte  8     (\n @9)
//   line 2: '> gamma'  bytes 10-16 (\n @17)
//   line 3: (virtual — position 18, after the trailing \n)
//
// Container [0,18]: trimmed span [0,2]   (lines 0–2)
// child1 Para alpha [2,7]:  trimmed span [0,0]
// child2 Para gamma [12,17]: trimmed span [2,2]
//
// Caret at the beginning of the buffer (Ls=0) → child1 (inside span [0,0]).
// Caret on the blank source line 1 (Ls=1) → nearest fallback (tie) → child1 (by r0).
// Caret on source line 2 (Ls=2) → child2 (inside span [2,2]).
// Caret in trailing blank (Ls=3, overshoot past trimmed end 2) → nearest → child2 (distance 1).
//
// The key assertion for §3: Ls=3 (trailing blank overshoot) should give SAME result as
// Ls=2 (last content line). This tests whether trailing whitespace produces consistent
// descent relative to the last content position.

const TRAIL_CONTENT = '> alpha\n>\n> gamma\n';
const trailMap = buildByteLineMap(TRAIL_CONTENT);

const trailIndex = makeIndex([
  ['0:0-18:0',  'TopLevel'],   // container BlockQuote
  ['0:2-7:0',   'Descendable'], // child1 Para alpha
  ['0:12-17:0', 'Descendable'], // child2 Para gamma
]);
const trailSurfaces = buildNestingSurfaces(trailIndex);
const atT = (s: ReturnType<typeof childSurfaceTowardLine>) => (s ? [s.r0, s.r1] : null);

describe('§3 trailing-whitespace descent consistency', () => {
  // Verify the fixture's trimmed spans match what the test comments claim.
  it('fixture: container trimmed span is [0,2], child1 is [0,0], child2 is [2,2]', () => {
    expect(surfaceLineSpan({ r0: 0, r1: 18 }, TRAIL_CONTENT, trailMap)).toEqual([0, 2]);
    expect(surfaceLineSpan({ r0: 2, r1: 7 }, TRAIL_CONTENT, trailMap)).toEqual([0, 0]);
    expect(surfaceLineSpan({ r0: 12, r1: 17 }, TRAIL_CONTENT, trailMap)).toEqual([2, 2]);
  });

  it('Ls at last content line (2) descends to child2 gamma [12,17]', () => {
    // This is the "correct" reference result: caret on the gamma line.
    expect(atT(childSurfaceTowardLine(trailSurfaces, 0, 18, 2, trailMap, TRAIL_CONTENT)))
      .toEqual([12, 17]);
  });

  it('§3 repro: Ls past trimmed end (3, trailing blank) must give same result as Ls=2 → child2 [12,17]', () => {
    // CAUSE 2 SCENARIO: bufferLine=3 when caret is after the trailing '\n' in the
    // anchorSlice (> alpha\n>\n> gamma\n has a trailing \n so position 18 → bufferLine=3).
    // Ls = map.lineOf(anchorR0=0) + bufferLine=3 = 3.
    // With Ls=3: no child's trimmed span contains 3. Nearest fallback:
    //   child1 [0,0] distance 3; child2 [2,2] distance 1 → child2 [12,17].
    // PASSES today. If the distance comparison ever changes (e.g. equidistant case or
    // additional blank lines make it equidistant), the tie-break by r0 picks child1
    // (WRONG). Clamping Ls to surfaceLineSpan(container)[1]=2 avoids the fallback
    // entirely and uses the "containing" path → child2 regardless.
    // This test pins the current correct behavior and will catch future regressions
    // when trailing whitespace happens to land Ls equidistant from two children.
    expect(atT(childSurfaceTowardLine(trailSurfaces, 0, 18, 3, trailMap, TRAIL_CONTENT)))
      .toEqual([12, 17]);
  });

  it('§3 equidistant/overshoot: Ls in the gap ties to child1 by r0; Ls past all children picks child2 by distance', () => {
    // The equidistant case is where cause 2 ACTUALLY fires: two children at [0,0] and [2,2],
    // Ls=1 (equidistant, distance 1 each). The current "nearest" fallback breaks ties by
    // smallest r0 → child1 (which is the tie-break behavior when Ls is inside the container range).
    //
    // Fixture: content 'AAA\n\nBBB\n' (9 bytes)
    //   line 0: 'AAA' [0,3]
    //   line 1: ''   [4,4]
    //   line 2: 'BBB' [5,8]
    //   (no line 3, just trailing \n at byte 8)
    //
    // Container [0,9]: trimmed span [0,2]
    // child1 'AAA' [0,3]: trimmed span [0,0]
    // child2 'BBB' [5,8]: trimmed span [2,2]
    //
    // Ls=1 (blank middle line, inside container): equidistant (distance 1 from each child).
    // Current behavior: tie → child1 (by r0). After clamping to container trimmed end (2):
    // Ls would be 1 (not overshooting), still tie → child1.
    // Note: clamping doesn't help the equidistant-inside-range case; it only helps when
    // Ls overshoots PAST the container's trimmed end. When Ls is between children (inside
    // the container range), the tie → child1 is arguably acceptable (the blank line IS
    // between the two children, closer semantically to neither).
    //
    // REAL OVERSHOOT CASE: if the container buffer ends with a blank line making bufferLine=3
    // (Ls=3), that's past child2's span [2,2] by distance 1, and child1 is past by distance 3.
    // → child2 wins clearly. No tie. Nearest picks the correct last child.
    //
    // CONCLUSION: the "nearest fallback" already picks the right child (last child) when
    // Ls overshoots past ALL children. The equidistant-tie case only arises when Ls is
    // BETWEEN children (in a gap), which is a rare edge case where both child1 and child2
    // are equidistant. For this case, tie→child1 is the current behavior.
    //
    // This test documents the equidistant-gap behavior (cause 2 edge case):
    const content2 = 'AAA\n\nBBB\n';
    const map2 = buildByteLineMap(content2);
    const idx2 = makeIndex([
      ['0:0-9:0', 'TopLevel'],
      ['0:0-3:0', 'Descendable'], // child1 AAA
      ['0:5-8:0', 'Descendable'], // child2 BBB
    ]);
    const surf2 = buildNestingSurfaces(idx2);
    // Ls=1 (blank gap between children): equidistant → child1 (current behavior via r0 tie)
    expect(atT(childSurfaceTowardLine(surf2, 0, 9, 1, map2, content2))).toEqual([0, 3]);
    // Ls=3 (overshoot past container trimmed end 2): child2 is nearer (distance 1 vs 3) → child2
    expect(atT(childSurfaceTowardLine(surf2, 0, 9, 3, map2, content2))).toEqual([5, 8]);
    // Ls=2 (at child2's trimmed span): child2 contains → child2
    expect(atT(childSurfaceTowardLine(surf2, 0, 9, 2, map2, content2))).toEqual([5, 8]);
  });

  it('§3 cause-1 vs cause-2 determination: same buffer line different columns give same Ls, same result', () => {
    // NOTE: Both calls pass the same Ls; this test cannot fail by construction.
    // It documents WHY column position is irrelevant at the unit level: Ls already
    // loses column information before childSurfaceTowardLine is called. The actual
    // column-invariance is enforced by readLiveCaret's '\n'-counting (production).
    //
    // "Beginning of a line vs inside it" on the SAME buffer line:
    // bufferLine is the count of '\n' before the cursor, independent of column.
    // Col 0 and col 5 on line 2 both give bufferLine=2, Ls=2.
    // childSurfaceTowardLine(Ls=2) → child2. No divergence.
    // This test DOCUMENTS that cause 1 (same-line same-Ls) is NOT a bug — there is
    // no mechanism for same-line positions to give different descent results.
    // The actual symptom "beginning vs inside" refers to different BUFFER LINES
    // (beginning of buffer line 0 vs caret in trailing blank line 3).
    const Ls_col0_line2 = 2;   // col 0 of source line 2 → bufferLine=2
    const Ls_col5_line2 = 2;   // col 5 of source line 2 → same bufferLine=2 (same Ls)
    expect(atT(childSurfaceTowardLine(trailSurfaces, 0, 18, Ls_col0_line2, trailMap, TRAIL_CONTENT)))
      .toEqual([12, 17]);
    expect(atT(childSurfaceTowardLine(trailSurfaces, 0, 18, Ls_col5_line2, trailMap, TRAIL_CONTENT)))
      .toEqual([12, 17]);
    // These are trivially the same because the input (Ls) is the same.
    // Confirms: cause 1 (different source lines → different results) is correct behavior.
    // The "bug" (if any) is only about trailing whitespace giving an UNEXPECTED different result,
    // not about column position on the same line.
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
    expect(relocateSurface(surfaces, map, 9, 0)).toBeNull(); // no surface starts at line 9 (far — outside the shift window)
  });

  it('relocates a surface whose start line shifted by one (tight-div → loose reserialization)', () => {
    // The tight-div nest-out reland bug: committing a tight div (Plain body) makes
    // the writer reserialize it LOOSE (Para), inserting a blank line BEFORE the
    // paragraph — so the surface the reland is hunting for is now one line LOWER
    // than the PRE-commit `fromStartLine` the reland captured. Exact-line matching
    // returns null → resolveLanding returns null → no reland → no edit surface.
    // relocateSurface must tolerate the small structural shift and still find it.
    const content2 = 'AAAA\nBBBB\nCCCC\nDDDD\n';
    const map2 = buildByteLineMap(content2);
    const shifted: NestingSurface[] = [
      { r0: 0, r1: 20 },   // div, start line 0, depth 0
      { r0: 10, r1: 14 },  // the paragraph — now at line 2 (was line 1 pre-commit)
    ];
    // Reland captured fromStartLine=1 (pre-commit); the para shifted to line 2.
    expect(relocateSurface(shifted, map2, 1, 1)).toEqual({ r0: 10, r1: 14 });
  });
});

// ── §1 surfaceAtLine (C1/A2): line-anchored surface resolution ─────────────────
//
// Test 1.a — pure unit test for surfaceAtLine.
//
// Worked example (from the §1 spec): a bullet list with sublist
//   - one      (line 0)
//   - two      (line 1)
//     [sub] alpha   (line 2)
//     beta          (line 3)
//   - three    (line 4)
//
// Encoded as:
//   content:
//     "- one\n- two\n  alpha\n  beta\n- three\n"
//      0    5  6   11  12   19  20  25  26     33
//   lines: 0="- one", 1="- two", 2="  alpha", 3="  beta", 4="- three"
//
// Surfaces (UNLOCK mode — includes nested item surfaces):
//   List [0,34]      depth 0  trimmed span [0,4]
//   Item1 [2,6]      depth 1  trimmed span [0,0]   "one"   (leaf — no children)
//   Item2 [8,34]     depth 1  trimmed span [1,4]   "two\n  alpha\n  beta" (container — has sub)
//   Sub [14,26]      depth 2  trimmed span [2,3]   "alpha\n  beta"        (leaf — no children)
//
// Note: "three" at line 4 is inside Item2 in this simplified model.
// For the worked example, we model a simpler fixture that still exercises the
// key behaviors: leaf resolution, container-gap → null, multi-line leaf.
//
// Simpler fixture (3 flat-ish surfaces plus a container):
//   content = "one\ntwo\nalpha\nbeta\nthree\n"
//   lines: 0="one", 1="two", 2="alpha", 3="beta", 4="three"
//
//   Surfaces:
//     Container [0,26]  depth 0  trimmed span [0,4]  (has children)
//     Leaf_one  [0,3]   depth 1  trimmed span [0,0]  leaf at line 0
//     Leaf_sub  [8,21]  depth 1  trimmed span [2,3]  leaf spanning lines 2-3
//     Leaf_three [22,27] depth 1 trimmed span [4,4]  leaf at line 4
//
//   Expected results:
//     L=0 → Leaf_one [0,3]    (leaf, depth 1)
//     L=1 → null              (container-gap: Container covers it but no leaf child at L=1)
//     L=2 → Leaf_sub [8,21]  (leaf, depth 1; covers lines 2-3)
//     L=3 → Leaf_sub [8,21]  (same leaf)
//     L=4 → Leaf_three [22,27] (leaf)
//     L=5 → null              (no surface covers L=5)

describe('surfaceAtLine (§1 C1/A2 — pure unit)', () => {
  // content: "one\ntwo\nalpha\nbeta\nthree\n" (27 bytes)
  //   line 0: "one"   bytes [0,3)
  //   line 1: "two"   bytes [4,7)
  //   line 2: "alpha" bytes [8,13)
  //   line 3: "beta"  bytes [14,18)
  //   line 4: "three" bytes [19,24)
  const SAL_CONTENT = 'one\ntwo\nalpha\nbeta\nthree\n';
  const salMap = buildByteLineMap(SAL_CONTENT);

  // Surfaces:
  //   Container [0,25] depth 0 trimmed span [0,4] (has children → is a container)
  //   Leaf_one  [0,3]  depth 1 trimmed span [0,0]
  //   Leaf_sub  [8,19] depth 1 trimmed span [2,3]  ("alpha\nbeta")
  //   Leaf_three [19,25] depth 1 trimmed span [4,4]
  const salSet: NestingSurface[] = [
    { r0: 0,  r1: 25 },  // Container (outer)
    { r0: 0,  r1: 3  },  // Leaf_one  (same r0 as container, smaller r1 → depth 1)
    { r0: 8,  r1: 19 },  // Leaf_sub  (lines 2-3)
    { r0: 19, r1: 25 },  // Leaf_three (line 4)
  ];
  const at = (s: NestingSurface | null) => (s ? [s.r0, s.r1] : null);

  it('L=0: returns Leaf_one [0,3] (deepest surface whose span contains L=0)', () => {
    // Both Container and Leaf_one span line 0; Leaf_one has greater depth (1) → wins.
    expect(at(surfaceAtLine(salSet, 0, SAL_CONTENT, salMap))).toEqual([0, 3]);
  });

  it('L=1: returns null (container-gap: Container spans line 1 but no leaf child does)', () => {
    // Container [0,25] spans line 1 (trimmed [0,4]). Its children at depth 1:
    //   Leaf_one  spans [0,0] → does not contain L=1
    //   Leaf_sub  spans [2,3] → does not contain L=1
    //   Leaf_three spans [4,4] → does not contain L=1
    // No child at L=1 → container-gap → null.
    expect(surfaceAtLine(salSet, 1, SAL_CONTENT, salMap)).toBeNull();
  });

  it('L=2: returns Leaf_sub [8,19] (spans lines 2-3)', () => {
    expect(at(surfaceAtLine(salSet, 2, SAL_CONTENT, salMap))).toEqual([8, 19]);
  });

  it('L=3: returns Leaf_sub [8,19] (same leaf, spans lines 2-3)', () => {
    expect(at(surfaceAtLine(salSet, 3, SAL_CONTENT, salMap))).toEqual([8, 19]);
  });

  it('L=4: returns Leaf_three [19,25]', () => {
    expect(at(surfaceAtLine(salSet, 4, SAL_CONTENT, salMap))).toEqual([19, 25]);
  });

  it('L=5: returns null (no surface spans line 5)', () => {
    expect(surfaceAtLine(salSet, 5, SAL_CONTENT, salMap)).toBeNull();
  });

  it('flat set (no container): every line returns its leaf', () => {
    // Two disjoint leaves, no container.
    const flatSet: NestingSurface[] = [
      { r0: 0, r1: 4 },  // "aaa\n" → trim [0,0]
      { r0: 4, r1: 8 },  // "bbb\n" → trim [1,1]
    ];
    const flatContent = 'aaa\nbbb\n';
    const flatMap = buildByteLineMap(flatContent);
    expect(at(surfaceAtLine(flatSet, 0, flatContent, flatMap))).toEqual([0, 4]);
    expect(at(surfaceAtLine(flatSet, 1, flatContent, flatMap))).toEqual([4, 8]);
  });

  it('A2: a container whose child spans [2,3] covers a gap on lines 0-1 and 4+', () => {
    // Container [0,20] depth 0 spans lines [0,4] (trimmed)
    // Single child [8,13] depth 1 spans line [2,2]  ("alpha")
    // Lines 0,1,3,4 are all container-gap → null.
    const gapContent = 'one\ntwo\nalpha\nbeta\nfive\n';
    const gapMap = buildByteLineMap(gapContent);
    const gapSet: NestingSurface[] = [
      { r0: 0,  r1: 25 },  // Container depth 0 trimmed [0,4]
      { r0: 8,  r1: 13 },  // Child "alpha" depth 1 trimmed [2,2]
    ];
    expect(surfaceAtLine(gapSet, 0, gapContent, gapMap)).toBeNull(); // gap
    expect(surfaceAtLine(gapSet, 1, gapContent, gapMap)).toBeNull(); // gap
    expect(at(surfaceAtLine(gapSet, 2, gapContent, gapMap))).toEqual([8, 13]); // leaf
    expect(surfaceAtLine(gapSet, 3, gapContent, gapMap)).toBeNull(); // gap (child only at 2)
    expect(surfaceAtLine(gapSet, 4, gapContent, gapMap)).toBeNull(); // gap
  });

  it('multi-level: inner leaf wins over its parent container', () => {
    // Three levels: outer [0,20] > mid [4,16] > inner [6,12]
    // All span line 1.
    // trimmed spans (all single bytes 'a') → just use tiny content
    const mlContent = 'AAAA\nBBBB\nCCCC\n';
    const mlMap = buildByteLineMap(mlContent);
    // No dedupe here; test with 2 distinct surfaces sharing a line.
    const ml2: NestingSurface[] = [
      { r0: 0,  r1: 15 },  // outer depth 0 spans all
      { r0: 5,  r1: 9  },  // inner depth 1 spans line 1 only
    ];
    // L=1: inner [5,9] has depth 1, outer depth 0 → inner wins
    expect(at(surfaceAtLine(ml2, 1, mlContent, mlMap))).toEqual([5, 9]);
    // But [5,9] is a leaf (no sub-surfaces) so NOT a container-gap
    // L=0: only outer covers it; outer HAS a descendant [5,9] that does NOT cover L=0 → gap
    expect(surfaceAtLine(ml2, 0, mlContent, mlMap)).toBeNull();
  });
});

// ── G16 blockquote-wrapped loose list: surfaceLineSpan + surfaceAtLine ────────
//
// Minimal repro:
//   > and can we actually   (line 0)
//   >                       (line 1, blank blockquote line with trailing space)
//   > 1.  oh                (line 2)
//   >                       (line 3, blank blockquote line with trailing space)
//   > 2.  dear              (line 4)
//   >                       (line 5, blank blockquote line with trailing space)
//   > 3.  > > god           (line 6)
//
// Offsets derived from the real pampa binary (see plan G16):
//   BlockQuote [0,65]   Para [2,24]   OrderedList [27,65]
//   Plain(oh)  [31,39]  Plain(dear)  [43,53]  (item-3 nested BQ/Para [57,65]…[61,65])
//
// Root cause: the old whitespace-only trim left the `>` blockquote continuation
// marker at r1, so Plain(oh)'s trimmed span was [2,4] instead of [2,2], and
// surfaceAtLine(destLine=3) re-resolved to `oh` (the "caught" bug).

// 65 bytes. Blank blockquote lines are "> " (trailing space) — see plan warning.
const BQLOOSE_CONTENT =
  '> and can we actually\n> \n> 1.  oh\n> \n> 2.  dear\n> \n> 3.  > > god\n';

describe('G16 surfaceLineSpan (blockquote-wrapped loose list — `>` trim)', () => {
  const map = buildByteLineMap(BQLOOSE_CONTENT);

  it('G16-span: oh [31,39] trims to [2,2] (not [2,4] with old whitespace-only trim)', () => {
    expect(surfaceLineSpan({ r0: 31, r1: 39 }, BQLOOSE_CONTENT, map)).toEqual([2, 2]);
  });

  it('G16-span: dear [43,53] trims to [4,4] (not [4,6] with old trim)', () => {
    expect(surfaceLineSpan({ r0: 43, r1: 53 }, BQLOOSE_CONTENT, map)).toEqual([4, 4]);
  });

  it('G16-span: leading Para [2,24] trims to [0,0] (shape guard — passes either way)', () => {
    expect(surfaceLineSpan({ r0: 2, r1: 24 }, BQLOOSE_CONTENT, map)).toEqual([0, 0]);
  });

  it('G16-span: outer BlockQuote [0,65] trims to [0,6] (shape guard — passes either way)', () => {
    expect(surfaceLineSpan({ r0: 0, r1: 65 }, BQLOOSE_CONTENT, map)).toEqual([0, 6]);
  });
});

describe('G16 surfaceAtLine (down-nav consequence — `oh` must not be "caught")', () => {
  // Surface set used by down-nav from inside the blockquote loose list.
  // BlockQuote [0,65], OrderedList [27,65], Plain(oh) [31,39], Plain(dear) [43,53]
  const bqSet: NestingSurface[] = [
    { r0: 0,  r1: 65 },  // BlockQuote (outermost)
    { r0: 27, r1: 65 },  // OrderedList (container)
    { r0: 31, r1: 39 },  // Plain(oh)
    { r0: 43, r1: 53 },  // Plain(dear)
  ];
  const map = buildByteLineMap(BQLOOSE_CONTENT);
  const at = (s: NestingSurface | null) => (s ? [s.r0, s.r1] : null);

  it('G16-at: line 3 (between oh/dear) → null (container-gap, not "oh" the old overlap)', () => {
    expect(surfaceAtLine(bqSet, 3, BQLOOSE_CONTENT, map)).toBeNull();
  });

  it('G16-at: line 4 → dear [43,53] (not oh [31,39] due to old overlap)', () => {
    expect(at(surfaceAtLine(bqSet, 4, BQLOOSE_CONTENT, map))).toEqual([43, 53]);
  });

  it('G16-at: line 2 → oh [31,39]', () => {
    expect(at(surfaceAtLine(bqSet, 2, BQLOOSE_CONTENT, map))).toEqual([31, 39]);
  });
});
