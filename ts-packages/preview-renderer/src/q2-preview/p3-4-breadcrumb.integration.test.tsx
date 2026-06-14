/**
 * P3.4 §3d integration tests: BreadcrumbChip floating chip — breadcrumb
 * rendering, ◀/▶ depth navigation, and crumb-click jump-to-depth.
 *
 * All tests drive the REAL PreviewRoot (no custom registry) so real Div/Para
 * components render nested [data-block-pool-id] elements and the real chip
 * renders. The REAL context callbacks are driven via the REAL chip
 * buttons/crumbs. No re-target logic is re-implemented here.
 *
 * Fixture:
 * ────────
 * content = '::: d\nAAA\n\nBBB\n:::\npara2\n'  (25 bytes)
 *
 * Byte map:
 *   0 :  1 :  2 :  3 ␠  4 d   5 \n
 *   6 A  7 A  8 A  9 \n 10 \n
 *  11 B 12 B 13 B 14 \n
 *  15 : 16 : 17 : 18 \n
 *  19 p 20 a 21 r 22 a 23 2 24 \n
 *
 * pool:
 *   pool[0] Div        {t:0, r:[0,18],  d:0}   siKey '0:0-18:0'   slice '::: d\nAAA\n\nBBB\n:::'
 *   pool[1] ParaA      {t:0, r:[6,9],   d:0}   siKey '0:6-9:0'    slice 'AAA'
 *   pool[2] ParaB      {t:0, r:[11,14], d:0}   siKey '0:11-14:0'  slice 'BBB'
 *   pool[3] para2      {t:0, r:[19,24], d:0}   siKey '0:19-24:0'  slice 'para2'
 *
 * Ancestor path for cursor=(11,14) [ParaB]:
 *   Div.d  (r=[0,18], isCurrent=false)
 *   Para   (r=[11,14], isCurrent=true)
 *
 * Event-isolation deferral (→ P3.5 Playwright):
 *   The rigorous stopPropagation / preventDefault behaviour for chip buttons
 *   (preventing host-level click-switch and blur-commit) depends on real browser
 *   pointer-event sequencing, real focus/blur, and real DOM hit-testing — none of
 *   which jsdom simulates. fireEvent.click fires no pointer events and does not
 *   move focus. Therefore NO jsdom test asserts "chip click does not switch
 *   blocks" (that assertion would be vacuous / not fail-on-revert). The production
 *   code DOES implement stopPropagation/preventDefault correctly for the real
 *   browser. Pointer-isolation testing is deferred to P3.5 Playwright.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import { detectPlatform } from './depthNav';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── Platform detection ──────────────────────────────────────────────────── */
// (detectPlatform used in tests 5 for the tooltip labels; not needed for click
// tests but kept for symmetry with p3-3-depth)
const _PLATFORM = detectPlatform();
void _PLATFORM; // suppress unused-var lint

/* ─── PointerEvent helper (verbatim from p3-3-depth) ─────────────────────── */
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

/* ─── Fixture ─────────────────────────────────────────────────────────────── */

// content bytes (length 25):
// :::␠d\nAAA\n\nBBB\n:::\npara2\n
const CONTENT = '::: d\nAAA\n\nBBB\n:::\npara2\n';

const POOL = [
    { t: 0, r: [0, 18], d: 0 },    // pool[0] Div        siKey 0:0-18:0
    { t: 0, r: [6, 9], d: 0 },     // pool[1] ParaA      siKey 0:6-9:0
    { t: 0, r: [11, 14], d: 0 },   // pool[2] ParaB      siKey 0:11-14:0
    { t: 0, r: [19, 24], d: 0 },   // pool[3] para2      siKey 0:19-24:0
];

const DIV_SLICE = '::: d\nAAA\n\nBBB\n:::';

function makeAstJson(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'Div',
                c: [
                    ['', ['d'], []],  // Attr: class "d" → label "Div.d"
                    [
                        { t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 },
                        { t: 'Para', c: [{ t: 'Str', c: 'BBB' }], s: 2 },
                    ],
                ],
                s: 0,
            },
            { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 3 },
        ],
        astContext: { p: POOL },
    });
}

function mountFixture(opts: { setAst?: any; unlockDepthCursor?: boolean } = {}) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockDepthCursor: opts.unlockDepthCursor,
        onNavigateToDocument: () => {},
    };
    return { ...render(<PreviewRoot {...props} />), setAst };
}

/** Open the editor on a given pool-id via real pointer events. */
async function openEditor(container: HTMLElement, poolId: string) {
    const el = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`)!;
    await act(async () => {
        fireEvent(el, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(el, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
}

/** Mock getBoundingClientRect on all [data-block-pool-id] tiles. */
function mockTileRects(container: HTMLElement) {
    const tiles = container.querySelectorAll<HTMLElement>('[data-block-pool-id]');
    tiles.forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 80, right: 300, bottom: pid * 80 + 60,
            width: 300, height: 60, x: 0, y: pid * 80, toJSON: () => ({}),
        } as DOMRect);
    });
}

const chip = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');
const ta = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea');

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 1 ★ (fail-on-revert): Chip hidden when LOCKED
 *
 * unlockDepthCursor is NOT set; open editor on ParaB (pool-id=2).
 * Editor opens (textarea appears) but chip does NOT appear.
 *
 * FAIL-ON-REVERT: removing the unlockDepthCursor gate in BreadcrumbChip
 * makes the chip appear when locked → chip(container) !== null → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 1 ★ — chip hidden when locked', () => {
    it('chip does not render when unlockDepthCursor is not set', async () => {
        const { container } = mountFixture({}); // no unlockDepthCursor
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB

        expect(ta(container)).not.toBeNull(); // editor IS open
        expect(chip(container)).toBeNull();   // chip is NOT shown
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 2: Chip hidden when NO editor open
 *
 * unlockDepthCursor=true but no editor open → chip null.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 2 — chip hidden when no editor open', () => {
    it('chip does not render when editTarget is null', async () => {
        const { container } = mountFixture({ unlockDepthCursor: true });
        await act(async () => {});

        expect(chip(container)).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 3: Chip shown when unlocked + editor open
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 3 — chip shown when unlocked + editor open', () => {
    it('chip renders when unlockDepthCursor=true and editor is open', async () => {
        const { container } = mountFixture({ unlockDepthCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB

        expect(chip(container)).not.toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 4: Ancestor path rendered
 *
 * unlocked, open ParaB (cursor=(11,14)).
 * Chip crumb buttons (.q2-crumb) must read ['Div.d', 'Para'] in order.
 * Last crumb (Para) has aria-current="true".
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 4 — ancestor path rendered in chip', () => {
    it('crumb buttons show [Div.d, Para] with Para as aria-current', async () => {
        const { container } = mountFixture({ unlockDepthCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — cursor=(11,14)

        const chipEl = chip(container);
        expect(chipEl).not.toBeNull();

        const crumbs = Array.from(chipEl!.querySelectorAll<HTMLElement>('.q2-crumb'));
        expect(crumbs.map(c => c.textContent)).toEqual(['Div.d', 'Para']);

        // Current node (Para) carries aria-current="true"
        expect(crumbs[1].getAttribute('aria-current')).toBe('true');
        // Non-current node (Div.d) must NOT have aria-current
        expect(crumbs[0].getAttribute('aria-current')).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 5 ★ (fail-on-revert): ◀ / ▶ buttons move depth
 *
 * unlocked, open ParaB (value 'BBB').
 * Click ◀ (q2-breadcrumb-out) → editor re-targets to Div → value = DIV_SLICE.
 * Click ▶ (q2-breadcrumb-in) → editor descends back to ParaB → value = 'BBB'.
 * setAst NOT called.
 *
 * FAIL-ON-REVERT: neutralize the ◀ onClick (or requestDepthMove) → editor stays
 * at 'BBB' → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 5 ★ — ◀/▶ buttons move depth', () => {
    it('out button moves to Div, in button moves back to ParaB', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst, unlockDepthCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — value 'BBB'
        expect(ta(container)!.value).toBe('BBB');

        // Click ◀ (depth out: ParaB → Div)
        const outBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-out')!;
        await act(async () => { fireEvent.click(outBtn); });
        mockTileRects(container);

        expect(ta(container)!.value).toBe(DIV_SLICE);

        // Click ▶ (depth in: Div → ParaB, toward leafAnchorR0=11)
        const inBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-in')!;
        await act(async () => { fireEvent.click(inBtn); });
        mockTileRects(container);

        expect(ta(container)!.value).toBe('BBB');

        // Depth moves must NOT commit
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 6 ★ (fail-on-revert): Crumb-click jumps + leafAnchorR0 unchanged
 *
 * unlocked, open ParaB (value 'BBB', leafAnchorR0=11).
 * Click the 'Div.d' crumb → requestDepthSelect(0,18) → value = DIV_SLICE.
 * Click ▶ → because leafAnchorR0 is still 11, descends to ParaB → value 'BBB'.
 * setAst NOT called.
 *
 * FAIL-ON-REVERT: if requestDepthSelect wrongly resets leafAnchorR0 to r0 (0),
 * the ▶ press descends toward r0=0 → hits ParaA (r=[6,9]) → value 'AAA' ≠ 'BBB'
 * → test fails RED. See the explicit probe in the report.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 6 ★ — crumb-click jump preserves leafAnchorR0', () => {
    it('jump to Div.d then ▶ still descends to ParaB (not ParaA)', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst, unlockDepthCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — value 'BBB', leafAnchorR0=11
        expect(ta(container)!.value).toBe('BBB');

        // Click the 'Div.d' crumb → jump to (0,18)
        const chipEl = chip(container)!;
        const divCrumb = Array.from(chipEl.querySelectorAll<HTMLElement>('.q2-crumb'))
            .find(c => c.textContent === 'Div.d')!;
        expect(divCrumb).not.toBeUndefined();

        await act(async () => { fireEvent.click(divCrumb); });
        mockTileRects(container);
        expect(ta(container)!.value).toBe(DIV_SLICE);

        // Click ▶ — must descend to ParaB (leafAnchorR0=11), not ParaA (r0=6)
        const inBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-in')!;
        await act(async () => { fireEvent.click(inBtn); });
        mockTileRects(container);
        expect(ta(container)!.value).toBe('BBB');

        // Jump must NOT commit
        expect(setAst).not.toHaveBeenCalled();
    });
});
