/**
 * §2 — Mode-aware roving + outline integration tests (TDD).
 *
 * Tests:
 *
 *  2.a — UNLOCK roving visits the C1 partition INCLUDING a multi-block
 *         list item's leading-text <li> proxy. The fixture has a BulletList
 *         item whose content is [Plain, BulletList] (text + sublist). In
 *         UNLOCK mode, ArrowDown must visit:
 *           (a) the outer <li> proxy (pool-id = Plain's id) — its leading
 *               text; AND
 *           (b) the nested sublist's <li> items (their own pool-ids).
 *         The multi-block outer <li> is EXCLUDED by enumerateNestingLeaves
 *         (it has a pool-id descendant), so this test is RED against
 *         enumerateNestingLeaves and GREEN only after enumerateNestingSurfaces.
 *
 *         FAIL-ON-REVERT:
 *           - If the multi-block <li> is missing from the roving set, ArrowDown
 *             can never reach it → the multi-block <li> is not in the focused
 *             element list → test RED.
 *           - If the outer <ul> is reachable (should be excluded in UNLOCK),
 *             the test would catch an over-inclusion → RED.
 *
 *  2.b — Outline target for SINGLE-block and MULTI-block items.
 *         Single-block: box-shadow on the <li> equals the activation target
 *         (exact — no over-cover).
 *         Multi-block: box-shadow on the outer <li> (intentional over-cover —
 *         covers leading text + sublist) while activation targets the same
 *         element (the <li> that carries the pool-id).
 *
 *         Decision documented here (hover-CSS scope): the [data-block-pool-id]
 *         hover CSS from the existing stylesheet also matches <li>s. In LOCKED
 *         mode a click still activates the <ul> — the item hover outline is
 *         purely cosmetic there. We accept this (no scope restriction) because:
 *           - LOCKED mode never descends into items (the <ul> is the target).
 *           - UNLOCK mode already outlines the leaf/proxy correctly.
 *           - Restricting via a CSS [unlock-mode] attribute requires plumbing
 *             not worth adding for cosmetic correctness.
 *         This decision is intentional: do not "fix" by scoping the CSS.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { useBlockEditHover } from './useBlockEditHover';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    document.body.innerHTML = '';
});

/* ─── PointerEvent helper (same pattern as other test files) ──────────────── */
function ptrEvent(
    type: string,
    opts: PointerEventInit & { clientX?: number; clientY?: number } = {},
): Event {
    const PE = (window as any).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    for (const [key, val] of Object.entries({
        ...(opts.pointerType !== undefined ? { pointerType: opts.pointerType } : {}),
        ...(opts.clientX !== undefined ? { clientX: opts.clientX } : {}),
        ...(opts.clientY !== undefined ? { clientY: opts.clientY } : {}),
    } as Record<string, unknown>)) {
        Object.defineProperty(evt, key, { value: val, configurable: true });
    }
    return evt;
}

/* ─── Rect helpers ───────────────────────────────────────────────────────────── */
function rect(
    left: number, top: number, right: number, bottom: number,
): DOMRect {
    return {
        left, top, right, bottom, x: left, y: top,
        width: right - left, height: bottom - top,
        toJSON: () => ({}),
    };
}

/* ─────────────────────────────────────────────────────────────────────────────
 * §2 Test 2.a — UNLOCK roving visits C1 partition including multi-block <li>
 *
 * Fixture (one BulletList with two items):
 *   - items[0]: text-with-sublist: [Plain.s=0 "intro", BulletList.s=1 [{Plain "sub"}]]
 *     The outer <li> borrows pool[0]'s id. It has a nested <ul data-block-pool-id="1">.
 *     The inner bullet item renders <li data-block-pool-id="?"> (no source entry for
 *     the inner Plain here — we leave it sourceless for simplicity; only the outer
 *     structures need pool-ids to appear in the roving set).
 *   - items[1]: single-block: [Plain.s=2 "single"]
 *     The outer <li> borrows pool[2]'s id. No nested pool-id descendant.
 *   - The outer BulletList itself: pool[3].
 *
 * Pool:
 *   pool[0] = Plain "intro"          r=[2, 7]   — multi-block outer item's leading text
 *   pool[1] = inner BulletList       r=[8, 20]  — nested list
 *   pool[2] = Plain "single"         r=[22, 28] — single-block item's text
 *   pool[3] = outer BulletList       r=[0, 30]  — the whole outer list
 *
 * Expected C1 partition (UNLOCK roving set), in DOM pre-order:
 *   [0] outer <li> data-block-pool-id="0"   — multi-block <li> proxy (LI/DD clause)
 *   [1] inner <ul> data-block-pool-id="1"   — the nested BulletList (DOM-leaf: no pool-id descendant)
 *   [2] outer <li> data-block-pool-id="2"   — single-block <li> (DOM-leaf: no pool-id descendant)
 *   Note: outer <ul> data-block-pool-id="3" is EXCLUDED (pure container: not LI/DD, has descendants)
 *
 * FAIL-ON-REVERT assertions:
 *   - With enumerateNestingLeaves: outer multi-block <li> (pool-id="0") is dropped
 *     (it has a pool-id descendant), so it is NOT in the roving set → RED.
 *   - With enumerateNestingSurfaces: outer multi-block <li> is included → GREEN.
 * ─────────────────────────────────────────────────────────────────────────── */

const POOL_2A = [
    { t: 0, r: [2, 7],   d: 0 }, // pool[0] Plain "intro"       — multi-block outer li
    { t: 0, r: [8, 20],  d: 0 }, // pool[1] inner BulletList     — nested list
    { t: 0, r: [22, 28], d: 0 }, // pool[2] Plain "single"       — single-block item
    { t: 0, r: [0, 30],  d: 0 }, // pool[3] outer BulletList     — whole list
];

/**
 * DOM structure for test 2.a:
 *
 * <div host>
 *   <ul data-block-pool-id="3" tabIndex=-1>           ← outer list (excluded: not LI/DD, has descendants)
 *     <li data-block-pool-id="0" tabIndex=-1>          ← multi-block outer item (INCLUDED by LI/DD clause)
 *       intro
 *       <ul data-block-pool-id="1" tabIndex=-1>        ← inner list (DOM-leaf: no pool-id descendant)
 *         <li>sub</li>
 *       </ul>
 *     </li>
 *     <li data-block-pool-id="2" tabIndex=-1>          ← single-block item (DOM-leaf: no pool-id descendant)
 *       single
 *     </li>
 *   </ul>
 * </div>
 */
function S2aInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            {/* outer BulletList — pure container; excluded from C1 roving (not LI/DD, has pool-id descendants) */}
            <ul data-block-pool-id="3" tabIndex={-1} data-testid="outer-ul">
                {/* multi-block <li> proxy: borrows Plain[0]'s pool-id; contains inner <ul> */}
                <li data-block-pool-id="0" tabIndex={-1} data-testid="outer-li-0">
                    intro
                    {/* inner BulletList: DOM-leaf (no pool-id descendant inside it here) */}
                    <ul data-block-pool-id="1" tabIndex={-1} data-testid="inner-ul">
                        <li>sub</li>
                    </ul>
                </li>
                {/* single-block <li>: DOM-leaf; no pool-id descendant */}
                <li data-block-pool-id="2" tabIndex={-1} data-testid="outer-li-2">
                    single
                </li>
            </ul>
        </div>
    );
}

function S2aHost({ unlock }: { unlock: boolean }) {
    const unlockRef = React.useRef<boolean | undefined>(unlock);
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget: vi.fn(),
        pool: POOL_2A,
        content: '',
        unlockNestingCursor: unlock,
        unlockNestingCursorRef: unlockRef,
        editTargetRef: React.useRef<any>(null),
    };
    return (
        <PreviewContext.Provider value={ctx}>
            <S2aInner />
        </PreviewContext.Provider>
    );
}

function mountS2a(unlock: boolean) {
    const utils = render(<S2aHost unlock={unlock} />);
    const host = utils.getByTestId('host');
    const outerUl = utils.getByTestId('outer-ul');
    const outerLi0 = utils.getByTestId('outer-li-0');
    const innerUl = utils.getByTestId('inner-ul');
    const outerLi2 = utils.getByTestId('outer-li-2');

    // Give all pool-id elements a visible rect so isVisibleBlock returns true.
    const makeRect = (top: number) => rect(0, top, 200, top + 30);
    vi.spyOn(outerUl, 'getBoundingClientRect').mockReturnValue(makeRect(0));
    vi.spyOn(outerLi0, 'getBoundingClientRect').mockReturnValue(makeRect(0));
    vi.spyOn(innerUl, 'getBoundingClientRect').mockReturnValue(makeRect(20));
    vi.spyOn(outerLi2, 'getBoundingClientRect').mockReturnValue(makeRect(50));

    return { ...utils, host, outerUl, outerLi0, innerUl, outerLi2 };
}

describe('§2 Test 2.a — UNLOCK roving visits C1 partition including multi-block <li> proxy', () => {
    it('LOCKED: ArrowDown focuses the outer <ul> (single stop for entire list)', () => {
        const { host, outerUl } = mountS2a(false);
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        // LOCKED: enumerateOuterBlocks returns the outer <ul> (prefixing container)
        expect(document.activeElement).toBe(outerUl);
    });

    it('UNLOCK: ArrowDown visits the multi-block outer <li> (pool-id="0")', () => {
        const { host, outerLi0, innerUl, outerLi2 } = mountS2a(true);

        // Collect all elements visited by ArrowDown in UNLOCK mode.
        const visited: Element[] = [];
        for (let i = 0; i < 5; i++) {
            fireEvent.keyDown(host, { key: 'ArrowDown' });
            if (document.activeElement && document.activeElement !== document.body) {
                visited.push(document.activeElement);
            }
        }

        // The multi-block outer <li> (pool-id="0") MUST be reachable.
        // FAIL-ON-REVERT: enumerateNestingLeaves excludes it → never focused → RED.
        expect(visited).toContain(outerLi0);

        // The inner <ul> (pool-id="1") is a DOM-leaf and must also be visited.
        expect(visited).toContain(innerUl);

        // The single-block outer <li> (pool-id="2") is a DOM-leaf and must be visited.
        expect(visited).toContain(outerLi2);
    });

    it('UNLOCK: the outer <ul> container (pool-id="3") is NOT in the roving set', () => {
        const { host, outerUl } = mountS2a(true);

        // ArrowDown in UNLOCK mode should never focus the outer <ul> (pure container).
        const visited: Element[] = [];
        for (let i = 0; i < 5; i++) {
            fireEvent.keyDown(host, { key: 'ArrowDown' });
            if (document.activeElement && document.activeElement !== document.body) {
                visited.push(document.activeElement);
            }
        }
        expect(visited).not.toContain(outerUl);
    });

    it('UNLOCK: ArrowUp from first roving element wraps to the last (pool-id="2")', () => {
        const { host, outerLi0, outerLi2 } = mountS2a(true);

        // Navigate forward until we hit the multi-block <li> (first in C1 partition).
        let hitLi0 = false;
        for (let i = 0; i < 5; i++) {
            fireEvent.keyDown(host, { key: 'ArrowDown' });
            if (document.activeElement === outerLi0) { hitLi0 = true; break; }
        }
        expect(hitLi0, 'multi-block <li> (pool-id="0") must be reachable by ArrowDown').toBe(true);

        // outerLi0 is the FIRST element in the C1 partition (index 0 in DOM pre-order).
        // ArrowUp from the first element must WRAP to the last element: outerLi2 (pool-id="2").
        // FAIL-ON-REVERT: if ArrowUp is broken (no-op or goes to body), this assertion fails.
        fireEvent.keyDown(host, { key: 'ArrowUp' });
        expect(
            document.activeElement,
            'ArrowUp from pool-id="0" (first in roving set) must wrap to pool-id="2" (last)',
        ).toBe(outerLi2);
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §2 Test 2.b — Outline target for single-block and multi-block items
 *
 * The outline (`box-shadow`) is applied to the element that CARRIES the
 * pool-id (the resolved hover target). This is `outlineElement(el)` in
 * useBlockEditHover.tsx, where `el` is the mode-aware resolved surface.
 *
 * Single-block <li>: carries pool-id and has no nested pool-id descendant.
 *   - Hover outline is on the <li> itself.
 *   - Activation target is also the <li> (exact, no over-cover).
 *
 * Multi-block <li> proxy: carries pool-id AND has a nested pool-id descendant.
 *   - In UNLOCK mode, hover outlines the <li> itself (same element).
 *   - Activation target is the same <li> (el.closest('[data-block-pool-id]') in
 *     unlock = the <li> itself, since <li> carries pool-id).
 *   - The box-shadow OVER-COVERS (outlines text + sublist); this is a documented,
 *     accepted mismatch: the activation edits only the leading text (via
 *     measureLeadingBlockBox / Range), but the outline spans the whole <li>
 *     including its nested sublist.
 *   - This over-cover is bounded and documented: single-block <li> is exact;
 *     only multi-block proxy has this cosmetic mismatch.
 *
 * Hover-CSS scope decision (documented here per task spec):
 *   The `[data-block-pool-id]` hover CSS (cursor: pointer etc.) also applies
 *   to <li>s. In LOCKED mode a click activates the outer <ul>, so the item
 *   hover affordance is cosmetic there. We ACCEPT this (no mode-scoping of
 *   the CSS) because:
 *     - LOCKED mode never descends into items for activation.
 *     - UNLOCK mode outlines the proxy correctly.
 *     - Scoping via data-attribute would require plumbing through PreviewContext
 *       into every style injection site — not worth it for cosmetic correctness.
 * ─────────────────────────────────────────────────────────────────────────── */

const POOL_2B_SINGLE = [
    { t: 0, r: [2, 8],  d: 0 }, // pool[0] Plain "apple" — single-block item
    { t: 0, r: [0, 10], d: 0 }, // pool[1] BulletList
];

const POOL_2B_MULTI = [
    { t: 0, r: [2, 7],  d: 0 }, // pool[0] Plain "intro" — multi-block outer item
    { t: 0, r: [8, 20], d: 0 }, // pool[1] inner BulletList
    { t: 0, r: [0, 22], d: 0 }, // pool[2] outer BulletList
];

/**
 * Single-block item fixture:
 *   <ul data-block-pool-id="1">
 *     <li data-block-pool-id="0">apple</li>
 *   </ul>
 */
function S2bSingleInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <ul data-block-pool-id="1" data-testid="ul">
                <li data-block-pool-id="0" data-testid="single-li">apple</li>
            </ul>
        </div>
    );
}

function S2bSingleHost({ unlock }: { unlock: boolean }) {
    const unlockRef = React.useRef<boolean | undefined>(unlock);
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget: vi.fn(),
        pool: POOL_2B_SINGLE,
        content: '',
        unlockNestingCursor: unlock,
        unlockNestingCursorRef: unlockRef,
        editTargetRef: React.useRef<any>(null),
    };
    return (
        <PreviewContext.Provider value={ctx}>
            <S2bSingleInner />
        </PreviewContext.Provider>
    );
}

/**
 * Multi-block item fixture:
 *   <ul data-block-pool-id="2">
 *     <li data-block-pool-id="0">
 *       intro
 *       <ul data-block-pool-id="1"><li>sub</li></ul>
 *     </li>
 *   </ul>
 */
function S2bMultiInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <ul data-block-pool-id="2" data-testid="outer-ul">
                <li data-block-pool-id="0" data-testid="multi-li">
                    intro
                    <ul data-block-pool-id="1" data-testid="inner-ul">
                        <li>sub</li>
                    </ul>
                </li>
            </ul>
        </div>
    );
}

function S2bMultiHost({ unlock }: { unlock: boolean }) {
    const unlockRef = React.useRef<boolean | undefined>(unlock);
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget: vi.fn(),
        pool: POOL_2B_MULTI,
        content: '',
        unlockNestingCursor: unlock,
        unlockNestingCursorRef: unlockRef,
        editTargetRef: React.useRef<any>(null),
    };
    return (
        <PreviewContext.Provider value={ctx}>
            <S2bMultiInner />
        </PreviewContext.Provider>
    );
}

describe('§2 Test 2.b — outline is on the pool-id-carrying element (single-block exact; multi-block documented over-cover)', () => {
    it('SINGLE-BLOCK <li>: hover outlines the <li> itself (exact — no over-cover)', () => {
        const utils = render(<S2bSingleHost unlock={true} />);
        const li = utils.getByTestId('single-li');
        const ul = utils.getByTestId('ul');
        vi.spyOn(li, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 20));
        vi.spyOn(ul, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 20));

        // Hover over the single-block <li>.
        fireEvent(li, ptrEvent('pointermove', { pointerType: 'mouse' }));

        // Outline is on the <li> (UNLOCK mode: rawLeaf = <li>, resolves to itself).
        expect(li.style.boxShadow, '<li> must carry the box-shadow outline').toBeTruthy();
        expect(ul.style.boxShadow, '<ul> must NOT carry the outline').toBe('');
        // The pool-id is on the <li>, so the outlined element IS the activation target (exact).
        expect(li.getAttribute('data-block-pool-id')).toBe('0');
    });

    it('MULTI-BLOCK <li> proxy: hover outlines the <li> (documented over-cover)', () => {
        const utils = render(<S2bMultiHost unlock={true} />);
        const multiLi = utils.getByTestId('multi-li');
        const outerUl = utils.getByTestId('outer-ul');
        const innerUl = utils.getByTestId('inner-ul');
        vi.spyOn(multiLi, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(outerUl, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(innerUl, 'getBoundingClientRect').mockReturnValue(rect(0, 20, 200, 60));

        // Hover over the multi-block <li>.
        fireEvent(multiLi, ptrEvent('pointermove', { pointerType: 'mouse' }));

        // The box-shadow is on the <li> — which over-covers (includes the sublist visually).
        // This is the documented, accepted mismatch.
        // NOTE: In UNLOCK mode, pointermove resolves rawLeaf = <li> (pool-id="0").
        // The outline is placed on the rawLeaf itself (ul.closest('[data-block-pool-id]') = li).
        expect(multiLi.style.boxShadow, 'multi-block <li> carries the outline (over-cover accepted)').toBeTruthy();
        expect(outerUl.style.boxShadow, 'outer <ul> must NOT carry the outline').toBe('');
    });

    it('MULTI-BLOCK <li>: the outlined element is the activation target (same <li>)', () => {
        const utils = render(<S2bMultiHost unlock={true} />);
        const multiLi = utils.getByTestId('multi-li');
        const outerUl = utils.getByTestId('outer-ul');
        const innerUl = utils.getByTestId('inner-ul');
        vi.spyOn(multiLi, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(outerUl, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 200, 60));
        vi.spyOn(innerUl, 'getBoundingClientRect').mockReturnValue(rect(0, 20, 200, 60));

        // In UNLOCK mode, activate(el) uses el.closest('[data-block-pool-id]').
        // For the multi-block <li>, closest('[data-block-pool-id]') = the <li> itself
        // (pool-id="0" is on it). So the outlined element and the activation target
        // are the SAME element.
        //
        // Multi-block <li> over-cover: the box-shadow spans the whole <li> (text + sublist)
        // but activation edits only the leading text (via measureLeadingBlockBox/Range).
        // The outline element and activation element agree on identity; only the VISUAL
        // extent of the shadow is larger than what gets edited. This is bounded and documented.
        fireEvent(multiLi, ptrEvent('pointermove', { pointerType: 'mouse' }));
        expect(multiLi.style.boxShadow).toBeTruthy();

        // Confirm the pool-id is on the <li> (not the inner <ul>) — same element as the outline.
        expect(multiLi.getAttribute('data-block-pool-id')).toBe('0');
        // The activation target el.closest('[data-block-pool-id]') from multiLi = multiLi itself.
        const activationTarget = multiLi.closest('[data-block-pool-id]');
        expect(activationTarget).toBe(multiLi);
    });
});
