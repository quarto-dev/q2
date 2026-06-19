/**
 * P3.3 §3b integration tests: unlocked sub-clause behaviors.
 *
 * Two regression back-fill tests that pin already-shipped behaviors:
 *
 *  Test 1 ★ (fail-on-revert): click-outside-resets-to-leaf (unlocked)
 *    With an editor open (unlockNestingCursor=true), a mouse click on a *different*
 *    nested block switches the editor to that block's LEAF — not to its prefixing
 *    container (which is what LOCKED mode would pick).
 *
 *  Test 2 ★ (fail-on-revert): ancestor-only change re-derives the breadcrumb path,
 *    cursor unchanged.
 *    An external re-render that changes only an ancestor's label (AST attr), with
 *    the cursor node's byte range held fixed, keeps the editor open (self-heal KEEP)
 *    AND the breadcrumb chip re-derives its labels from the new AST.
 *
 * Both tests drive the REAL PreviewRoot + real useBlockEditHover click path + real
 * BreadcrumbChip. The resolution / ancestor-path logic is never re-implemented here.
 *
 * Fail-on-revert probes:
 *  Test 1: forcing the locked branch (`resolveOuterBlock`) in activate() makes the
 *    editor open on the whole BlockQuote → textarea.value === '> BBB', not 'BBB'.
 *  Test 2: memoizing buildAncestorPath on [anchorR0, anchorR1] (stable across the
 *    ancestor-only change) causes the chip to show stale 'Div.a' labels after the
 *    re-render where the class becomes 'b'.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import {
    render,
    cleanup,
    act,
    fireEvent,
} from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── PointerEvent helper (verbatim from p3-4-breadcrumb / p2-4d) ───────────── */
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

/** Mock getBoundingClientRect on all [data-block-pool-id] tiles with distinct non-zero rects. */
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

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 1 ★ (fail-on-revert): click-outside-resets-to-leaf (unlocked)
 *
 * Fixture: two single-line blockquotes
 *
 * content (13 bytes): "> AAA\n\n> BBB\n"
 *  >0 ' '1 A2 A3 A4 \n5  \n6  >7 ' '8 B9 B10 B11 \n12
 *
 * pool:
 *   pool[0] BQ1   r=[0,5]   "> AAA"   (blockquote wrapper)
 *   pool[1] ParaA r=[2,5]   "AAA"     (leaf paragraph inside BQ1)
 *   pool[2] BQ2   r=[7,12]  "> BBB"   (blockquote wrapper)
 *   pool[3] ParaB r=[9,12]  "BBB"     (leaf paragraph inside BQ2)
 *
 * In LOCKED mode, clicking ParaB would resolve to the containing BQ2 tile
 * (resolveOuterBlock collapses to outermost prefixing container → anchorSlice='> BBB').
 * In UNLOCKED mode, activate() uses el.closest('[data-block-pool-id]') → the leaf
 * (ParaB, anchorSlice='BBB'). This test pins the UNLOCKED behavior.
 *
 * FAIL-ON-REVERT: forcing the locked branch in activate() (always use resolveOuterBlock)
 * makes textarea.value === '> BBB' instead of 'BBB' → test RED.
 * ─────────────────────────────────────────────────────────────────────────── */

// content (13 bytes): "> AAA\n\n> BBB\n"
const CONTENT_1 = '> AAA\n\n> BBB\n';
const POOL_1 = [
    { t: 0, r: [0, 5],  d: 0 }, // pool[0] BQ1   slice "> AAA"
    { t: 0, r: [2, 5],  d: 0 }, // pool[1] ParaA slice "AAA"
    { t: 0, r: [7, 12], d: 0 }, // pool[2] BQ2   slice "> BBB"
    { t: 0, r: [9, 12], d: 0 }, // pool[3] ParaB slice "BBB"
];

function makeAst1(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0], meta: {},
        blocks: [
            { t: 'BlockQuote', c: [{ t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 }], s: 0 },
            { t: 'BlockQuote', c: [{ t: 'Para', c: [{ t: 'Str', c: 'BBB' }], s: 3 }], s: 2 },
        ],
        astContext: { p: POOL_1 },
    });
}

describe('P3.3 §3b test 1 ★ — click-outside-resets-to-leaf (unlocked)', () => {
    it('click on a different nested block opens leaf, not BlockQuote container', async () => {
        const setAst = vi.fn();
        const astJson = makeAst1();
        const props: PreviewRootProps = {
            astJson,
            untransformedAstJson: astJson,
            renderedContent: CONTENT_1,
            currentFilePath: '/test.qmd',
            assetManifest: {},
            setAst,
            unlockNestingCursor: true,
            onNavigateToDocument: () => {},
        };

        const { container } = render(<PreviewRoot {...props} />);

        // Step 0: settle initial render, mock tile rects.
        await act(async () => {});
        mockTileRects(container);

        // Step 1: open ParaA (pool-id=1) via mouse click (unlocked → leaf).
        const tileParaA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileParaA, 'pool-id=1 (ParaA) should be in DOM').not.toBeNull();
        await act(async () => {
            fireEvent(tileParaA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileParaA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea, 'editor should open on ParaA').not.toBeNull();
        // In unlocked mode, the editor opens on the LEAF (ParaA → 'AAA').
        // In locked mode it would resolve to the containing BQ1 → '> AAA'.
        expect(textarea!.value).toBe('AAA');

        // Step 2: click-switch to ParaB (pool-id=3) — unmodified path (no typing).
        // Follow the unmodified click-switch sequence from p2-4d.integration.test.tsx:
        //   pointerdown on target → blur on textarea → pointerup on target.
        const tileParaB = container.querySelector<HTMLElement>('[data-block-pool-id="3"]');
        expect(tileParaB, 'pool-id=3 (ParaB) should be in DOM').not.toBeNull();
        mockTileRects(container);

        await act(async () => {
            fireEvent(tileParaB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });
        await act(async () => {
            fireEvent.blur(textarea!);
        });
        await act(async () => {
            fireEvent(tileParaB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // Step 3: re-query textarea. Must be open on ParaB's LEAF ('BBB').
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea, 'editor should switch to ParaB').not.toBeNull();
        // UNLOCKED → leaf (ParaB, anchorSlice='BBB').
        // LOCKED    → resolveOuterBlock would give BQ2, anchorSlice='> BBB'.
        expect(textarea!.value).toBe('BBB');

        // Unmodified click-switch: no commit.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 2 ★ (fail-on-revert): ancestor-only change re-derives the path, cursor unchanged
 *
 * Fixture: fenced Div with one single-line child.
 *
 * content (14 bytes): "::: a\nAAA\n:::\n"
 *  :0 :1 :2 ' '3 a4 \n5  A6 A7 A8 \n9  :10 :11 :12 \n13
 *
 * pool:
 *   pool[0] Div   r=[0,12]  "::: a\nAAA\n:::"   (fenced div wrapper)
 *   pool[1] child r=[6,9]   "AAA"               (leaf Para inside Div)
 *
 * Between first and second render, ONLY the Div's class changes: 'a' → 'b'.
 * The child's byte range [6,9] is IDENTICAL → self-heal KEEP (editor stays open).
 * But the ancestor label changes: chip crumbs must re-derive from the new AST.
 *
 * FAIL-ON-REVERT: memoizing buildAncestorPath on [anchorR0, anchorR1] (stable
 * across this ancestor-only change) causes the chip to show stale 'Div.a' after
 * the re-render → chip crumbs remain ['Div.a', 'Para'] → step 5 asserts RED.
 * ─────────────────────────────────────────────────────────────────────────── */

// content (14 bytes): "::: a\nAAA\n:::\n"
const CONTENT_2 = '::: a\nAAA\n:::\n';
const POOL_2 = [
    { t: 0, r: [0, 12], d: 0 }, // pool[0] Div   slice "::: a\nAAA\n:::"
    { t: 0, r: [6, 9],  d: 0 }, // pool[1] child slice "AAA"
];

function makeAst2(divClass: string): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0], meta: {},
        blocks: [
            { t: 'Div', c: [['', [divClass], []], [
                { t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 },
            ]], s: 0 },
        ],
        astContext: { p: POOL_2 },
    });
}

describe('P3.3 §3b test 2 ★ — ancestor-only change re-derives path, cursor unchanged', () => {
    it('breadcrumb re-derives after ancestor label changes; editor stays on child', async () => {
        const setAst = vi.fn();
        const astJson1 = makeAst2('a');
        const props: PreviewRootProps = {
            astJson: astJson1,
            untransformedAstJson: astJson1,
            renderedContent: CONTENT_2,
            currentFilePath: '/test.qmd',
            assetManifest: {},
            setAst,
            unlockNestingCursor: true,
            onNavigateToDocument: () => {},
        };

        const { container, rerender } = render(<PreviewRoot {...props} />);

        // Step 0: settle render, mock tile rects.
        await act(async () => {});
        mockTileRects(container);

        // Step 1: open the child (pool-id=1) via mouse click (unlocked → leaf).
        const tileChild = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileChild, 'pool-id=1 (child Para) should be in DOM').not.toBeNull();
        await act(async () => {
            fireEvent(tileChild!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileChild!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea, 'editor should open on child').not.toBeNull();
        expect(textarea!.value).toBe('AAA');

        // Step 2: assert chip crumbs = ['Div.a', 'Para'] with 'Para' as current.
        const chipEl = container.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');
        expect(chipEl, 'breadcrumb chip should be visible').not.toBeNull();

        const crumbs1 = Array.from(chipEl!.querySelectorAll<HTMLElement>('.q2-crumb'));
        expect(crumbs1.map(c => c.textContent)).toEqual(['Dv', '¶']);
        // Discriminate by full label via title (abbreviation collapses both Div.a and Div.b to 'Dv').
        expect(crumbs1[0].getAttribute('title')).toBe('Div.a');
        const currentCrumb1 = crumbs1.find(c => c.getAttribute('aria-current') === 'true');
        expect(currentCrumb1, 'Para crumb should be aria-current').not.toBeUndefined();
        expect(currentCrumb1!.textContent).toBe('¶');

        // Step 3: external re-render — ancestor-only change (Div class: 'a' → 'b').
        // Same CONTENT_2 and POOL_2 (child range [6,9] unchanged).
        const astJson2 = makeAst2('b');
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={astJson2}
                    untransformedAstJson={astJson2}
                    renderedContent={CONTENT_2}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor={true}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Step 4: cursor unchanged — editor still open on child ('AAA'), no commit.
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter, 'editor should still be open (self-heal KEEP)').not.toBeNull();
        expect(textareaAfter!.value).toBe('AAA');
        expect(setAst).not.toHaveBeenCalled();

        // Step 5: path re-derived — chip crumbs now ['Div.b', 'Para'], 'Para' still current.
        // This assertion goes RED if buildAncestorPath is memoized on [anchorR0, anchorR1],
        // because those are stable across the ancestor-only change and the memo would
        // return stale crumbs from the previous AST ('Div.a').
        const chipAfter = container.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');
        expect(chipAfter, 'breadcrumb chip should still be visible').not.toBeNull();

        const crumbs2 = Array.from(chipAfter!.querySelectorAll<HTMLElement>('.q2-crumb'));
        expect(crumbs2.map(c => c.textContent)).toEqual(['Dv', '¶']);
        // Discriminate by full label via title — after rerender the ancestor is now Div.b.
        expect(crumbs2[0].getAttribute('title')).toBe('Div.b');
        const currentCrumb2 = crumbs2.find(c => c.getAttribute('aria-current') === 'true');
        expect(currentCrumb2, 'Para crumb should remain aria-current').not.toBeUndefined();
        expect(currentCrumb2!.textContent).toBe('¶');
    });
});
