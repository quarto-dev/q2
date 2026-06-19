/**
 * §0 — List-item surfaces integration tests (TDD).
 *
 * Tests the pool-id borrow onto <li>/<dd> (for tight / Plain-leading items)
 * and the Range-aware leading-block measure helper in outerBlocks.ts.
 *
 * Tests:
 *  0.a — Unit (jsdom): Range-aware leading-block measure. Stubs getClientRects
 *         so we can assert the Range path measures the leading-text extent, not
 *         the whole element.
 *
 *  0.b — Integration: a tight single-block bullet list renders
 *         <li data-block-pool-id>; snapshotOuterBlockGeometry keys the item by
 *         its leading block's range; LOCKED mode activates the whole <ul>
 *         (resolveOuterBlock climbs to the <ul> because <ul> is a PREFIXING_TAG).
 *
 *  0.c — Integration: a text-with-sublist item — the <li> borrows the leading
 *         Plain's pool-id; the leading-block Range measure EXCLUDES the sublist
 *         height (measured height < full <li> height).
 *
 *  0.f — Integration: an EMPTY list item (authored `- `) renders a bare <li>
 *         with NO data-block-pool-id and does NOT crash the list render; a
 *         sibling normal item DOES carry one.
 *
 *  0.g — Integration (Amendment A1 — the predicate test): a LOOSE list item
 *         (leading Para) renders <li> with NO data-block-pool-id, and the inner
 *         <p> retains the SOLE pool-id (no duplicate). Also tests <dd> whose
 *         definition body leads with a Para.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { previewRegistry } from './registry';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import type { ResolvedSource } from './sourceIndex';
import { measureLeadingBlockBox } from './outerBlocks';
import { snapshotOuterBlockGeometry } from './outerBlocks';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    document.body.innerHTML = '';
});

/* ─── shared helpers ────────────────────────────────────────────────────────── */

function rect(
    left: number, top: number, right: number, bottom: number,
): DOMRect {
    return {
        left, top, right, bottom, x: left, y: top,
        width: right - left, height: bottom - top,
        toJSON: () => ({}),
    };
}

function astJson(blocks: any[], pool: any[] = []): string {
    const ast: any = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        ...(pool.length > 0 ? { astContext: { p: pool } } : {}),
    };
    return JSON.stringify(ast);
}

const STR = (c: string) => ({ t: 'Str', c });
const PLAIN = (...inlines: any[]) => ({ t: 'Plain', c: inlines });
const PARA = (...inlines: any[]) => ({ t: 'Para', c: inlines });

const noopNav = () => {};
const noopSet = () => {};

/** Minimal resolveSource that returns non-Opaque for any node that has .s */
function makeResolveSource(pool: any[]) {
    return (node: any): ResolvedSource | null => {
        const s = node?.s;
        if (s === undefined || s === null) return null;
        const entry = pool[s];
        if (!entry) return null;
        return { reachabilityClass: 'Reachable', sourceEntry: entry, sourceIndex: null as any };
    };
}

/** Mount with a real PreviewContext supplying the pool and resolveSource. */
function mountWithPool(blocks: any[], pool: any[]) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        pool,
        content: '',
        resolveSource: makeResolveSource(pool),
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson(blocks, pool)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={noopSet}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
}

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 0.a — Range-aware leading-block measure
 *
 * When blockEl is an <li>/<dd> that has a descendant carrying
 * [data-block-pool-id] (a nested block like a sublist), measureLeadingBlockBox
 * uses a DOM Range from the element's start to its first such child (exclusive).
 * Otherwise it falls back to the element's own box.
 *
 * jsdom: getClientRects/getBoundingClientRect on a Range returns zeros; we stub
 * Range.prototype.getClientRects (or the specific range) to assert the Range
 * path is taken and returns the stubbed extent.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('0.a — measureLeadingBlockBox: Range-aware leading-block measure', () => {
    it('falls back to element box when li has no nested pool-id child', () => {
        // A plain <li> with no nested [data-block-pool-id] descendant:
        // measureLeadingBlockBox should measure the element directly.
        const li = document.createElement('li');
        li.textContent = 'simple item';
        document.body.appendChild(li);

        vi.spyOn(li, 'getBoundingClientRect').mockReturnValue(
            rect(0, 0, 200, 20),
        );
        // getComputedStyle returns empty strings in jsdom → px() → 0
        const result = measureLeadingBlockBox(li);
        expect(result.contentHeight).toBe(20); // rect.height - 0 padding/border
        expect(result.rangeUsed).toBe(false);
    });

    it('uses a Range when li has a nested [data-block-pool-id] child', () => {
        // <li> with both a text run and a nested <ul data-block-pool-id="0">
        const li = document.createElement('li');
        const text = document.createTextNode('leading text ');
        const sublist = document.createElement('ul');
        sublist.setAttribute('data-block-pool-id', '0');
        li.appendChild(text);
        li.appendChild(sublist);
        document.body.appendChild(li);

        // Stub the li's own rect (full box, includes sublist)
        vi.spyOn(li, 'getBoundingClientRect').mockReturnValue(
            rect(0, 0, 200, 60), // height 60 includes sublist
        );

        // jsdom does not implement Range.getBoundingClientRect — install a
        // polyfill so vi.spyOn can intercept it.
        // We track calls ourselves to verify the Range path is exercised.
        let rangeBcrCalled = false;
        const polyfill = function (this: Range) {
            rangeBcrCalled = true;
            return rect(0, 0, 200, 20); // only the leading text line
        };
        (Range.prototype as any).getBoundingClientRect = polyfill;

        try {
            const result = measureLeadingBlockBox(li);
            expect(result.rangeUsed).toBe(true);
            expect(result.contentHeight).toBe(20); // from range rect, no padding/border
            // Confirm the Range path was exercised
            expect(rangeBcrCalled).toBe(true);
        } finally {
            // Clean up polyfill
            delete (Range.prototype as any).getBoundingClientRect;
        }
    });

    it('falls back gracefully when Range.getBoundingClientRect is unavailable (jsdom)', () => {
        // Without the polyfill, measureLeadingBlockBox guards with typeof check.
        // The function should not throw; contentHeight will be 0; rangeUsed is still true.
        const li = document.createElement('li');
        const text = document.createTextNode('text');
        const sublist = document.createElement('ul');
        sublist.setAttribute('data-block-pool-id', '0');
        li.appendChild(text);
        li.appendChild(sublist);
        document.body.appendChild(li);

        vi.spyOn(li, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        // jsdom has no getBoundingClientRect on Range — must not throw
        expect(() => measureLeadingBlockBox(li)).not.toThrow();
        const result = measureLeadingBlockBox(li);
        expect(result.rangeUsed).toBe(true); // Range path was still taken
        expect(result.contentHeight).toBe(0); // no layout data available in jsdom
    });

    it('returns element box for non-li/dd elements (para, div)', () => {
        const p = document.createElement('p');
        p.setAttribute('data-block-pool-id', '0');
        p.textContent = 'paragraph text';
        document.body.appendChild(p);

        vi.spyOn(p, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 24));
        const result = measureLeadingBlockBox(p);
        expect(result.rangeUsed).toBe(false);
        expect(result.contentHeight).toBe(24);
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 0.b — tight single-block bullet list: pool-id on <li>, LOCKED mode
 *
 * A tight BulletList item has `[Plain]` as its content. The <li> should borrow
 * the Plain's pool-id. In LOCKED mode (no unlockNestingCursor) a click resolves
 * to the outermost prefixing container (<ul>).
 *
 * Fixture (tight list, 2 items):
 *   content: "- apple\n- banana\n"
 *   pool[0] = Plain "apple" r=[2,7]
 *   pool[1] = Plain "banana" r=[10,16]
 *   pool[2] = BulletList r=[0,18]
 *
 * AST: BulletList.s=2, items[0]=[Plain.s=0], items[1]=[Plain.s=1]
 * ─────────────────────────────────────────────────────────────────────────── */

describe('0.b — tight single-block BulletList: <li> borrows leading Plain pool-id', () => {
    const POOL_0B = [
        { t: 0, r: [2, 7],   d: 0 },  // pool[0] Plain "apple"
        { t: 0, r: [10, 16], d: 0 }, // pool[1] Plain "banana"
        { t: 0, r: [0, 18],  d: 0 }, // pool[2] BulletList
    ];

    const BLOCKS_0B = [
        {
            t: 'BulletList',
            s: 2,
            c: [
                [{ t: 'Plain', s: 0, c: [STR('apple')] }],
                [{ t: 'Plain', s: 1, c: [STR('banana')] }],
            ],
        },
    ];

    it('<li> elements carry data-block-pool-id matching their Plain block pool index', () => {
        const { container } = mountWithPool(BLOCKS_0B, POOL_0B);
        const lis = container.querySelectorAll('li');
        expect(lis).toHaveLength(2);
        expect(lis[0].getAttribute('data-block-pool-id')).toBe('0');
        expect(lis[1].getAttribute('data-block-pool-id')).toBe('1');
    });

    it('<ul> also carries its own pool-id (BulletList is still editable as a whole)', () => {
        const { container } = mountWithPool(BLOCKS_0B, POOL_0B);
        const ul = container.querySelector('ul');
        expect(ul?.getAttribute('data-block-pool-id')).toBe('2');
    });

    it('<Plain> renders NO element — no extra DOM between <li> and text', () => {
        const { container } = mountWithPool(BLOCKS_0B, POOL_0B);
        const li = container.querySelector('li');
        // Plain renders a fragment, so the li's firstElementChild should be null
        // (or at most a text node, not a wrapper element from Plain itself).
        // The text content should be directly in the li.
        expect(li?.textContent).toBe('apple');
        // No extra wrapper element inside the li
        const innerPoolEl = li?.querySelector('[data-block-pool-id]');
        expect(innerPoolEl).toBeNull();
    });

    it('snapshotOuterBlockGeometry includes the li as a surface (via its borrowed pool-id)', () => {
        const { container } = mountWithPool(BLOCKS_0B, POOL_0B);
        const ul = container.querySelector<HTMLElement>('ul');
        const li0 = container.querySelectorAll<HTMLElement>('li')[0];
        const li1 = container.querySelectorAll<HTMLElement>('li')[1];
        expect(ul).not.toBeNull();
        expect(li0).not.toBeNull();
        expect(li1).not.toBeNull();

        // Give each element a visible rect so snapshotOuterBlockGeometry includes them.
        vi.spyOn(ul!, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(li0!, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 30));
        vi.spyOn(li1!, 'getBoundingClientRect').mockReturnValue(rect(0, 30, 200, 60));

        // topBlockR0 = 0 (the BulletList's r[0])
        const map = snapshotOuterBlockGeometry(li0!, POOL_0B, 0);
        // The snapshot should include: ul (key "0:18") and li0 (key "2:7") and li1 (key "10:16")
        expect(map.size).toBeGreaterThanOrEqual(2);
        // The ul is keyed relative to topBlockR0=0: "0:18"
        expect(map.has('0:18')).toBe(true);
        // li0 is keyed by its borrowed pool-id = pool[0] = r=[2,7] → key "2:7"
        expect(map.has('2:7')).toBe(true);
        // li1 is keyed by pool[1] = r=[10,16] → key "10:16"
        expect(map.has('10:16')).toBe(true);
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 0.c — text-with-sublist item: leading Range measure excludes sublist
 *
 * An item whose content is [Plain, BulletList]. The <li> borrows the Plain's
 * pool-id. measureLeadingBlockBox should use a Range (because the <li> has a
 * [data-block-pool-id] descendant — the nested <ul>) and the measured height
 * should be LESS than the full <li> height.
 *
 * Fixture (one item with a sublist):
 *   AST: BulletList item[0] = [Plain.s=0, BulletList.s=1]
 *   pool[0] = Plain "intro" r=[2,7]
 *   pool[1] = inner BulletList r=[8,20]
 *   pool[2] = outer BulletList r=[0,20]
 *
 * The outer <li> has data-block-pool-id="0" (borrowed from the Plain).
 * Inside it: text ("intro") + inner <ul data-block-pool-id="1">.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('0.c — text-with-sublist item: Range measure excludes sublist height', () => {
    const POOL_0C = [
        { t: 0, r: [2, 7],   d: 0 },  // pool[0] Plain "intro"
        { t: 0, r: [8, 20],  d: 0 },  // pool[1] inner BulletList
        { t: 0, r: [0, 22],  d: 0 },  // pool[2] outer BulletList
    ];

    const BLOCKS_0C = [
        {
            t: 'BulletList',
            s: 2,
            c: [
                [
                    { t: 'Plain', s: 0, c: [STR('intro')] },
                    {
                        t: 'BulletList',
                        s: 1,
                        c: [[{ t: 'Plain', c: [STR('sub-item')] }]],
                    },
                ],
            ],
        },
    ];

    it('<li> borrows the leading Plain pool-id (not the inner BulletList)', () => {
        const { container } = mountWithPool(BLOCKS_0C, POOL_0C);
        const lis = container.querySelectorAll('li');
        // The outer <li> should have pool-id="0" (Plain)
        expect(lis[0].getAttribute('data-block-pool-id')).toBe('0');
    });

    it('inner BulletList still carries its own pool-id inside the <li>', () => {
        const { container } = mountWithPool(BLOCKS_0C, POOL_0C);
        const innerUl = container.querySelector('li > ul');
        expect(innerUl?.getAttribute('data-block-pool-id')).toBe('1');
    });

    it('measureLeadingBlockBox uses Range path (rangeUsed=true) for <li> with nested pool-id child', () => {
        const { container } = mountWithPool(BLOCKS_0C, POOL_0C);
        const li = container.querySelector<HTMLElement>('li');
        expect(li).not.toBeNull();

        // Stub li's full rect (taller, includes sublist)
        vi.spyOn(li!, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        // jsdom doesn't implement Range.getBoundingClientRect — install a polyfill
        // so the stub returns a measurable value for the leading text.
        (Range.prototype as any).getBoundingClientRect = () => rect(0, 0, 200, 20);

        try {
            const result = measureLeadingBlockBox(li!);
            expect(result.rangeUsed).toBe(true);
            // 20 < 60: the range-measured height is less than the full li height
            expect(result.contentHeight).toBe(20);
            expect(result.contentHeight).toBeLessThan(60);
        } finally {
            delete (Range.prototype as any).getBoundingClientRect;
        }
    });

    // 0.c-wiring: proves the range-aware measure is wired into the PRODUCTION path.
    // This test drives snapshotOuterBlockGeometry (the same path captureGeometry
    // uses in PreviewRoot.tsx) and asserts the <li>'s snapshot entry uses the short
    // Range height (20), NOT the full element height (60). If measureBlockBox were
    // still measuring the element box for li/dd, this would see 60 and FAIL.
    it('0.c-wiring: snapshotOuterBlockGeometry uses Range-aware measure for <li> with sublist (production path)', () => {
        const { container } = mountWithPool(BLOCKS_0C, POOL_0C);
        const outerUl = container.querySelector<HTMLElement>('ul');
        const li = container.querySelector<HTMLElement>('li');
        const innerUl = container.querySelector<HTMLElement>('li > ul');
        expect(outerUl).not.toBeNull();
        expect(li).not.toBeNull();
        expect(innerUl).not.toBeNull();

        // Stub element rects: outer <ul> and inner <ul> both tall (60px).
        // The <li> itself is also tall (60px) — includes the sublist height.
        vi.spyOn(outerUl!, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(li!, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(innerUl!, 'getBoundingClientRect').mockReturnValue(rect(0, 20, 200, 60));

        // Stub Range.prototype.getBoundingClientRect to return only the leading text
        // height (20px). Production measureBlockBox should use this for the <li>.
        (Range.prototype as any).getBoundingClientRect = () => rect(0, 0, 200, 20);

        try {
            // topBlockR0 = pool[2].r[0] = 0 (the outer BulletList).
            // POOL_0C: pool[0]={r:[2,7]}, pool[1]={r:[8,20]}, pool[2]={r:[0,22]}
            const topBlockR0 = 0;
            const map = snapshotOuterBlockGeometry(li!, POOL_0C, topBlockR0);

            // The <li> carries pool[0]={r:[2,7]}, so its block-relative key is "2:7".
            // Before the fix: measureBlockBox(li) returns rect.height=60 → key "2:7" has contentHeight 60.
            // After the fix: measureBlockBox detects li+nested-pool-id → uses Range → contentHeight 20.
            expect(map.has('2:7')).toBe(true);
            const liEntry = map.get('2:7')!;
            expect(liEntry.contentHeight).toBe(20);   // SHORT height from Range, not 60 from element rect
            expect(liEntry.contentHeight).toBeLessThan(60);
        } finally {
            delete (Range.prototype as any).getBoundingClientRect;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 0.f — EMPTY list item renders bare <li>, no crash
 *
 * An empty list item has content `[]` (empty block array). The <li> should
 * render with NO data-block-pool-id and must not crash. A sibling normal item
 * should still carry its pool-id.
 *
 * Fixture:
 *   items[0] = []                         (empty item)
 *   items[1] = [Plain.s=0 "text"]        (normal item)
 *   pool[0] = Plain "text" r=[4, 8]
 *   pool[1] = BulletList r=[0, 10]
 * ─────────────────────────────────────────────────────────────────────────── */

describe('0.f — empty list item renders bare <li> with no pool-id, no crash', () => {
    const POOL_0F = [
        { t: 0, r: [4, 8],   d: 0 },  // pool[0] Plain "text"
        { t: 0, r: [0, 10],  d: 0 },  // pool[1] BulletList
    ];

    const BLOCKS_0F = [
        {
            t: 'BulletList',
            s: 1,
            c: [
                [],                                                  // empty item
                [{ t: 'Plain', s: 0, c: [STR('text')] }],          // normal item
            ],
        },
    ];

    it('does not crash when rendering an empty list item', () => {
        expect(() => mountWithPool(BLOCKS_0F, POOL_0F)).not.toThrow();
    });

    it('empty item renders <li> with NO data-block-pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0F, POOL_0F);
        const lis = container.querySelectorAll('li');
        expect(lis).toHaveLength(2);
        // First item is empty → no pool-id
        expect(lis[0].hasAttribute('data-block-pool-id')).toBe(false);
    });

    it('sibling normal item still carries data-block-pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0F, POOL_0F);
        const lis = container.querySelectorAll('li');
        expect(lis[1].getAttribute('data-block-pool-id')).toBe('0');
    });

    // Also verify OrderedList doesn't crash on empty items
    const BLOCKS_0F_OL = [
        {
            t: 'OrderedList',
            s: 1,
            c: [
                [1, { t: 'DefaultStyle' }, { t: 'DefaultDelim' }],
                [
                    [],                                              // empty item
                    [{ t: 'Plain', s: 0, c: [STR('text')] }],     // normal item
                ],
            ],
        },
    ];

    it('OrderedList: empty item renders bare <li>, sibling item has pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0F_OL, POOL_0F);
        const lis = container.querySelectorAll('li');
        expect(lis).toHaveLength(2);
        expect(lis[0].hasAttribute('data-block-pool-id')).toBe(false);
        expect(lis[1].getAttribute('data-block-pool-id')).toBe('0');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 0.g — Amendment A1: LOOSE item (leading Para) → NO pool-id borrow on <li>
 *
 * When a list item's first block is a Para (not a Plain), the <li> must NOT
 * borrow the pool-id. The <p> inside the <li> keeps the sole pool-id — there
 * must be no duplication.
 *
 * Same applies for DefinitionList: a <dd> whose body leads with Para must not
 * borrow; the inner <p> retains the sole id.
 *
 * Fixture (loose list — Para-leading items):
 *   items[0] = [Para.s=0 "Hello"]
 *   pool[0] = Para "Hello" r=[0, 5]
 *   pool[1] = BulletList r=[0, 7]
 * ─────────────────────────────────────────────────────────────────────────── */

describe('0.g — Amendment A1: loose item (Para-leading) → no borrow on <li>/<dd>', () => {
    const POOL_0G = [
        { t: 0, r: [0, 5],  d: 0 },  // pool[0] Para "Hello"
        { t: 0, r: [0, 7],  d: 0 },  // pool[1] BulletList
    ];

    const BLOCKS_0G_BL = [
        {
            t: 'BulletList',
            s: 1,
            c: [
                [{ t: 'Para', s: 0, c: [STR('Hello')] }],   // Para-leading (loose)
            ],
        },
    ];

    it('BulletList loose item: <li> has NO data-block-pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0G_BL, POOL_0G);
        const li = container.querySelector('li');
        expect(li).not.toBeNull();
        expect(li!.hasAttribute('data-block-pool-id')).toBe(false);
    });

    it('BulletList loose item: inner <p> retains the SOLE pool-id (no duplication)', () => {
        const { container } = mountWithPool(BLOCKS_0G_BL, POOL_0G);
        // There should be exactly ONE element with pool-id="0" in the whole tree
        const allWithPid0 = container.querySelectorAll('[data-block-pool-id="0"]');
        expect(allWithPid0).toHaveLength(1);
        // And it should be a <p>, not an <li>
        expect(allWithPid0[0].tagName.toLowerCase()).toBe('p');
    });

    // OrderedList variant
    const POOL_0G_OL = [
        { t: 0, r: [0, 5],  d: 0 },  // pool[0] Para
        { t: 0, r: [0, 7],  d: 0 },  // pool[1] OrderedList
    ];

    const BLOCKS_0G_OL = [
        {
            t: 'OrderedList',
            s: 1,
            c: [
                [1, { t: 'DefaultStyle' }, { t: 'DefaultDelim' }],
                [
                    [{ t: 'Para', s: 0, c: [STR('Hello')] }],
                ],
            ],
        },
    ];

    it('OrderedList loose item: <li> has NO data-block-pool-id, <p> keeps the id', () => {
        const { container } = mountWithPool(BLOCKS_0G_OL, POOL_0G_OL);
        const li = container.querySelector('li');
        expect(li!.hasAttribute('data-block-pool-id')).toBe(false);
        const allWithPid0 = container.querySelectorAll('[data-block-pool-id="0"]');
        expect(allWithPid0).toHaveLength(1);
        expect(allWithPid0[0].tagName.toLowerCase()).toBe('p');
    });

    // DefinitionList variant: <dd> with Para-leading body must not borrow
    const POOL_0G_DL = [
        { t: 0, r: [5, 10], d: 0 },  // pool[0] Para "World"
        { t: 0, r: [0, 15], d: 0 },  // pool[1] DefinitionList
    ];

    const BLOCKS_0G_DL = [
        {
            t: 'DefinitionList',
            s: 1,
            c: [
                [
                    [STR('Term')],
                    [[{ t: 'Para', s: 0, c: [STR('World')] }]],
                ],
            ],
        },
    ];

    it('DefinitionList: <dd> with Para-leading body has NO data-block-pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0G_DL, POOL_0G_DL);
        const dd = container.querySelector('dd');
        expect(dd).not.toBeNull();
        expect(dd!.hasAttribute('data-block-pool-id')).toBe(false);
    });

    it('DefinitionList: inner <p> retains the SOLE pool-id (no duplication on dd)', () => {
        const { container } = mountWithPool(BLOCKS_0G_DL, POOL_0G_DL);
        const allWithPid0 = container.querySelectorAll('[data-block-pool-id="0"]');
        expect(allWithPid0).toHaveLength(1);
        expect(allWithPid0[0].tagName.toLowerCase()).toBe('p');
    });

    // DefinitionList with Plain-leading body SHOULD borrow
    const POOL_0G_DL_PLAIN = [
        { t: 0, r: [5, 10], d: 0 },  // pool[0] Plain "World"
        { t: 0, r: [0, 15], d: 0 },  // pool[1] DefinitionList
    ];

    const BLOCKS_0G_DL_PLAIN = [
        {
            t: 'DefinitionList',
            s: 1,
            c: [
                [
                    [STR('Term')],
                    [[{ t: 'Plain', s: 0, c: [STR('World')] }]],
                ],
            ],
        },
    ];

    it('DefinitionList: <dd> with Plain-leading body DOES carry data-block-pool-id', () => {
        const { container } = mountWithPool(BLOCKS_0G_DL_PLAIN, POOL_0G_DL_PLAIN);
        const dd = container.querySelector('dd');
        expect(dd).not.toBeNull();
        expect(dd!.getAttribute('data-block-pool-id')).toBe('0');
    });
});
