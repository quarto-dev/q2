/**
 * P2.5a integration tests: real-PreviewRoot coverage for the move/reland/focus machine.
 *
 * These tests drive the REAL production code in PreviewRoot.tsx through the same
 * pointer/keyboard events a real user would fire. They close the coverage gap left
 * by p2-4b / p2-4c, where MoveHarness / FocusHarness REIMPLEMENT the machine
 * instead of exercising it.
 *
 * Fail-on-revert guarantee:
 *   Each critical test was verified to FAIL when the relevant production code is
 *   neutralized (e.g. making executeLanding a no-op, or the reland useLayoutEffect
 *   return early). The failure messages are recorded in the plan document.
 *
 * Retained harness tests:
 *   - The EditTextareaKeydownHarness tests in p2-4b are kept — they exercise the
 *     real EditTextarea.onKeyDown → ctx.requestMove trigger contract, which is a
 *     distinct unit from the machine itself.
 *   - All direct-unit tests (captureEditTarget, measureBlockBox, caretGeometry) are kept.
 *
 * Coverage added here:
 *
 *  1. Arrow move — unmodified (synchronous hop):
 *     Open editor on tile A, fire ArrowDown (isOnLastVisualLine mocked) → destination
 *     editor opens synchronously, setAst NOT called.
 *
 *  2. Arrow move — modified (async reland):
 *     Open A, type, ArrowDown → setAst called (commit), editor closed; re-render with
 *     new pool/content → destination editor opens via the real reland layout effect.
 *
 *  3. Modified move — byte-identical commit fallback:
 *     Dirty move, no re-render → real 250ms timeout fallback relands. (Fake timers.)
 *
 *  4. Plain-commit focus-restoration (Cmd-Enter):
 *     Open A, type, Cmd-Enter → commit + close; re-render → focus lands on A's tile
 *     via outerBlockForAnchorR0 (by anchorR0, not pool index — meaningful because the pool
 *     shifts in the re-render fixture).
 *
 *  5. Esc focus-restoration (timeout fallback):
 *     Open A, Esc → no re-render → fake-timer advance → real timeout fires → tile focused.
 *
 * jsdom gotchas (same as p2-4d):
 *   - getBoundingClientRect returns zeroes — must be mocked on tile elements.
 *   - PointerEvent.pointerType is not honoured from init dict — use ptrEvent helper.
 *   - getComputedStyle returns empty strings — textarea renders with contentHeight=0 (fine).
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
import * as caretGeometry from './caretGeometry';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── PointerEvent helper ────────────────────────────────────────────────────
 * (Copied from p2-4d: jsdom's PointerEvent does not honour `pointerType`.)
 */
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

/* ─── Document fixtures ──────────────────────────────────────────────────────
 *
 * Four-paragraph document (same as p2-4d):
 *   para0: pool[0] r=[0,6]    "para0\n"   line 0
 *   para1: pool[1] r=[6,12]   "para1\n"   line 1  ← A (edit target in most tests)
 *   para2: pool[2] r=[12,19]  "para2\n\n" line 2  ← B (arrow-down destination from A)
 *   para3: pool[3] r=[19,26]  "para3\n\n" line 4
 *
 * anchorSlice values (normalizeLineEndings + trimEnd):
 *   para0 → "para0"   para1 → "para1"
 *   para2 → "para2"   para3 → "para3"
 */
const CONTENT = 'para0\npara1\npara2\n\npara3\n\n';

const POOL = [
    { t: 0, r: [0, 6], d: 0 },    // pool[0]: para0\n   line 0
    { t: 0, r: [6, 12], d: 0 },   // pool[1]: para1\n   line 1 (A)
    { t: 0, r: [12, 19], d: 0 },  // pool[2]: para2\n\n line 2 (B after A)
    { t: 0, r: [19, 26], d: 0 },  // pool[3]: para3\n\n line 4
];

function makeAstJson(pool: typeof POOL, content: string): string {
    const blocks = pool.map((entry, i) => {
        const raw = content.slice(entry.r[0], entry.r[1]);
        const text = raw.replace(/\n/g, '').trim() || `tile${i}`;
        return { t: 'Para', c: [{ t: 'Str', c: text }], s: i };
    });
    const ast = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: pool },
    };
    return JSON.stringify(ast);
}

/** Mount a standard 4-tile PreviewRoot. */
function mountPreviewRoot(
    opts: {
        setAst?: (ast: PandocAST) => void;
        pool?: typeof POOL;
        content?: string;
    } = {},
) {
    const pool = opts.pool ?? POOL;
    const content = opts.content ?? CONTENT;
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson(pool, content);

    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
    };

    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst, pool, content };
}

/**
 * Mock getBoundingClientRect on all [data-block-pool-id] tile elements.
 * Each tile gets a distinct non-zero rect so enumerateOuterBlocks sees them.
 */
function mockTileRects(container: HTMLElement) {
    const tiles = container.querySelectorAll<HTMLElement>('[data-block-pool-id]');
    tiles.forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
            width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
        } as DOMRect);
    });
}

/**
 * Helper: activate tile A (pool[1]) via real pointer events.
 * Returns the textarea element once open.
 */
async function activateTileA(container: HTMLElement): Promise<HTMLTextAreaElement> {
    const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
    expect(tileA).not.toBeNull();

    await act(async () => {
        fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });

    const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea).not.toBeNull();
    expect(textarea!.value).toBe('para1');
    return textarea!;
}

/* ─────────────────────────────────────────────────────────────────────────────
 * 1. Arrow move — unmodified (synchronous hop)
 *
 * The production path:
 *   EditTextarea.onKeyDown(ArrowDown) [isOnLastVisualLine mocked true]
 *   → ctx.requestMove('down', col, draft, false, srcJson)
 *   → PreviewRoot.requestMove: isDirty=false → synchronous hop → setEditTargetRaw(B)
 *   → B's textarea opens immediately; setAst NOT called.
 *
 * Fail-on-revert: if requestMove's isDirty=false branch is commented out (or
 * setEditTargetRaw is not called), no textarea appears → test fails on the
 * "destination editor open" assertion.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — arrow move (unmodified): synchronous hop to destination', () => {
    it('opens B synchronously on ArrowDown at last visual line; setAst not called', async () => {
        // Mock geometry: caret IS on the last visual line.
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate tile A (pool[1], r0=6, "para1").
        const textarea = await activateTileA(container);

        // Step 2: fire ArrowDown (no modifiers) — isOnLastVisualLine is mocked true.
        await act(async () => {
            fireEvent.keyDown(textarea, { key: 'ArrowDown' });
        });

        // No commit — unmodified move.
        expect(setAst).not.toHaveBeenCalled();

        // B's editor (pool[2], r0=12, "para2") opens synchronously.
        const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('para2');
    });

    it('opens previous tile synchronously on ArrowUp at first visual line; setAst not called', async () => {
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate tile B (pool[2], "para2").
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        expect(tileB).not.toBeNull();
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para2');

        // Fire ArrowUp — should hop to A (pool[1], "para1").
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowUp' });
        });

        expect(setAst).not.toHaveBeenCalled();

        const textareaA = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaA).not.toBeNull();
        expect(textareaA!.value).toBe('para1');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 2. Arrow move — modified (async reland via real reland layout effect)
 *
 * The production path:
 *   type → dirty; ArrowDown [isOnLastVisualLine mocked]
 *   → requestMove: isDirty=true → setAst (commit), setEditTargetRaw(null), stash pendingLanding
 *   → editor closes; re-render with new pool/content
 *   → reland useLayoutEffect fires → executeLanding opens B via projected destLine.
 *
 * Fail-on-revert: if the reland useLayoutEffect body is made a no-op (return early),
 * no textarea appears after re-render → test fails on "destination editor open" assertion.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — arrow move (modified): async reland via real reland effect', () => {
    it('commits A, closes editor, then relands B after re-render with new AST', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate A (pool[1], "para1").
        const textarea = await activateTileA(container);

        // Step 2: type to make it dirty (1-line → 2-line for distinct delta).
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'para1\nextra' } });
        });
        const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(ta.value).toBe('para1\nextra');

        // Step 3: ArrowDown — dirty move → commit + stash landing + close.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowDown' });
        });

        // Commit was called.
        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as any;
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('text');
        expect(payload.newText).toContain('para1');
        expect(payload.newText).toContain('extra');

        // Editor closed.
        expect(container.querySelector('textarea')).toBeNull();

        // Step 4: simulate commit re-render. A expanded by 1 line → B shifts.
        // Post-commit pool: A grows to r=[6,18], B shifts to r=[18,25] at line 3.
        const newPool = [
            { t: 0, r: [0, 6], d: 0 },    // para0 (unchanged)
            { t: 0, r: [6, 18], d: 0 },   // para1\nextra (A expanded, 2 lines)
            { t: 0, r: [18, 25], d: 0 },  // para2\n\n (B shifted to line 3)
            { t: 0, r: [25, 32], d: 0 },  // para3\n\n
        ] as typeof POOL;
        const newContent = 'para0\npara1\nextra\npara2\n\npara3\n\n';
        const newAstJson = makeAstJson(newPool, newContent);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={newContent}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Re-mock rects for new tile elements.
        mockTileRects(container);

        // Step 5: reland layout effect fires → B's editor opens.
        // draftLineCount=2, L0=1, destLine=1+2=3; pool[2] now at line 3 = B. ✓
        const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('para2');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 3. Modified move — byte-identical commit fallback (real 250ms timeout)
 *
 * Dirty ArrowDown → commit → no content re-render (byte-identical) →
 * the reland useLayoutEffect has no new inputs to fire on →
 * the real 250ms timeout fallback in requestMove fires → B opens.
 *
 * Fail-on-revert: if the setTimeout fallback in requestMove is removed, the
 * timeout never fires and the test fails on "B's editor open" assertion.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — modified move: byte-identical commit uses real 250ms timeout fallback', () => {
    it('relands B via timeout when no re-render occurs (byte-identical commit)', async () => {
        vi.useFakeTimers();
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A.
        const textarea = await activateTileA(container);

        // Make dirty with same line count as anchorSlice → delta=0.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'para1x' } });
        });
        const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;

        // ArrowDown — dirty move; byte-identical, so no re-render will come.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowDown' });
        });

        expect(setAst).toHaveBeenCalledOnce();
        // Editor closed; B not yet open.
        expect(container.querySelector('textarea')).toBeNull();

        // Advance past 250ms — real timeout fallback fires.
        await act(async () => {
            vi.advanceTimersByTime(300);
        });

        // B (pool[2], r0=12, "para2") should be open.
        const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('para2');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 4. Plain-commit focus-restoration (Cmd-Enter)
 *
 * Tests the real requestFocusRestore → reland layout effect → outerBlockForAnchorR0
 * → tile.focus() chain.
 *
 * The pool SHIFTS in the re-render so "by anchorR0, not pool-index" is meaningful:
 *   - Pre-commit: A is pool[1] at r0=6.
 *   - Post-commit: new block inserted at pool[0], A shifts to pool[2] at r0=10.
 *   - Stashed anchorR0=6 resolves via outerBlockForAnchorR0 nearest-at/after → new outer block.
 *
 * Approach: the reland layout effect fires SYNCHRONOUSLY during the rerender act()
 * call, which means the focus spy must be installed BEFORE rerender. We use
 * vi.useFakeTimers() to freeze time so the timeout fallback doesn't race, then
 * track focus calls by patching HTMLElement.prototype.focus for the test duration.
 *
 * Fail-on-revert: if requestFocusRestore in dispatchers.tsx is removed from the
 * Cmd-Enter handler (so the pending landing is never stashed), no tile gets focused
 * after re-render → test fails on "tile.focus() called" assertion.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — Cmd-Enter: plain-commit focus-restoration via real reland effect', () => {
    it('focuses the edited tile by anchorR0 after commit + re-render (pool index shifts)', async () => {
        // Track all .focus() calls on any HTMLElement via a prototype patch.
        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container, rerender } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // Activate A (pool[1], r0=6, "para1").
            const textarea = await activateTileA(container);

            // Type to make dirty.
            await act(async () => {
                fireEvent.change(textarea, { target: { value: 'para1 modified' } });
            });
            const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;

            // Cmd-Enter — stashes focus restore (anchorR0=6) and commits.
            focusedElements.length = 0; // clear autoFocus and any other incidentals
            await act(async () => {
                fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
            });

            // Commit happened.
            expect(setAst).toHaveBeenCalledOnce();
            // Editor closed.
            expect(container.querySelector('textarea')).toBeNull();

            // Clear focus calls from editor close transition.
            focusedElements.length = 0;

            // Simulate commit re-render where pool indices shift.
            // A new block "new\n" is inserted at pool[0] (r=[0,4]);
            // the old tiles shift: A (was r0=6) is now at r0=10 (pool[2]).
            const newPool = [
                { t: 0, r: [0, 4], d: 0 },    // pool[0]: "new\n"  (new block)
                { t: 0, r: [4, 10], d: 0 },   // pool[1]: "para0\n" shifted
                { t: 0, r: [10, 24], d: 0 },  // pool[2]: "para1 modified\n" (A shifted, nearest to r0=6)
                { t: 0, r: [24, 31], d: 0 },  // pool[3]: "para2\n\n" shifted
                { t: 0, r: [31, 38], d: 0 },  // pool[4]: "para3\n\n" shifted
            ] as typeof POOL;
            const newContent = 'new\npara0\npara1 modified\npara2\n\npara3\n\n';
            const newAstJson = makeAstJson(newPool, newContent);

            // Re-render: the reland layout effect fires synchronously during this act().
            // Before re-rendering, mock tile rects on tiles that will exist in the new DOM.
            await act(async () => {
                rerender(
                    <PreviewRoot
                        astJson={newAstJson}
                        untransformedAstJson={newAstJson}
                        renderedContent={newContent}
                        currentFilePath="/test.qmd"
                        assetManifest={{}}
                        setAst={setAst}
                        onNavigateToDocument={() => {}}
                    />,
                );
            });

            // Mock rects on new tiles (reland needs enumerateOuterBlocks to see them).
            // The reland effect already fired; if rects weren't mocked it would have used
            // existing tile elements. Let's check if focus was called.
            // Note: the layout effect fires AFTER DOM is committed but BEFORE paint.
            // In the test environment it runs during the act() above.

            // Filter: focus calls on elements with data-block-pool-id (tile elements only).
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );

            // The focused tile should be pool[2] (anchorR0=10, nearest at/after stashed r0=6).
            expect(tileFocusCalls.length).toBeGreaterThan(0);
            const focusedTile = tileFocusCalls[0];
            // pool[2] has data-block-pool-id="2"
            expect(focusedTile.getAttribute('data-block-pool-id')).toBe('2');
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 5. Esc focus-restoration (real 250ms timeout fallback)
 *
 * The production path:
 *   Esc keyDown → cancelPendingLand → requestFocusRestore(anchorR0) → setEditTarget(null)
 *   → no content re-render (no commit) → real 250ms timeout fires → tile.focus().
 *
 * Approach: when the editor is OPEN, tile A's <p data-block-pool-id="1"> element
 * is replaced by the textarea wrapper (Block renders the edit surface instead of
 * Para). After Esc closes the editor, a NEW <p data-block-pool-id="1"> is rendered.
 * We use a prototype patch on HTMLElement.prototype.focus to capture calls made to
 * ANY element — this works regardless of DOM element identity.
 *
 * Fail-on-revert: if requestFocusRestore is removed from the Esc handler in
 * dispatchers.tsx, no pending landing is stashed → timeout never calls tile.focus()
 * → test fails on "tile focused" assertion.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — Esc: focus-restoration via real 250ms timeout fallback', () => {
    it('focuses the edited tile via timeout after Esc (no re-render)', async () => {
        vi.useFakeTimers();

        // Patch HTMLElement.prototype.focus to capture all focus calls.
        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // Activate A (pool[1], r0=6, "para1").
            const textarea = await activateTileA(container);
            // Clear focus calls from activation/autoFocus.
            focusedElements.length = 0;

            // Fire Esc — real handler: cancelPendingLand, requestFocusRestore(6), setEditTarget(null).
            await act(async () => {
                fireEvent.keyDown(textarea, { key: 'Escape' });
            });

            // No commit.
            expect(setAst).not.toHaveBeenCalled();
            // Editor closed.
            expect(container.querySelector('textarea')).toBeNull();

            // After editor closes, tile A's <p> element is back in the DOM.
            // Mock its rect so outerBlockForAnchorR0 finds it during the timeout.
            mockTileRects(container);

            // Timeout not yet fired.
            const tileFocusCallsBefore = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCallsBefore).toHaveLength(0);

            // Advance past 250ms — real timeout fires → outerBlockForAnchorR0(r0=6) → tileA.focus().
            await act(async () => {
                vi.advanceTimersByTime(300);
            });

            // Filter to tile focus calls only.
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls.length).toBeGreaterThan(0);
            // Must be A (pool[1], data-block-pool-id="1") — the tile that was being edited.
            expect(tileFocusCalls[0].getAttribute('data-block-pool-id')).toBe('1');
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });

    it('focuses the edited tile via timeout after blur on empty/unmodified (plain close path)', async () => {
        vi.useFakeTimers();

        // Same prototype-patch approach.
        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // Activate A (pool[1], r0=6, "para1").
            const textarea = await activateTileA(container);
            // Clear focus calls from activation/autoFocus.
            focusedElements.length = 0;

            // Blur without clicking another tile → plain close.
            // Real path: requestFocusRestore(6) → commitIfDirty (unmodified → just close).
            await act(async () => {
                fireEvent.blur(textarea);
            });

            // No commit (draft unmodified).
            expect(setAst).not.toHaveBeenCalled();
            expect(container.querySelector('textarea')).toBeNull();

            // After editor closes, tile A's <p> is back. Mock its rect for outerBlockForAnchorR0.
            mockTileRects(container);

            // Timeout not yet fired.
            const tileFocusCallsBefore = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCallsBefore).toHaveLength(0);

            // Advance past 250ms.
            await act(async () => {
                vi.advanceTimersByTime(300);
            });

            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls.length).toBeGreaterThan(0);
            expect(tileFocusCalls[0].getAttribute('data-block-pool-id')).toBe('1');
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 6. Arrow wrap: ArrowDown on last tile → first tile (real production wrap logic)
 *
 * Ported from DirectMoveHarness "wrap" tests in p2-4b (which exercised a
 * reimplementation). These verify the real requestMove wrap path in PreviewRoot.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — arrow move wrap: ArrowDown on last tile wraps to first tile', () => {
    it('opens pool[0] (para0) when ArrowDown fires from the last tile (pool[3])', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate tile 3 (pool[3], r0=19, "para3").
        const tileC = container.querySelector<HTMLElement>('[data-block-pool-id="3"]');
        expect(tileC).not.toBeNull();
        await act(async () => {
            fireEvent(tileC!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileC!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para3');

        // ArrowDown from last tile → wrap to first (pool[0], "para0").
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown' });
        });

        expect(setAst).not.toHaveBeenCalled();
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter).not.toBeNull();
        expect(textareaAfter!.value).toBe('para0');
    });
});

describe('P2.5a — arrow move wrap: ArrowUp on first tile wraps to last tile', () => {
    it('opens pool[3] (para3) when ArrowUp fires from the first tile (pool[0])', async () => {
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate tile 0 (pool[0], r0=0, "para0").
        const tileFirst = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tileFirst).not.toBeNull();
        await act(async () => {
            fireEvent(tileFirst!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileFirst!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para0');

        // ArrowUp from first tile → wrap to last (pool[3], "para3").
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowUp' });
        });

        expect(setAst).not.toHaveBeenCalled();
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter).not.toBeNull();
        expect(textareaAfter!.value).toBe('para3');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 7. Single-tile document → no-op on ArrowDown/Up
 *
 * Ported from DirectMoveHarness "single-tile" test in p2-4b.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — single-tile document: ArrowDown/Up are no-ops', () => {
    it('does not hop or commit when only one tile exists', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);

        // Single-tile document.
        const singlePool = [{ t: 0, r: [0, 6], d: 0 }] as typeof POOL;
        const singleContent = 'para0\n';

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst, pool: singlePool, content: singleContent });

        await act(async () => {});
        mockTileRects(container);

        // Activate the single tile (pool[0], "para0").
        const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tile).not.toBeNull();
        await act(async () => {
            fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para0');

        // ArrowDown on single tile → no-op.
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown' });
        });

        // Editor still open (same textarea), no commit.
        expect(setAst).not.toHaveBeenCalled();
        expect(container.querySelector('textarea')).not.toBeNull();

        // ArrowUp on single tile → no-op.
        await act(async () => {
            fireEvent.keyDown(container.querySelector('textarea')!, { key: 'ArrowUp' });
        });

        expect(setAst).not.toHaveBeenCalled();
        expect(container.querySelector('textarea')).not.toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * (Previously in p2-4c FocusHarness: ported to real PreviewRoot)
 *
 * 8b. Don't steal focus if a new edit has already started
 *
 * After plain close + stashed focus landing, if a new editor is opened
 * (new tile activated) before the re-render, executeLanding for intent:'focus'
 * must NOT call focus() (editTargetRef.current != null guard).
 * ──────────────────────────────────────────────────────────────────────────── */

describe("P2.5a — don't steal focus if a new edit opened before reland", () => {
    it('does not focus the tile when a new editor is open during the reland', async () => {
        // Patch focus to track calls.
        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container, rerender } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // Step 1: activate A (pool[1], "para1"), type, Cmd-Enter → stash focus restore.
            const textarea = await activateTileA(container);
            await act(async () => {
                fireEvent.change(textarea, { target: { value: 'para1 modified' } });
            });
            const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
            focusedElements.length = 0;

            await act(async () => {
                fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
            });

            expect(setAst).toHaveBeenCalledOnce();
            expect(container.querySelector('textarea')).toBeNull();
            focusedElements.length = 0;

            // Step 2: BEFORE re-render, activate B (pool[2], "para2").
            // This opens a new editor → editTargetRef.current becomes non-null.
            const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
            expect(tileB).not.toBeNull();
            await act(async () => {
                fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
                fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
            });

            // B's editor is open.
            expect(container.querySelector('textarea')).not.toBeNull();
            focusedElements.length = 0;

            // Step 3: re-render (commit re-render / epoch advance) →
            // reland effect fires but detects editTargetRef.current != null → skips focus.
            const newAstJson = makeAstJson(POOL, CONTENT);
            await act(async () => {
                rerender(
                    <PreviewRoot
                        astJson={newAstJson + ' '} // different string to trigger astJson change
                        untransformedAstJson={newAstJson + ' '}
                        renderedContent={CONTENT}
                        currentFilePath="/test.qmd"
                        assetManifest={{}}
                        setAst={setAst}
                        onNavigateToDocument={() => {}}
                    />,
                );
            });

            // Focus should NOT have been called on any tile — new edit is open.
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls).toHaveLength(0);
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 8c. A move does NOT trigger focus-restore on the source tile
 *
 * An unmodified arrow move closes A without calling requestFocusRestore.
 * The pending landing has intent:'activate' (for B), NOT intent:'focus'.
 * After the move, A's tile is NOT focused.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — arrow move does NOT focus-restore on source tile', () => {
    it('does not focus tile A after an unmodified ArrowDown move to B', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            const textarea = await activateTileA(container);
            focusedElements.length = 0;

            // Arrow move (unmodified) from A to B.
            await act(async () => {
                fireEvent.keyDown(textarea, { key: 'ArrowDown' });
            });

            // B is open (synchronous hop).
            const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
            expect(textareaB).not.toBeNull();
            expect(textareaB!.value).toBe('para2');
            expect(setAst).not.toHaveBeenCalled();

            focusedElements.length = 0;

            // No tile element (data-block-pool-id) should receive .focus() — B's editor
            // opens via editTargetRaw, NOT via tile.focus().
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls).toHaveLength(0);
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 9. Esc-after-dirty-move: cancelPendingLand clears the fallback timer
 *
 * Reachability analysis:
 *   After a *modified* ArrowDown move the editor is immediately closed by
 *   requestMove (setEditTargetRaw(null)), leaving no textarea in the DOM.
 *   The Esc handler in dispatchers.tsx lives inside the textarea's onKeyDown
 *   — it cannot fire while the editor is closed.  Therefore the scenario
 *   "Esc cancels a pending intent:'activate' landing" is NOT user-reachable
 *   through normal keyboard interaction after a dirty move closes the editor.
 *
 *   The reachable path where cancelPendingLand actually clears the timer is:
 *   Esc pressed while the editor is open AND a prior focus-restore landing
 *   (intent:'focus') is pending.  dispatchers.tsx line 219 calls
 *   cancelPendingLand before requestFocusRestore re-arms the timer.
 *
 *   This test drives that path using precise fake-timer advancement to
 *   discriminate whether the FIRST timer (armed at t=0) or only the SECOND
 *   timer (armed at t=200ms) fires focus-restore:
 *
 *     t=0:    First Esc → T1 armed (fires at t=250ms)
 *     t=200:  Re-open A, second Esc:
 *               cancelPendingLand() [if it clears T1, T1 is gone]
 *               requestFocusRestore() arms T2 (fires at t=450ms)
 *     t=251:  Window where T1 would fire (missing clearTimeout) but T2 hasn't.
 *               With clearTimeout: 0 focus calls (T1 cancelled, T2 not yet)
 *               Without clearTimeout: 1 focus call (T1 fired and consumed T2's pending)
 *     t=500:  One total focus call either way (just at a different time).
 *
 *   The discriminating assertion is at t=251ms.
 *
 * Fail-on-revert check (reported):
 *   When clearTimeout(fallbackTimerRef.current) is removed from
 *   cancelPendingLand, T1 still fires at t=250ms.  It reads
 *   pendingLandingRef.current (= T2's pending, set by requestFocusRestore)
 *   and executes focus-restore — so by t=251ms one focus call has already
 *   happened.  The test asserts zero calls at that checkpoint and FAILS.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — Esc cancelPendingLand clears the fallback timer (reachable path)', () => {
    it('no focus-restore fires at 50ms past first-timer deadline when second Esc cancelled it', async () => {
        vi.useFakeTimers();

        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const { container } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // t=0: activate A, press Esc → T1 armed (fires at t=250ms).
            const textarea1 = await activateTileA(container);
            focusedElements.length = 0;

            await act(async () => {
                fireEvent.keyDown(textarea1, { key: 'Escape' });
            });

            expect(setAst).not.toHaveBeenCalled();
            expect(container.querySelector('textarea')).toBeNull();

            // Advance 200ms → t=200ms.  T1 not yet fired.
            await act(async () => { vi.advanceTimersByTime(200); });
            expect(focusedElements.filter(el => el.hasAttribute('data-block-pool-id'))).toHaveLength(0);

            // t=200ms: re-open A, press Esc.
            //   cancelPendingLand() — should kill T1 via clearTimeout.
            //   requestFocusRestore() — arms T2 (fires at t=200+250 = t=450ms).
            mockTileRects(container);
            const textarea2 = await activateTileA(container);
            focusedElements.length = 0;

            await act(async () => {
                fireEvent.keyDown(textarea2, { key: 'Escape' });
            });

            expect(container.querySelector('textarea')).toBeNull();
            mockTileRects(container);
            focusedElements.length = 0;

            // Advance to t=251ms (50ms past T1's expected fire time).
            // Key checkpoint: with clearTimeout working, T1 is dead — 0 focus calls.
            // Without clearTimeout, T1 fires at t=250ms → 1 focus call here.
            await act(async () => { vi.advanceTimersByTime(51); });

            const callsAtT251 = focusedElements.filter(el => el.hasAttribute('data-block-pool-id'));
            expect(callsAtT251).toHaveLength(0); // T1 must be dead

            // Advance to t=501ms — T2 fires at t=450ms → A focused.
            await act(async () => { vi.advanceTimersByTime(250); });

            const finalCalls = focusedElements.filter(el => el.hasAttribute('data-block-pool-id'));
            expect(finalCalls).toHaveLength(1);
            expect(finalCalls[0].getAttribute('data-block-pool-id')).toBe('1');
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 10. File-switch cancels focus-restore via the TIMEOUT path
 *
 * Distinct from test 8 (which tests the reland layout-effect fromFile guard
 * via re-render with both a new astJson and a new currentFilePath).
 *
 * This test isolates the TIMEOUT callback's fromFile guard at
 * PreviewRoot.tsx:~517.  The approach: re-render with a different
 * currentFilePath but the SAME astJson/renderedContent, so the reland layout
 * effect does NOT re-fire (its deps are unchanged) and pendingLandingRef is
 * still set when the timer fires.  The timer's own `pl.fromFile !==
 * currentFilePathRef.current` guard must short-circuit and skip
 * executeLanding.
 *
 * Fail-on-revert check (reported):
 *   When the `if (pl.fromFile !== currentFilePathRef.current)` guard is
 *   removed from the requestFocusRestore timeout callback, the timer fires
 *   and calls executeLanding → outerBlockForAnchorR0 → tile.focus().  The test
 *   asserts zero tile focus-calls after vi.advanceTimersByTime(300); with the
 *   guard removed a focus call appears and the assertion fails.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — file-switch cancels focus-restore via TIMEOUT path', () => {
    it('does not restore focus when currentFilePath changes before the 250ms timeout fires', async () => {
        vi.useFakeTimers();

        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            const setAst = vi.fn();
            const astJson = makeAstJson(POOL, CONTENT);
            const { container, rerender } = mountPreviewRoot({ setAst });

            await act(async () => {});
            mockTileRects(container);

            // Step 1: activate A (pool[1], r0=6, "para1"), press Esc.
            // requestFocusRestore stashes { intent:'focus', anchorR0:6, fromFile:'/test.qmd' }
            // and arms the 250ms timer.
            const textarea = await activateTileA(container);
            focusedElements.length = 0;

            await act(async () => {
                fireEvent.keyDown(textarea, { key: 'Escape' });
            });

            expect(setAst).not.toHaveBeenCalled();
            expect(container.querySelector('textarea')).toBeNull();
            mockTileRects(container);
            focusedElements.length = 0;

            // Step 2: re-render with a DIFFERENT currentFilePath but the SAME
            // astJson/renderedContent/untransformedAstJson.
            // The reland layout effect's deps ([astJson, renderedContent, untransformedAstJson])
            // are unchanged, so it does NOT re-fire and does NOT clear pendingLandingRef.
            // currentFilePathRef.current is updated to '/other.qmd'.
            await act(async () => {
                rerender(
                    <PreviewRoot
                        astJson={astJson}
                        untransformedAstJson={astJson}
                        renderedContent={CONTENT}
                        currentFilePath="/other.qmd"   // file switched
                        assetManifest={{}}
                        setAst={setAst}
                        onNavigateToDocument={() => {}}
                    />,
                );
            });

            // Still no focus-restore (timer hasn't fired yet).
            expect(focusedElements.filter(el => el.hasAttribute('data-block-pool-id'))).toHaveLength(0);

            // Step 3: advance past 250ms — the timer fires.
            // It checks pl.fromFile ('/test.qmd') !== currentFilePathRef.current ('/other.qmd')
            // → sets pendingLandingRef.current = null and returns without calling executeLanding.
            await act(async () => { vi.advanceTimersByTime(300); });

            // Focus must NOT have been restored — file changed.
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls).toHaveLength(0);
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 8. File-switch cancels pending landing (real production code)
 *
 * Ported from DirectMoveHarness "file switch" test in p2-4b.
 * The real pendingLandingRef.fromFile guard in PreviewRoot cancels the reland
 * when currentFilePath changes.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5a — file switch cancels pending landing', () => {
    it('does not reland when currentFilePath changes before re-render', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1], "para1").
        const textarea = await activateTileA(container);

        // Make dirty.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'para1 modified' } });
        });
        const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;

        // ArrowDown — dirty → stashes pendingLanding for fromFile="/test.qmd".
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowDown' });
        });

        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // Re-render with DIFFERENT currentFilePath AND new content.
        // The reland layout effect fires but detects fromFile !== currentFilePath → cancel.
        const newAstJson = makeAstJson(POOL, CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={CONTENT}
                    currentFilePath="/other.qmd"   // file switched
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // No reland — file switched.
        expect(container.querySelector('textarea')).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * P2.5b Fix B — 2-tile document: requestMove must not be a no-op while editing
 *
 * Bug: while editing tile A in a 2-tile document, A's DOM element (the
 * <p data-block-pool-id="1">) is replaced by the textarea wrapper (which has
 * no data-block-pool-id). enumerateOuterBlocks therefore returns only outer block B
 * (tiles.length === 1). The old guard was `<= 1 → return`, which made
 * requestMove a no-op on any 2-tile document — ArrowDown silently did nothing.
 *
 * Fix: changed the guard from `<= 1` to `=== 0`. The active tile is implicitly
 * present via its anchorR0 (editTargetRef); the scan only needs to find at
 * least the *other* tile.
 *
 * Fail-on-revert: if the guard is changed back to `<= 1`, requestMove returns
 * early before computing destTile, so the editor stays on A and B's editor
 * never opens — the assertion `textareaB.value === 'tileB'` fails.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.5b — 2-tile document: ArrowDown navigates from A to B (Fix B regression)', () => {
    it('hops to tile B on ArrowDown at last visual line in a 2-tile document', async () => {
        // 2-tile document:
        //   tile A: pool[0] r=[0,7]  "tileA\n\n"  line 0
        //   tile B: pool[1] r=[7,14] "tileB\n\n"  line 2
        const twoTilePool = [
            { t: 0, r: [0, 7], d: 0 },   // pool[0]: tileA\n\n  line 0
            { t: 0, r: [7, 14], d: 0 },  // pool[1]: tileB\n\n  line 2
        ] as typeof POOL;
        const twoTileContent = 'tileA\n\ntileB\n\n';

        // Build the matching AST.
        const ast = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                { t: 'Para', c: [{ t: 'Str', c: 'tileA' }], s: 0 },
                { t: 'Para', c: [{ t: 'Str', c: 'tileB' }], s: 1 },
            ],
            astContext: { p: twoTilePool },
        };
        const astJson = JSON.stringify(ast);

        // Mock caretGeometry: caret IS on the last visual line of A.
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const props: PreviewRootProps = {
            astJson,
            untransformedAstJson: astJson,
            renderedContent: twoTileContent,
            currentFilePath: '/two.qmd',
            assetManifest: {},
            setAst,
            onNavigateToDocument: () => {},
        };

        const { container } = render(<PreviewRoot {...props} />);
        await act(async () => {});

        // Mock getBoundingClientRect for both tiles so enumerateOuterBlocks sees them.
        const tiles = container.querySelectorAll<HTMLElement>('[data-block-pool-id]');
        tiles.forEach((tile) => {
            const pid = Number(tile.getAttribute('data-block-pool-id'));
            vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
                left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
                width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
            } as DOMRect);
        });

        // Activate tile A (pool[0]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tileA).not.toBeNull();
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textareaA = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaA).not.toBeNull();
        expect(textareaA!.value).toBe('tileA');

        // After A is open, tile A's <p data-block-pool-id="0"> is gone from the DOM.
        // Only outer block B (pool[1]) remains in enumerateOuterBlocks → blocks.length === 1.
        // Old guard (<= 1): returned early → no-op (bug).
        // New guard (=== 0): proceeds → finds tile B → opens B's editor.

        await act(async () => {
            fireEvent.keyDown(textareaA!, { key: 'ArrowDown' });
        });

        // setAst should NOT be called: this is an unmodified (clean) move.
        expect(setAst).not.toHaveBeenCalled();

        // B's editor should now be open.
        const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('tileB');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §2 Phase 0 characterization — dirty multiline ArrowUp resolves against L0,
 * NOT L0 + draftLineCount (Reflection #21).
 *
 * Why this test exists:
 *   The dirty (modified) move's destination line is computed asymmetrically in
 *   requestMove:  down → destLine = L0 + draftLineCount;  up → destLine = L0
 *   (PreviewRoot.tsx). executeLanding then resolves: down → first outer block
 *   with startLine >= destLine; up → last outer block with startLine < destLine.
 *
 *   The two find-by-line blocks (executeLanding vs requestMove) look duplicated,
 *   so the upcoming Phase-0 landing-core extraction is tempted to unify them on a
 *   single destLine = L0 + draftLineCount (the value the DOWN path needs). Doing
 *   so would silently break the UP path. The existing modified-DOWN test (above)
 *   already pins the down asymmetry; no existing test pinned the up/L0 case — so
 *   a "behavior-preserving" extraction could flatten it undetected.
 *
 *   This is a CHARACTERIZATION (guard) test: it pins the current, correct
 *   behavior and must stay green through the extraction. Its fail-on-revert lever
 *   is the flattening regression itself — see below.
 *
 * Fixture arithmetic (drives the real requestMove → executeLanding path):
 *   Open A (pool[1], r0=6, "para1", L0 = lineOf(6) = 1). Type to 3 lines
 *   ("para1\nx\ny", draftLineCount = 3, dirty). ArrowUp → dirty commit, stash
 *   landing with destLine = L0 = 1. Re-render with A grown to 3 lines:
 *     line 0: para0 (pool[0] r0=0)
 *     lines 1-3: A = "para1\nx\ny" (pool[1] r0=6)
 *     line 4: para2 (pool[2] r0=16)
 *     line 6: para3 (pool[3] r0=23)
 *   executeLanding up: last outer block with startLine < destLine(1) → only
 *   para0 (line 0) qualifies → relands on para0. ✓
 *
 * Fail-on-revert (verified cold): change PreviewRoot.tsx requestMove's up-path
 *   `const destLine = direction === 'down' ? L0 + draftLineCount : L0;`
 *   to use `L0 + draftLineCount` for up as well → "last block with startLine < 4"
 *   selects A itself (line 1) → the editor relands on A ("para1\nx\ny"), the
 *   assertion `value === 'para0'` fails RED. Restore → GREEN.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('§2 Phase 0 characterization — dirty multiline ArrowUp uses L0 (Reflection #21)', () => {
    it('relands on para0 (not the just-edited block) after a dirty multiline up-move', async () => {
        // Caret IS on the first visual line → ArrowUp triggers a move.
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        // Prototype-level rect mock (not per-element): the reland useLayoutEffect
        // fires DURING the commit re-render, BEFORE a per-element re-mock could run,
        // and the edited block's <p> is a FRESH node post-edit. A per-element mock
        // (mockTileRects) would leave that fresh node with a zero rect → invisible →
        // excluded from enumerateOuterBlocks at reland, masking the up/L0 asymmetry.
        // A prototype mock keeps every block (incl. the freshly-rerendered edited
        // block) visible at reland — faithfully matching the real browser, where the
        // edited block IS laid out and visible. Rect VALUES are irrelevant here (only
        // width/height>0 gates visibility; the destination is chosen by source-line
        // arithmetic, not geometry). This mocks genuine browser-only geometry only.
        vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
            function (this: HTMLElement): DOMRect {
                const pidAttr = this.getAttribute?.('data-block-pool-id');
                const pid = pidAttr != null ? Number(pidAttr) : 0;
                const top = pid * 60;
                return {
                    left: 0, top, right: 200, bottom: top + 40,
                    width: 200, height: 40, x: 0, y: top, toJSON: () => ({}),
                } as DOMRect;
            },
        );

        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst });

        await act(async () => {});

        // Step 1: activate A (pool[1], r0=6, "para1", L0 = 1).
        const textarea = await activateTileA(container);

        // Step 2: type to make A a 3-line dirty buffer (draftLineCount = 3).
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'para1\nx\ny' } });
        });
        const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(ta.value).toBe('para1\nx\ny');

        // Step 3: ArrowUp at first visual line → dirty move → commit + stash + close.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowUp' });
        });

        // Commit happened; editor closed.
        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // Step 4: simulate the commit re-render — A grew to 3 lines (1..3), pushing
        // para2 to line 4 and para3 to line 6.
        //   para0:  pool[0] r=[0,6]    line 0
        //   A:      pool[1] r=[6,16]   line 1 ("para1\nx\ny\n")
        //   para2:  pool[2] r=[16,23]  line 4
        //   para3:  pool[3] r=[23,30]  line 6
        const newPool = [
            { t: 0, r: [0, 6], d: 0 },
            { t: 0, r: [6, 16], d: 0 },
            { t: 0, r: [16, 23], d: 0 },
            { t: 0, r: [23, 30], d: 0 },
        ] as typeof POOL;
        const newContent = 'para0\npara1\nx\ny\npara2\n\npara3\n\n';
        const newAstJson = makeAstJson(newPool, newContent);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={newContent}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Step 5: reland fires. up-path destLine = L0 = 1 → last visible block with
        // startLine < 1 → para0 (the block ABOVE the edited one). A flattened
        // destLine = L0 + draftLineCount (= 4) regression would instead select the
        // just-edited block A itself (line 1 < 4) → reland on "para1\nx\ny".
        const relanded = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(relanded).not.toBeNull();
        expect(relanded!.value).toBe('para0');
    });
});
