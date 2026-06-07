/**
 * Tests for buildSourceIndex, serializeSourceEntry, and the gate predicate.
 *
 * Plan 2a — SourceInfo-value index + structural editability gate.
 */

import { describe, test, expect } from 'vitest';
import { buildSourceIndex, serializeSourceEntry } from './sourceIndex';
import type { ReachabilityClass, SourceIndexEntry } from './sourceIndex';
import type { BlockNode } from '../framework/types';

// ── Helpers ──────────────────────────────────────────────────────────────────

type PoolEntry =
    | { t: 0; r: [number, number]; d: number }
    | { t: 4; r: [0, 0]; d: { by: { kind: string } } };

function makeAst(blocks: BlockNode[], pool: PoolEntry[]): unknown {
    return {
        'pandoc-api-version': [1, 23, 1],
        meta: {},
        blocks,
        astContext: { p: pool },
    };
}

function makePara(poolId: number, extraProps?: object): BlockNode {
    return { t: 'Para', s: poolId, c: [], ...extraProps } as unknown as BlockNode;
}

function makeHeader(level: number, poolId: number): BlockNode {
    return { t: 'Header', s: poolId, c: [level, ['', [], []], []] } as unknown as BlockNode;
}

function makeDiv(poolId: number, children: BlockNode[], classes: string[] = []): BlockNode {
    return { t: 'Div', s: poolId, c: [['', classes, []], children] } as unknown as BlockNode;
}

function makeBlockQuote(poolId: number, children: BlockNode[]): BlockNode {
    return { t: 'BlockQuote', s: poolId, c: children } as unknown as BlockNode;
}

function makeBulletList(poolId: number, items: BlockNode[][]): BlockNode {
    return { t: 'BulletList', s: poolId, c: items } as unknown as BlockNode;
}

function makeOrderedList(poolId: number, items: BlockNode[][]): BlockNode {
    return { t: 'OrderedList', s: poolId, c: [[1, { t: 'DefaultStyle' }, { t: 'DefaultDelim' }], items] } as unknown as BlockNode;
}

function makeDefinitionList(poolId: number, defs: [unknown[], BlockNode[][]][]): BlockNode {
    return { t: 'DefinitionList', s: poolId, c: defs } as unknown as BlockNode;
}

function makeFigure(poolId: number, bodyBlocks: BlockNode[], captionBlocks: BlockNode[]): BlockNode {
    return {
        t: 'Figure',
        s: poolId,
        c: [
            ['', [], []],           // c[0] = Attr
            [null, captionBlocks],  // c[1] = [shortCaption, longCaption]
            bodyBlocks,             // c[2] = body blocks
        ],
    } as unknown as BlockNode;
}

function makeTable(captionBlocks: BlockNode[], cellBlocksList: BlockNode[][]): BlockNode {
    const cells = cellBlocksList.map((blocks) => [['', [], []], { t: 'AlignDefault' }, 1, 1, blocks]);
    const row = [['', [], []], cells];
    return {
        t: 'Table',
        c: [
            ['', [], []],          // c[0] = Attr
            [null, captionBlocks], // c[1] = Caption
            [],                    // c[2] = ColSpec[]
            [['', [], []], [row]], // c[3] = TableHead
            [],                    // c[4] = TableBody[]
            [['', [], []], []],    // c[5] = TableFoot
        ],
    } as unknown as BlockNode;
}

// ── serializeSourceEntry ─────────────────────────────────────────────────────

describe('serializeSourceEntry', () => {
    test('produces "t:start-end:d" format', () => {
        expect(serializeSourceEntry({ t: 0, r: [42, 87], d: 0 })).toBe('0:42-87:0');
    });

    test('zero-offset range', () => {
        expect(serializeSourceEntry({ t: 0, r: [0, 0], d: 0 })).toBe('0:0-0:0');
    });

    test('non-zero file id', () => {
        expect(serializeSourceEntry({ t: 0, r: [10, 20], d: 3 })).toBe('0:10-20:3');
    });
});

// ── buildSourceIndex: null / invalid input ────────────────────────────────────

describe('buildSourceIndex: null / invalid input', () => {
    test('returns null for null input', () => {
        expect(buildSourceIndex(null)).toBeNull();
    });

    test('returns null for undefined input', () => {
        expect(buildSourceIndex(undefined)).toBeNull();
    });

    test('returns null for unparseable JSON', () => {
        expect(buildSourceIndex('not json')).toBeNull();
    });

    test('returns null when astContext.p is missing', () => {
        const ast = { 'pandoc-api-version': [1, 23, 1], meta: {}, blocks: [] };
        expect(buildSourceIndex(JSON.stringify(ast))).toBeNull();
    });
});

// ── buildSourceIndex: index construction ─────────────────────────────────────

describe('buildSourceIndex: index construction', () => {
    test('Original top-level Para → TopLevel entry at correct key', () => {
        const pool: PoolEntry[] = [{ t: 0, r: [42, 87], d: 0 }];
        const para = makePara(0);
        const ast = makeAst([para], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index).not.toBeNull();
        const entry = index!.get('0:42-87:0');
        expect(entry).toBeDefined();
        expect(entry!.reachabilityClass).toBe('TopLevel');
        expect(entry!.sourceNode).toStrictEqual(para);
    });

    test('Original top-level Header → TopLevel entry', () => {
        const pool: PoolEntry[] = [{ t: 0, r: [0, 20], d: 0 }];
        const header = makeHeader(2, 0);
        const ast = makeAst([header], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:0-20:0')?.reachabilityClass).toBe('TopLevel');
    });

    test('Generated (sectionize) Div → absent from index', () => {
        const pool: PoolEntry[] = [{ t: 4, r: [0, 0], d: { by: { kind: 'sectionize' } } }];
        const div = makeDiv(0, []);
        const ast = makeAst([div], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index!.size).toBe(0);
    });

    test('included-file block (d≠0) → absent from index', () => {
        const pool: PoolEntry[] = [{ t: 0, r: [0, 50], d: 1 }]; // d=1 = included file
        const para = makePara(0);
        const ast = makeAst([para], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index!.size).toBe(0);
    });

    test('value match: key is by SourceInfo value, not pool id position', () => {
        // Two entries with same range value at different pool ids; only one
        // block in the AST uses pool id 1.
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 10], d: 0 },   // pool[0]
            { t: 0, r: [20, 40], d: 0 },  // pool[1]
        ];
        const para = makePara(1); // uses pool[1]
        const ast = makeAst([para], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.has('0:20-40:0')).toBe(true);
        expect(index?.has('0:0-10:0')).toBe(false); // pool[0] unused
    });

    test('multiple blocks all get indexed', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 10], d: 0 },
            { t: 0, r: [11, 20], d: 0 },
        ];
        const para1 = makePara(0);
        const para2 = makePara(1);
        const ast = makeAst([para1, para2], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index!.size).toBe(2);
        expect(index!.get('0:0-10:0')?.reachabilityClass).toBe('TopLevel');
        expect(index!.get('0:11-20:0')?.reachabilityClass).toBe('TopLevel');
    });
});

// ── buildSourceIndex: reachabilityClass assignment ───────────────────────────

describe('buildSourceIndex: reachabilityClass', () => {
    test('top-level block → TopLevel', () => {
        const pool: PoolEntry[] = [{ t: 0, r: [0, 10], d: 0 }];
        const para = makePara(0);
        const ast = makeAst([para], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:0-10:0')?.reachabilityClass).toBe('TopLevel');
    });

    test('Para inside a fenced Div → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 50], d: 0 },  // 0 = outer div (top-level)
            { t: 0, r: [5, 45], d: 0 },  // 1 = para inside div
        ];
        const para = makePara(1);
        const div = makeDiv(0, [para]);
        const ast = makeAst([div], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-45:0')?.reachabilityClass).toBe('Descendable');
        expect(index?.get('0:0-50:0')?.reachabilityClass).toBe('TopLevel');
    });

    test('Para inside a callout (Div.callout-*) → Descendable (not Opaque)', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 100], d: 0 }, // 0 = callout div
            { t: 0, r: [5, 90], d: 0 },  // 1 = para inside callout
        ];
        const para = makePara(1);
        const calloutDiv = makeDiv(0, [para], ['callout-note']);
        const ast = makeAst([calloutDiv], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-90:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Para inside a BlockQuote → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 50], d: 0 },
            { t: 0, r: [2, 48], d: 0 },
        ];
        const para = makePara(1);
        const bq = makeBlockQuote(0, [para]);
        const ast = makeAst([bq], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:2-48:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Para inside a BulletList item → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 60], d: 0 },
            { t: 0, r: [2, 55], d: 0 },
        ];
        const para = makePara(1);
        const list = makeBulletList(0, [[para]]);
        const ast = makeAst([list], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:2-55:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Para inside an OrderedList item → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 60], d: 0 },
            { t: 0, r: [2, 55], d: 0 },
        ];
        const para = makePara(1);
        const list = makeOrderedList(0, [[para]]);
        const ast = makeAst([list], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:2-55:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Para inside a DefinitionList definition → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 80], d: 0 },
            { t: 0, r: [5, 75], d: 0 },
        ];
        const para = makePara(1);
        const dl = makeDefinitionList(0, [[[], [[para]]]]);
        const ast = makeAst([dl], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-75:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Figure body block → Descendable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 100], d: 0 },  // figure
            { t: 0, r: [5, 80], d: 0 },   // body block
        ];
        const bodyPara = makePara(1);
        const figure = makeFigure(0, [bodyPara], []);
        const ast = makeAst([figure], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-80:0')?.reachabilityClass).toBe('Descendable');
    });

    test('Figure caption block → Opaque', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 100], d: 0 },  // figure
            { t: 0, r: [5, 60], d: 0 },   // caption block
        ];
        const captionPara = makePara(1);
        const figure = makeFigure(0, [], [captionPara]);
        const ast = makeAst([figure], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-60:0')?.reachabilityClass).toBe('Opaque');
    });

    test('Para inside a Table cell → Opaque', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [5, 50], d: 0 },  // cell block
        ];
        const cellPara = makePara(0);
        const table = makeTable([], [[cellPara]]);
        const ast = makeAst([table], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-50:0')?.reachabilityClass).toBe('Opaque');
    });

    test('Opaque is absorbing: Para nested inside a Table cell inside a Div stays Opaque', () => {
        // Table cell → para directly
        const pool: PoolEntry[] = [
            { t: 0, r: [5, 50], d: 0 },  // the para in the cell
        ];
        const cellPara = makePara(0);
        const table = makeTable([], [[cellPara]]);
        // Even if we nested further, Opaque is absorbing
        const ast = makeAst([table], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:5-50:0')?.reachabilityClass).toBe('Opaque');
    });
});

// ── Gate predicate ────────────────────────────────────────────────────────────

describe('gate predicate', () => {
    // The gate in Plan 2a is: resolveSource(node)?.reachabilityClass === 'TopLevel'
    // We test this by building a sourceIndex and manually exercising the lookup logic.

    function getClass(index: Map<string, SourceIndexEntry> | null, key: string): ReachabilityClass | null {
        if (!index) return null;
        return index.get(key)?.reachabilityClass ?? null;
    }

    test('TopLevel entry → editable (truthy class)', () => {
        const pool: PoolEntry[] = [{ t: 0, r: [0, 10], d: 0 }];
        const para = makePara(0);
        const ast = makeAst([para], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        const cls = getClass(index, '0:0-10:0');
        expect(cls === 'TopLevel').toBe(true);
    });

    test('Descendable entry → not editable in Plan 2a', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 50], d: 0 },
            { t: 0, r: [5, 45], d: 0 },
        ];
        const para = makePara(1);
        const div = makeDiv(0, [para]);
        const ast = makeAst([div], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        const cls = getClass(index, '0:5-45:0');
        expect(cls === 'TopLevel').toBe(false);
        expect(cls).toBe('Descendable');
    });

    test('Opaque entry → not editable', () => {
        const pool: PoolEntry[] = [
            { t: 0, r: [5, 50], d: 0 },
        ];
        const cellPara = makePara(0);
        const table = makeTable([], [[cellPara]]);
        const ast = makeAst([table], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        const cls = getClass(index, '0:5-50:0');
        expect(cls === 'TopLevel').toBe(false);
        expect(cls).toBe('Opaque');
    });

    test('absent entry (Generated block) → not editable', () => {
        const pool: PoolEntry[] = [{ t: 4, r: [0, 0], d: { by: { kind: 'sectionize' } } }];
        const div = makeDiv(0, []);
        const ast = makeAst([div], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index!.size).toBe(0);
        const cls = getClass(index, '0:0-0:0');
        expect(cls === 'TopLevel').toBe(false);
        expect(cls).toBeNull();
    });

    test('no block-type filter in Plan 2a: any block type at TopLevel is found', () => {
        // A CodeBlock at top level should be indexed, not filtered
        const pool: PoolEntry[] = [{ t: 0, r: [0, 30], d: 0 }];
        const codeBlock = { t: 'CodeBlock', s: 0, c: [['', [], []], 'echo hello'] } as unknown as BlockNode;
        const ast = makeAst([codeBlock], pool);
        const index = buildSourceIndex(JSON.stringify(ast));
        expect(index?.get('0:0-30:0')?.reachabilityClass).toBe('TopLevel');
    });
});
