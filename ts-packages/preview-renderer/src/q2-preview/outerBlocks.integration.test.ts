/**
 * Unit tests for outerBlocks.ts — pure DOM helper for outer-block resolution
 * (Plan 2b, Phase 2.2).
 *
 * jsdom returns zero-area rects by default, so every test that exercises
 * getBoundingClientRect mocks it per-element via vi.spyOn.
 *
 * Epsilon choice: the coincidence epsilon is tuned so that:
 *   - A true 0px coincidence (chrome-less wrapper) → coincides → wrapper wins.
 *   - A 1px border (each edge delta is exactly 1px) → does NOT coincide → leaf wins.
 * This means eps must satisfy 0 < eps < 1. We use eps = 0.5 (midpoint), which
 * also tolerates sub-pixel rendering jitter (<0.5px) in real browsers.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import {
    isVisibleBlock,
    rectsCoincide,
    resolveOuterBlock,
    enumerateOuterBlocks,
    enumerateNestingLeaves,
    snapshotOuterBlockGeometry,
} from './outerBlocks';

afterEach(() => {
    vi.restoreAllMocks();
    // Clear any DOM nodes appended during the test so a failing assertion
    // does not leave stale elements that pollute the next test.
    document.body.innerHTML = '';
});

/* ─── helpers ──────────────────────────────────────────────────────────────── */

/** Build a DOMRect-like object. */
function rect(
    left: number, top: number, right: number, bottom: number,
): DOMRect {
    return {
        left, top, right, bottom,
        x: left, y: top,
        width: right - left,
        height: bottom - top,
        toJSON: () => ({}),
    };
}

const FULL = rect(0, 0, 200, 40);     // a typical visible element
const ZERO = rect(0, 0, 0, 0);        // collapsed / hidden

/** Attach a mock getBoundingClientRect to an element. */
function mockRect(el: Element, r: DOMRect) {
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue(r);
}

/** Build a minimal DOM tree. Returns the root and named elements. */
function makeDom(html: string): { root: HTMLElement; [key: string]: Element } {
    const root = document.createElement('div');
    root.innerHTML = html;
    document.body.appendChild(root);
    const named: Record<string, Element> = { root };
    root.querySelectorAll('[data-name]').forEach((el) => {
        named[el.getAttribute('data-name')!] = el;
    });
    return named as any;
}

/* ─── isVisibleBlock ─────────────────────────────────────────────────────────── */

describe('isVisibleBlock', () => {
    it('returns true for a non-zero-area rect', () => {
        const el = document.createElement('p');
        mockRect(el, FULL);
        expect(isVisibleBlock(el)).toBe(true);
    });

    it('returns false for a zero-width element', () => {
        const el = document.createElement('p');
        mockRect(el, rect(0, 0, 0, 40));
        expect(isVisibleBlock(el)).toBe(false);
    });

    it('returns false for a zero-height element (e.g. display:none)', () => {
        const el = document.createElement('p');
        mockRect(el, ZERO);
        expect(isVisibleBlock(el)).toBe(false);
    });
});

/* ─── rectsCoincide ─────────────────────────────────────────────────────────── */

describe('rectsCoincide', () => {
    it('returns true when rects are identical (0px delta on all edges)', () => {
        const a = document.createElement('div');
        const b = document.createElement('div');
        mockRect(a, rect(0, 0, 200, 40));
        mockRect(b, rect(0, 0, 200, 40));
        expect(rectsCoincide(a, b)).toBe(true);
    });

    it('returns false when one edge differs by 1px (1px border chrome)', () => {
        // A 1px border shrinks the inner rect on all four sides by 1px.
        // Outer: (0, 0, 200, 40); inner: (1, 1, 199, 39) → each edge delta = 1px.
        const a = document.createElement('div');  // outer wrapper
        const b = document.createElement('div');  // inner child with 1px border
        mockRect(a, rect(0, 0, 200, 40));
        mockRect(b, rect(1, 1, 199, 39));
        // Each delta is 1px → must NOT coincide (chrome detected).
        expect(rectsCoincide(a, b)).toBe(false);
    });

    it('uses the provided epsilon parameter', () => {
        const a = document.createElement('div');
        const b = document.createElement('div');
        // 2px offset on every edge.
        mockRect(a, rect(0, 0, 200, 40));
        mockRect(b, rect(2, 2, 198, 38));
        expect(rectsCoincide(a, b, 1)).toBe(false);
        expect(rectsCoincide(a, b, 2)).toBe(true);
    });
});

/* ─── resolveOuterBlock — coincidence climb ──────────────────────────────────── */

describe('resolveOuterBlock — coincidence climb (no prefixing containers)', () => {
    it('returns null when the element has no [data-block-pool-id] ancestor', () => {
        const el = document.createElement('span');
        document.body.appendChild(el);
        expect(resolveOuterBlock(el)).toBeNull();
    });

    it('chrome-less single-child wrapper: click on child → resolves to wrapper', () => {
        // A bare <div data-block-pool-id="1"> wrapping a single <p data-block-pool-id="2">
        // where both have identical bounding rects → wrapper wins.
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="wrapper">
                <p data-block-pool-id="2" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.child, rect(0, 0, 200, 40));

        expect(resolveOuterBlock(dom.child)).toBe(dom.wrapper);
    });

    it('multi-child wrapper: click on one child → resolves to that child (not wrapper)', () => {
        // The wrapper is taller than either child → rects do NOT coincide → leaf wins.
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="wrapper">
                <p data-block-pool-id="2" data-name="child1">first</p>
                <p data-block-pool-id="3" data-name="child2">second</p>
            </div>
        `);
        // wrapper spans both children vertically; child1 only spans the first line.
        mockRect(dom.wrapper, rect(0, 0, 200, 80));
        mockRect(dom.child1, rect(0, 0, 200, 40));
        mockRect(dom.child2, rect(0, 40, 200, 80));

        expect(resolveOuterBlock(dom.child1)).toBe(dom.child1);
    });

    it('is idempotent: resolving an already-resolved outer block returns that same outer block', () => {
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="wrapper">
                <p data-block-pool-id="2" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.child, rect(0, 0, 200, 40));

        const outerBlock = resolveOuterBlock(dom.child);
        expect(outerBlock).toBe(dom.wrapper);
        // resolving the outer block itself is idempotent
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        expect(resolveOuterBlock(dom.wrapper!)).toBe(dom.wrapper);
    });

    it('lone <p> with no container: resolves to itself', () => {
        const dom = makeDom(`
            <p data-block-pool-id="5" data-name="para">text</p>
        `);
        mockRect(dom.para, FULL);
        expect(resolveOuterBlock(dom.para)).toBe(dom.para);
    });

    it('skips hidden (zero-rect) ancestors in the coincidence climb', () => {
        // A hidden wrapper (zero rect) wraps a visible child; the hidden wrapper
        // must not participate in the climb — the child is returned.
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="hidden">
                <p data-block-pool-id="2" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.hidden, ZERO);   // collapsed
        mockRect(dom.child, FULL);

        expect(resolveOuterBlock(dom.child)).toBe(dom.child);
    });
});

/* ─── resolveOuterBlock — prefixing-atomic (WINS over coincidence) ──────────── */

describe('resolveOuterBlock — prefixing-atomic containers', () => {
    it('<blockquote data-block-pool-id> containing a <p data-block-pool-id>: resolves to blockquote', () => {
        const dom = makeDom(`
            <blockquote data-block-pool-id="10" data-name="bq">
                <p data-block-pool-id="11" data-name="para">quoted text</p>
            </blockquote>
        `);
        // Give both a visible rect (prefixing-atomic rule doesn't need rect comparison,
        // but isVisibleBlock needs a non-zero rect to not be filtered out).
        mockRect(dom.bq, rect(0, 0, 200, 60));
        mockRect(dom.para, rect(20, 10, 200, 50));  // indented — intentionally differs

        expect(resolveOuterBlock(dom.para)).toBe(dom.bq);
    });

    it('<ul data-block-pool-id> containing an inner <p data-block-pool-id>: resolves to ul', () => {
        const dom = makeDom(`
            <ul data-block-pool-id="20" data-name="list">
                <li>
                    <p data-block-pool-id="21" data-name="item">list item text</p>
                </li>
            </ul>
        `);
        mockRect(dom.list, rect(0, 0, 200, 40));
        mockRect(dom.item, rect(20, 5, 200, 35));

        expect(resolveOuterBlock(dom.item)).toBe(dom.list);
    });

    it('<ol data-block-pool-id> containing an inner <p data-block-pool-id>: resolves to ol', () => {
        const dom = makeDom(`
            <ol data-block-pool-id="30" data-name="list">
                <li>
                    <p data-block-pool-id="31" data-name="item">item</p>
                </li>
            </ol>
        `);
        mockRect(dom.list, rect(0, 0, 200, 40));
        mockRect(dom.item, rect(20, 5, 200, 35));

        expect(resolveOuterBlock(dom.item)).toBe(dom.list);
    });

    it('<dl data-block-pool-id> containing an inner <p data-block-pool-id>: resolves to dl', () => {
        const dom = makeDom(`
            <dl data-block-pool-id="40" data-name="list">
                <dt>term</dt>
                <dd>
                    <p data-block-pool-id="41" data-name="item">definition</p>
                </dd>
            </dl>
        `);
        mockRect(dom.list, rect(0, 0, 200, 60));
        mockRect(dom.item, rect(20, 30, 200, 55));

        expect(resolveOuterBlock(dom.item)).toBe(dom.list);
    });

    it('nested prefixing: blockquote > ul > p → outermost (blockquote) wins', () => {
        const dom = makeDom(`
            <blockquote data-block-pool-id="50" data-name="bq">
                <ul data-block-pool-id="51" data-name="list">
                    <li>
                        <p data-block-pool-id="52" data-name="para">nested item</p>
                    </li>
                </ul>
            </blockquote>
        `);
        mockRect(dom.bq, rect(0, 0, 200, 80));
        mockRect(dom.list, rect(20, 10, 200, 70));
        mockRect(dom.para, rect(40, 15, 200, 60));

        expect(resolveOuterBlock(dom.para)).toBe(dom.bq);
    });

    it('prefixing-atomic dominates coincidence: chrome-less div inside blockquote → blockquote wins', () => {
        // The chrome-less div has identical rect to the blockquote (coincident),
        // but prefixing-atomic fires FIRST → blockquote wins, never the div.
        const dom = makeDom(`
            <blockquote data-block-pool-id="60" data-name="bq">
                <div data-block-pool-id="61" data-name="wrapper">
                    <p data-block-pool-id="62" data-name="para">text</p>
                </div>
            </blockquote>
        `);
        // All rects identical (chrome-less all the way down)
        mockRect(dom.bq, rect(0, 0, 200, 40));
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.para, rect(0, 0, 200, 40));

        expect(resolveOuterBlock(dom.para)).toBe(dom.bq);
    });

    it('idempotent for a prefixing outer block: resolveOuterBlock(blockquote) === blockquote', () => {
        const dom = makeDom(`
            <blockquote data-block-pool-id="70" data-name="bq">
                <p data-block-pool-id="71" data-name="para">text</p>
            </blockquote>
        `);
        mockRect(dom.bq, rect(0, 0, 200, 60));
        mockRect(dom.para, rect(20, 10, 200, 50));

        expect(resolveOuterBlock(dom.bq)).toBe(dom.bq);
    });

    it('hidden outer prefixing: hidden blockquote > visible ul > visible p → resolves to ul (outermost visible prefixing)', () => {
        // Defensive guard: display:none normally hides the whole subtree, but if
        // somehow the outer prefixing element is hidden while an inner one is
        // visible, we return the outermost VISIBLE prefixing element, never a
        // hidden one. Without the guard, the old code would have returned the
        // hidden blockquote.
        const dom = makeDom(`
            <blockquote data-block-pool-id="80" data-name="bq">
                <ul data-block-pool-id="81" data-name="ul">
                    <li>
                        <p data-block-pool-id="82" data-name="para">item</p>
                    </li>
                </ul>
            </blockquote>
        `);
        mockRect(dom.bq,   ZERO);                       // hidden (zero rect)
        mockRect(dom.ul,   rect(0, 0, 200, 40));        // visible
        mockRect(dom.para, rect(20, 5, 200, 35));       // visible

        // resolveOuterBlock(p) should return ul (outermost *visible* prefixing),
        // NOT the hidden blockquote.
        expect(resolveOuterBlock(dom.para)).toBe(dom.ul);
    });
});

/* ─── coincidence epsilon boundary ─────────────────────────────────────────── */

describe('resolveOuterBlock — epsilon boundary (1px border vs. true coincidence)', () => {
    it('exactly 0px delta on all edges → coincides → wrapper wins', () => {
        const dom = makeDom(`
            <div data-block-pool-id="80" data-name="wrapper">
                <p data-block-pool-id="81" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.child, rect(0, 0, 200, 40));   // identical → 0px delta

        expect(resolveOuterBlock(dom.child)).toBe(dom.wrapper);
    });

    it('1px border (each edge delta = 1px) → does NOT coincide → leaf wins', () => {
        // A 1px border on the wrapper means the inner child's rect is inset by 1px
        // on every side: left/top +1, right/bottom −1.
        const dom = makeDom(`
            <div data-block-pool-id="90" data-name="wrapper">
                <p data-block-pool-id="91" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.child, rect(1, 1, 199, 39));   // 1px inset on each side

        expect(resolveOuterBlock(dom.child)).toBe(dom.child);
    });
});

/* ─── enumerateNestingLeaves (§3 mode-aware roving) ──────────────────────────── */

describe('enumerateNestingLeaves', () => {
    it('returns only the innermost (no pool-id descendant) visible surfaces', () => {
        // A list container holds two leaf items; the container is NOT a leaf.
        const dom = makeDom(`
            <ul data-block-pool-id="0" data-name="ul">
                <li><p data-block-pool-id="1" data-name="i1">one</p></li>
                <li><p data-block-pool-id="2" data-name="i2">two</p></li>
            </ul>
        `);
        mockRect(dom.ul, rect(0, 0, 200, 80));
        mockRect(dom.i1, rect(0, 0, 200, 40));
        mockRect(dom.i2, rect(0, 40, 200, 80));

        const leaves = enumerateNestingLeaves(dom.root);
        expect(leaves).toEqual([dom.i1, dom.i2]); // the <ul> container is excluded
    });

    it('excludes hidden leaves (zero rect)', () => {
        const dom = makeDom(`
            <p data-block-pool-id="1" data-name="visible">visible</p>
            <p data-block-pool-id="2" data-name="hidden">hidden</p>
        `);
        mockRect(dom.visible, FULL);
        mockRect(dom.hidden, ZERO);

        expect(enumerateNestingLeaves(dom.root)).toEqual([dom.visible]);
    });

    it('preserves DOM pre-order', () => {
        const dom = makeDom(`
            <p data-block-pool-id="1" data-name="a">A</p>
            <p data-block-pool-id="2" data-name="b">B</p>
        `);
        mockRect(dom.a, rect(0, 0, 200, 20));
        mockRect(dom.b, rect(0, 20, 200, 40));
        expect(enumerateNestingLeaves(dom.root)).toEqual([dom.a, dom.b]);
    });
});

/* ─── enumerateOuterBlocks ──────────────────────────────────────────────────── */

describe('enumerateOuterBlocks', () => {
    it('chrome-less wrapper + lone child: deduped to one outer block (the wrapper)', () => {
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="wrapper">
                <p data-block-pool-id="2" data-name="child">text</p>
            </div>
        `);
        mockRect(dom.wrapper, rect(0, 0, 200, 40));
        mockRect(dom.child, rect(0, 0, 200, 40));

        const blocks = enumerateOuterBlocks(dom.root);
        expect(blocks).toHaveLength(1);
        expect(blocks[0]).toBe(dom.wrapper);
    });

    it('multi-child wrapper: all three distinct outer blocks in DOM pre-order', () => {
        const dom = makeDom(`
            <div data-block-pool-id="1" data-name="wrapper">
                <p data-block-pool-id="2" data-name="c1">first</p>
                <p data-block-pool-id="3" data-name="c2">second</p>
            </div>
        `);
        // wrapper spans both → taller → not coincident with either child
        mockRect(dom.wrapper, rect(0, 0, 200, 80));
        mockRect(dom.c1, rect(0, 0, 200, 40));
        mockRect(dom.c2, rect(0, 40, 200, 80));

        const blocks = enumerateOuterBlocks(dom.root);
        // wrapper + c1 + c2 (all three, none coincident)
        expect(blocks).toHaveLength(3);
        expect(blocks[0]).toBe(dom.wrapper);
        expect(blocks[1]).toBe(dom.c1);
        expect(blocks[2]).toBe(dom.c2);
    });

    it('hidden outer block (zero rect) is excluded', () => {
        const dom = makeDom(`
            <p data-block-pool-id="1" data-name="visible">visible</p>
            <p data-block-pool-id="2" data-name="hidden">hidden</p>
        `);
        mockRect(dom.visible, FULL);
        mockRect(dom.hidden, ZERO);

        const blocks = enumerateOuterBlocks(dom.root);
        expect(blocks).toHaveLength(1);
        expect(blocks[0]).toBe(dom.visible);
    });

    it('DOM pre-order is preserved', () => {
        const dom = makeDom(`
            <p data-block-pool-id="1" data-name="a">A</p>
            <p data-block-pool-id="2" data-name="b">B</p>
            <p data-block-pool-id="3" data-name="c">C</p>
        `);
        mockRect(dom.a, rect(0, 0, 200, 20));
        mockRect(dom.b, rect(0, 20, 200, 40));
        mockRect(dom.c, rect(0, 40, 200, 60));

        const blocks = enumerateOuterBlocks(dom.root);
        expect(blocks.map((b) => b.getAttribute('data-block-pool-id'))).toEqual(['1', '2', '3']);
    });

    it('blockquote containing a para: only the blockquote outer block appears', () => {
        // Prefixing-atomic means the para resolves to the blockquote, so dedup
        // collapses them to one entry.
        const dom = makeDom(`
            <blockquote data-block-pool-id="1" data-name="bq">
                <p data-block-pool-id="2" data-name="para">text</p>
            </blockquote>
        `);
        mockRect(dom.bq, rect(0, 0, 200, 60));
        mockRect(dom.para, rect(20, 10, 200, 50));

        const blocks = enumerateOuterBlocks(dom.root);
        expect(blocks).toHaveLength(1);
        expect(blocks[0]).toBe(dom.bq);
    });
});

/* ─── snapshotOuterBlockGeometry (§1 geometry snapshot) ─────────────────────────
 *
 * Capture the rendered geometry of a multilevel block's whole subtree at
 * activation, keyed BLOCK-RELATIVE to the top-level block's start byte
 * (`r0 − topBlockR0`, `r1 − topBlockR0`) so the keys are shift-invariant under an
 * insert-above. jsdom can't lay out, so rects are mocked per-element; in jsdom
 * `getComputedStyle` returns empty strings → measureBlockBox's padding/border
 * terms are 0 → `contentHeight === rect.height`, which is what we assert.
 */
describe('snapshotOuterBlockGeometry', () => {
    type PoolEntry = { t: number; r: [number, number]; d: number };

    // 3-level nested bullet list mirroring the acceptance fixture:
    //   ul0 = whole list   pool[0] r=[0,69]
    //   ul1 = sub-list     pool[1] r=[20,69]
    //   ul2 = sub-sub-list pool[2] r=[39,60]
    const LIST_HTML = `
      <ul data-block-pool-id="0" data-name="ul0">
        <li>another</li>
        <li>hello
          <ul data-block-pool-id="1" data-name="ul1">
            <li>sub-item
              <ul data-block-pool-id="2" data-name="ul2">
                <li>sub-sub-item</li>
              </ul>
            </li>
            <li>nother</li>
          </ul>
        </li>
      </ul>`;
    const LIST_POOL: PoolEntry[] = [
        { t: 0, r: [0, 69], d: 0 },
        { t: 0, r: [20, 69], d: 0 },
        { t: 0, r: [39, 60], d: 0 },
    ];

    it('captures every visible nested surface, keyed block-relative, with content heights', () => {
        const dom = makeDom(LIST_HTML);
        // Distinct heights so we can assert each value maps to the right surface.
        mockRect(dom.ul0, rect(0, 0, 200, 120)); // whole list: tall
        mockRect(dom.ul1, rect(0, 0, 200, 80));  // sub-list
        mockRect(dom.ul2, rect(0, 0, 200, 40));  // sub-sub-list

        // Open from the innermost; the helper climbs to the outermost pool-id (ul0).
        const map = snapshotOuterBlockGeometry(dom.ul2, LIST_POOL, /* topBlockR0 */ 0);

        expect(map.size).toBe(3);
        // Block-relative keys (topBlockR0 = 0 → same as absolute here).
        expect(map.get('0:69')?.contentHeight).toBe(120);
        expect(map.get('20:69')?.contentHeight).toBe(80);
        expect(map.get('39:60')?.contentHeight).toBe(40);
    });

    it('keys are block-relative: a non-zero topBlockR0 shifts every key uniformly', () => {
        const dom = makeDom(LIST_HTML);
        mockRect(dom.ul0, rect(0, 0, 200, 120));
        mockRect(dom.ul1, rect(0, 0, 200, 80));
        mockRect(dom.ul2, rect(0, 0, 200, 40));

        // Pretend the whole list begins at byte 20 (an insert-above shifted it):
        // pool ranges shift too, but the helper subtracts topBlockR0, so the keys
        // are identical to the topBlockR0 = 0 case above.
        const shiftedPool: PoolEntry[] = [
            { t: 0, r: [20, 89], d: 0 },
            { t: 0, r: [40, 89], d: 0 },
            { t: 0, r: [59, 80], d: 0 },
        ];
        const map = snapshotOuterBlockGeometry(dom.ul2, shiftedPool, /* topBlockR0 */ 20);
        expect(map.size).toBe(3);
        expect(map.get('0:69')?.contentHeight).toBe(120);
        expect(map.get('20:69')?.contentHeight).toBe(80);
        expect(map.get('39:60')?.contentHeight).toBe(40);
    });

    it('returns an empty map for a flat block (≤1 visible pool-id) — no nesting to size', () => {
        const dom = makeDom(`<p data-block-pool-id="0" data-name="p0">flat</p>`);
        mockRect(dom.p0, rect(0, 0, 200, 40));
        const map = snapshotOuterBlockGeometry(dom.p0, [{ t: 0, r: [0, 5], d: 0 }], 0);
        expect(map.size).toBe(0);
    });

    it('excludes hidden (zero-rect) surfaces from the snapshot', () => {
        const dom = makeDom(LIST_HTML);
        mockRect(dom.ul0, rect(0, 0, 200, 120));
        mockRect(dom.ul1, ZERO);                 // collapsed / hidden
        mockRect(dom.ul2, rect(0, 0, 200, 40));
        const map = snapshotOuterBlockGeometry(dom.ul2, LIST_POOL, 0);
        expect(map.size).toBe(2);
        expect(map.has('0:69')).toBe(true);
        expect(map.has('20:69')).toBe(false);    // hidden ul1 excluded
        expect(map.has('39:60')).toBe(true);
    });

    it('full (r0,r1) key disambiguates a container and a first child sharing r0 (r0-only would collide)', () => {
        // Div [0,30] whose FIRST child Para starts at the Div's own r0 (0). An
        // r0-only key ("0") would collide; the full key ("0:30" vs "0:15") does not.
        const dom = makeDom(`
          <div data-block-pool-id="0" data-name="div0">
            <p data-block-pool-id="1" data-name="p1">child</p>
          </div>`);
        mockRect(dom.div0, rect(0, 0, 200, 60));
        mockRect(dom.p1, rect(0, 0, 200, 30));
        const pool: PoolEntry[] = [
            { t: 0, r: [0, 30], d: 0 },
            { t: 0, r: [0, 15], d: 0 },
        ];
        const map = snapshotOuterBlockGeometry(dom.p1, pool, 0);
        expect(map.size).toBe(2);                // no collision
        expect(map.get('0:30')?.contentHeight).toBe(60);
        expect(map.get('0:15')?.contentHeight).toBe(30);
    });
});
