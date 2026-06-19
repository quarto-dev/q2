/**
 * G9 integration tests: blur the outgoing cell during the reland gap.
 *
 * Root cause: with the settle-gate there is a deterministic interval between
 * editor-close (dirty commit) and destination editor-open (post-settle reland).
 * During this gap the source cell re-renders as static stale content. Without
 * a visual cue, it's jarring.
 *
 * Fix:
 *  - `fadeSourceR0Ref` records the source's `anchorR0` at every dirty-commit site.
 *  - A `useLayoutEffect` keyed on `editTarget` fires on the editor-close render
 *    (`editTarget === null && pendingLandingRef.current !== null`). It scans
 *    ALL `[data-block-pool-id]` elements (NOT outerBlockForAnchorR0, which misses
 *    nested elements) and adds `q2-reland-fade` to the one whose pool entry
 *    has `r[0] === fadeSourceR0Ref.current`.
 *  - `clearRelandFade()` removes the class — called at the top of `openEditTarget`
 *    and in `cancelPendingLand`.
 *  - CSS keyframes injected in `useBlockEditHover.tsx` stylesheet.
 *
 * T7 ★ (fail-on-revert — three revert hunks):
 *  (a) Remove `el.classList.add('q2-reland-fade')` → source cell never gets class
 *      → first assert RED.
 *  (b) Replace querySelectorAll scan with `outerBlockForAnchorR0` → nested source
 *      not found → no class → RED (binds the scan-vs-outer-only fix).
 *  (c) Remove `clearRelandFade()` from `openEditTarget` → class lingers after
 *      reland → second assert RED.
 *
 * Nested source is REQUIRED: T7 uses the child para (pool[1], r=[2,22]) as the
 * dirty-commit source. `outerBlockForAnchorR0` only scans outer (non-contained)
 * blocks and would skip the nested child para — binding revert hunk (b).
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';
import { detectPlatform } from './nestingNav';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

const PLATFORM = detectPlatform();

function nestingChord(key: 'ArrowLeft' | 'ArrowRight'): KeyboardEventInit {
    return PLATFORM === 'mac'
        ? { key, metaKey: true, ctrlKey: true, altKey: false, shiftKey: false }
        : { key, altKey: true, shiftKey: true, metaKey: false, ctrlKey: false };
}

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

/* ─── Fixture (verbatim from g6-g7-settle-gate.integration.test.tsx) ────── */
const CONTENT = '> line one\n> line two\n\npara2\n\n';
const POOL = [
    { t: 0, r: [0, 23], d: 0 },   // pool[0]: BlockQuote — OUTER
    { t: 0, r: [2, 22], d: 0 },   // pool[1]: ChildPara  — NESTED (r0=2)
    { t: 0, r: [23, 30], d: 0 },  // pool[2]: Para2
];
const CHILD_SI_KEY = '0:2-22:0';
const CLEAN_BUFFER = 'line one\nline two';

function makeAstJson(pool = POOL): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'BlockQuote',
                c: [
                    {
                        t: 'Para',
                        c: [
                            { t: 'Str', c: 'line one' },
                            { t: 'SoftBreak' },
                            { t: 'Str', c: 'line two' },
                        ],
                        s: 1,
                    },
                ],
                s: 0,
            },
            {
                t: 'Para',
                c: [{ t: 'Str', c: 'para2' }],
                s: 2,
            },
        ],
        astContext: { p: pool },
    });
}

function mountFixture(opts: {
    setAst?: (ast: PandocAST) => void;
} = {}) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockNestingCursor: true,
        nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        onNavigateToDocument: () => {},
    };
    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst };
}

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
 * T7 ★ (fail-on-revert — three revert hunks):
 *
 * G9 fade apply/clear with a NESTED source (ChildPara, pool[1], r0=2).
 *
 * Flow:
 *  1. Open ChildPara editor, dirty it.
 *  2. Nest-out (dirty path): commit fires, editor closes, pendingLanding set.
 *  3. Assert pool[1] DOM element has class `q2-reland-fade` (apply effect fired).
 *  4. Advance 250ms (settle-gate DEFERS, no reland yet) — class still present.
 *  5. Settled rerender (fresh content) → executeLanding opens blockquote editor.
 *  6. Assert NO element has `q2-reland-fade` (clearRelandFade called in openEditTarget).
 *
 * The NESTED source requirement: pool[1] (r0=2) is INSIDE pool[0] (r0=0). If the
 * implementation uses `outerBlockForAnchorR0` (which scans only non-contained outer
 * blocks), it would find pool[0] (the blockquote) rather than pool[1] (the child)
 * and add the class to the wrong element — OR (if r0=2 doesn't match any outer block)
 * find nothing and add no class. The scan-all-`[data-block-pool-id]` approach
 * finds pool[1] correctly.
 *
 * THREE revert hunks:
 *  (a) Remove `el.classList.add('q2-reland-fade')` → pool[1] element never gets
 *      class → step 3 assertion RED.
 *  (b) Replace querySelectorAll scan with `outerBlockForAnchorR0` → r0=2 not
 *      found in outer blocks (pool[0].r[0]=0, not 2) → no class → step 3 RED.
 *  (c) Remove `clearRelandFade()` from `openEditTarget` → class lingers after
 *      reland → step 6 assertion RED.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('G9 T7 ★ — reland-fade applies to nested source cell and clears on land', () => {
    it('pool[1] gets q2-reland-fade on editor-close; cleared after reland', async () => {
        vi.useFakeTimers();

        const setAst = vi.fn();
        const { container, rerender } = mountFixture({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate the nested ChildPara (pool[1]).
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Step 2: dirty the draft.
        const DIRTY_EDIT = 'line one edited\nline two';
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: DIRTY_EDIT } });
        });

        // Step 3: nest-out (dirty path → commitAndArmReland → editor closes).
        await act(async () => {
            fireEvent.keyDown(
                container.querySelector<HTMLTextAreaElement>('textarea')!,
                nestingChord('ArrowLeft'),
            );
        });

        // Editor closed (commit fired).
        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // Step 4: Assert the source cell (pool[1]) has `q2-reland-fade`.
        // The apply useLayoutEffect fires on the editor-close render.
        // The source cell r0=2 is a NESTED block inside the blockquote.
        // Using querySelectorAll-all scan is required to find it.
        const sourceCellEl = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(sourceCellEl, 'source cell (pool[1]) should still be in DOM after editor close').not.toBeNull();
        expect(
            sourceCellEl!.classList.contains('q2-reland-fade'),
            'source cell must have q2-reland-fade class during the reland gap',
        ).toBe(true);

        // Step 5: Advance 250ms — settle-gate defers (content unchanged). Class still present.
        await act(async () => {
            vi.advanceTimersByTime(300);
        });
        // Class should still be present (gate deferred, editor not yet open).
        expect(
            container.querySelector<HTMLElement>('[data-block-pool-id="1"]')?.classList.contains('q2-reland-fade'),
        ).toBe(true);

        // Step 6: Settled rerender — fresh content.
        const SETTLED_CONTENT = '> line one edited\n> line two\n\npara2\n\n';
        const settledPool = [
            { t: 0, r: [0, 31], d: 0 },   // BlockQuote (larger)
            { t: 0, r: [2, 30], d: 0 },   // ChildPara (larger)
            { t: 0, r: [31, 38], d: 0 },  // Para2
        ];
        const settledAstJson = makeAstJson(settledPool);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={settledAstJson}
                    untransformedAstJson={settledAstJson}
                    renderedContent={SETTLED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Editor should open on the blockquote (nest-out destination).
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter, 'editor should open on blockquote after settled rerender').not.toBeNull();

        // Step 7: Assert NO element has `q2-reland-fade` anymore.
        // clearRelandFade() was called inside openEditTarget.
        const fadedEls = container.querySelectorAll('.q2-reland-fade');
        expect(
            fadedEls.length,
            'q2-reland-fade must be cleared when the destination editor opens',
        ).toBe(0);
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * G14-1 ★ (fail-on-revert — reset hunk A):
 *
 * Stale `fadeSourceR0Ref` re-fades an unrelated cell on a plain close.
 *
 * Flow:
 *  1–6. Same dirty nest-out + settled rerender as T7 (arms fadeSourceR0Ref=2,
 *       then T7's openEditTarget clears the fade class but — pre-fix — leaves
 *       fadeSourceR0Ref.current = 2, sticky).
 *  7. Blockquote editor is now open. PLAIN BLUR it (no dirty edit): fires
 *     requestFocusRestore → stashes intent:'focus' landing → closes editor.
 *  8. Editor close render: editTarget becomes null + pendingLanding is set →
 *     G9 apply useLayoutEffect fires. With stale r0=2 and pool[1].r[0]===2
 *     still in DOM, the unfixed code re-fades pool[1].
 *  9. executeLanding(focus) runs (IMMEDIATELY, no timer advance needed — it
 *     fires synchronously in the same act), focuses the outer block, nulls
 *     pendingLandingRef — but does NOT call clearRelandFade() → class lingers.
 * 10. Assert immediately: 0 elements with .q2-reland-fade.
 *     Unfixed: expected 0, got 1 → RED (proven 2026-06-18).
 *     Fixed (A: fadeSourceR0Ref.current = null in clearRelandFade): r0===null
 *     → apply effect early-returns → no re-fade → GREEN.
 *
 * Exercise precondition: the settled pool must keep an entry with r[0]===2 so
 * the stale ref has a DOM target to re-fade (settledPool[1].r=[2,30] satisfies
 * this). The dirty nest-out in steps 1–4 arms the ref; the T7-phase clears the
 * class but (pre-fix) leaves the ref set.
 *
 * Named revert hunk → RED: remove `fadeSourceR0Ref.current = null` from
 * `clearRelandFade` → ref never reset → apply effect re-fades pool[1] →
 * `expected 0, got 1`.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('G14-1 ★ — stale fadeSourceR0Ref does not re-fade on plain close', () => {
    it('no .q2-reland-fade after plain blur of blockquote editor (no timer advance)', async () => {
        vi.useFakeTimers();

        const setAst = vi.fn();
        const { container, rerender } = mountFixture({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // ── Steps 1–4 (replicate T7's dirty nest-out) ──────────────────────
        // Activate the nested ChildPara (pool[1]).
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Dirty the draft.
        const DIRTY_EDIT = 'line one edited\nline two';
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: DIRTY_EDIT } });
        });

        // Nest-out (dirty path → commitAndArmReland → arms fadeSourceR0Ref=2).
        await act(async () => {
            fireEvent.keyDown(
                container.querySelector<HTMLTextAreaElement>('textarea')!,
                nestingChord('ArrowLeft'),
            );
        });

        // Editor closed; commit fired.
        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // ── Step 5: settled rerender → openEditTarget fires → clearRelandFade called ──
        // (Pre-fix: clears the class but leaves fadeSourceR0Ref.current=2 sticky.)
        const SETTLED_CONTENT = '> line one edited\n> line two\n\npara2\n\n';
        const settledPool = [
            { t: 0, r: [0, 31], d: 0 },   // BlockQuote (larger)
            { t: 0, r: [2, 30], d: 0 },   // ChildPara (r[0]===2 — still present for precondition)
            { t: 0, r: [31, 38], d: 0 },  // Para2
        ];
        const settledAstJson = makeAstJson(settledPool);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={settledAstJson}
                    untransformedAstJson={settledAstJson}
                    renderedContent={SETTLED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Blockquote editor should now be open (T7 confirmed this).
        const bqTextarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(bqTextarea, 'blockquote editor must be open after settled rerender').not.toBeNull();

        // No fade class at this point (T7 verified this).
        expect(container.querySelectorAll('.q2-reland-fade').length).toBe(0);

        // ── Step 6: PLAIN BLUR the blockquote editor (no dirty edit) ───────
        // This fires requestFocusRestore → stashes intent:'focus' landing →
        // closes the editor → G9 apply effect fires with stale r0=2.
        await act(async () => {
            fireEvent.blur(bqTextarea!);
        });

        // ── Step 7: Assert IMMEDIATELY (no timer advance) ───────────────────
        // The 'focus' landing runs synchronously in executeLanding.
        // Fixed: fadeSourceR0Ref.current was reset to null in clearRelandFade →
        //        apply effect early-returns → no re-fade → 0 elements.
        // Unfixed: stale r0=2 → pool[1].r[0]===2 → re-faded → 1 element.
        const fadedElsAfterBlur = container.querySelectorAll('.q2-reland-fade');
        expect(
            fadedElsAfterBlur.length,
            'no .q2-reland-fade should exist after plain blur (stale ref must not re-fade)',
        ).toBe(0);
    });
});
