/**
 * P2.4d integration tests: click-switch between tiles.
 *
 * These tests exercise the REAL production path through PreviewRoot —
 * the actual requestClickSwitch / handleClickSwitchBlur / consumeDirtySwitchHandled
 * / executeLanding / reland machinery in PreviewRoot.tsx, the real onPointerDown /
 * onPointerUp classification in useBlockEditHover.tsx, and the real onBlur /
 * commitIfDirty flow in EditTextarea (dispatchers.tsx).
 *
 * Reverting any of the P2.4d production changes (entry.tsx → PreviewRoot.tsx,
 * dispatchers.tsx, useBlockEditHover.tsx, PreviewContext.tsx) must cause
 * the dirty click-switch test to FAIL (verified during TDD).
 *
 * Coverage:
 *
 *  1. Dirty click-switch (B after A): edit tile A, type (dirty), click tile B →
 *     A is committed (setAst spy called), after re-render B opens via projected
 *     landing. B's anchorR0 shifts because A grew; projection must land on B,
 *     NOT on the wrong tile.
 *
 *  2. Unmodified click-switch: edit A (no typing), click B → B opens directly
 *     via pointerup activate; no setAst call (no commit).
 *
 *  3. Click inside the active region is a caret-move (P1 preserved): click
 *     inside A's editor region does NOT set clickSwitch, does NOT commit.
 *
 *  4. Click to empty area is plain close (P2.4c preserved): click outside any
 *     tile while editing A → plain close, no switch, focus restored.
 *
 *  5. Byte-identical dirty click-switch: commit produces byte-identical output
 *     (props unchanged) → timeout fallback still relands B.
 *
 * jsdom gotchas:
 *  - getBoundingClientRect returns zeroes by default. Must be mocked on tile
 *    elements for enumerateLockedTiles to treat them as visible.
 *  - PointerEvent.pointerType is NOT honoured from the constructor in jsdom 26.
 *    Use Object.defineProperty via the ptrEvent helper (from useBlockEditHover
 *    integration tests).
 *  - getComputedStyle returns empty strings — measureTileBox returns { contentHeight: 0,
 *    boxStyle: {} } which is fine; the editor still opens and the textarea is found.
 *
 * Why PreviewRoot and not entry.tsx:
 *  - entry.tsx has module-top side effects (Bootstrap injection, message listener,
 *    window.__Q2_PREVIEW_RENDERER__ global) that would pollute the test environment.
 *  - PreviewRoot is the component that owns all P2.4d state and callbacks.
 *    Importing it directly gives a clean, side-effect-free mount.
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

/* ─── PointerEvent helper ────────────────────────────────────────────────────
 * jsdom's PointerEvent does not honour `pointerType` from the constructor
 * init dict, so React sees e.pointerType === undefined. Object.defineProperty
 * forces the value onto the event object so the hook's `e.pointerType !== 'mouse'`
 * branch evaluates correctly.
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
 * Four-paragraph document:
 *   para0: pool[0] r=[0,6]    "para0\n"   line 0
 *   para1: pool[1] r=[6,12]   "para1\n"   line 1  ← A (edit target)
 *   para2: pool[2] r=[12,19]  "para2\n\n" line 2  ← B (click-switch target after A)
 *   para3: pool[3] r=[19,26]  "para3\n\n" line 4
 *
 * anchorSlice values (normalizeLineEndings + trimEnd):
 *   para0 → "para0"   para1 → "para1"
 *   para2 → "para2"   para3 → "para3"
 *
 * AST blocks have `s` (source pool index) matching their pool entry.
 * The same JSON is used for both astJson and untransformedAstJson so
 * buildSourceIndex finds the entries (both pools are identical).
 */
const CONTENT = 'para0\npara1\npara2\n\npara3\n\n';

const POOL = [
    { t: 0, r: [0, 6], d: 0 },    // pool[0]: para0\n   line 0
    { t: 0, r: [6, 12], d: 0 },   // pool[1]: para1\n   line 1 (A)
    { t: 0, r: [12, 19], d: 0 },  // pool[2]: para2\n\n line 2 (B after A)
    { t: 0, r: [19, 26], d: 0 },  // pool[3]: para3\n\n line 4
];

function makeAstJson(pool: typeof POOL, content: string): string {
    // Build Para blocks from the content. Each block has:
    //  - s: pool index
    //  - c: inlines derived from the trimmed slice
    // The inline text is stripped of newlines for display; the source identity
    // comes from the pool entry alone.
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

/** Make a standard 4-tile PreviewRoot render for testing. */
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
        untransformedAstJson: astJson,  // same pool → sourceIndex resolves all blocks
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
 * Mock getBoundingClientRect on all [data-block-pool-id] elements inside
 * the container so enumerateLockedTiles treats them as visible.
 *
 * Each tile gets a distinct non-zero rect (top = poolId * 60).
 * Must be called AFTER the initial render so the elements exist in the DOM.
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

/* ─── 1. Dirty click-switch: B after A (projected landing) ──────────────────
 *
 * The must-have test. Verifies:
 *  - P2.4d in useBlockEditHover: onPointerDown while editing calls requestClickSwitch.
 *  - P2.4d in PreviewRoot: requestClickSwitch records B.
 *  - P2.4d in dispatchers: onBlur calls handleClickSwitchBlur → commits A (setAst),
 *    stashes pendingLanding, closes without focus-restore.
 *  - P2.4d in useBlockEditHover: onPointerUp calls consumeDirtySwitchHandled → suppresses activate.
 *  - Reland layout effect (PreviewRoot): on re-render with new pool/content, opens B.
 *
 * Mandatory fail-on-revert: reverting any P2.4d production change breaks this test.
 */
describe('P2.4d — dirty click-switch: B after A (projected landing)', () => {
    it('commits A and opens B after re-render using projected destLine', async () => {
        const setAst = vi.fn();
        const { container, rerender, pool, content } = mountPreviewRoot({ setAst });

        // Wait for initial render to settle, then mock tile rects.
        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate tile A (pool[1], r0=6, "para1") via mouse click.
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileA).not.toBeNull();

        // pointerdown + pointerup on A activates it.
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // A's editor should be open: a textarea is rendered in the document.
        let textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();
        // The textarea is seeded with A's anchorSlice ("para1").
        expect(textarea!.value).toBe('para1');

        // Step 2: type into A's textarea to make it dirty (add "\nextra").
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'para1\nextra' } });
        });

        // Re-query textarea (same element, value updated).
        textarea = container.querySelector('textarea')!;
        expect(textarea!.value).toBe('para1\nextra');

        // Step 3: pointerdown on B (pool[2], r0=12). This fires BEFORE blur.
        // useBlockEditHover.onPointerDown classifies it as click-switch → calls requestClickSwitch.
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        expect(tileB).not.toBeNull();

        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // setAst not yet called (pointerdown alone doesn't commit).
        expect(setAst).not.toHaveBeenCalled();

        // Step 4: blur on A's textarea. The real handleClickSwitchBlur:
        //   - detects dirty draft ("para1\nextra" !== "para1")
        //   - commits A via setAst (PreviewNodeEditPayload)
        //   - stashes pendingLanding for B
        //   - sets dirtySwitchHandledRef
        //   - closes editor (textarea disappears)
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // Commit happened: setAst was called once with the edited text.
        expect(setAst).toHaveBeenCalledOnce();
        const commitPayload = setAst.mock.calls[0][0] as any;
        expect(commitPayload.__isPreviewNodeEdit).toBe(true);
        expect(commitPayload.channel).toBe('text');
        // The committed text should be the dirty draft (normalized).
        expect(commitPayload.newText).toContain('para1');
        expect(commitPayload.newText).toContain('extra');

        // Editor is now closed — no textarea in the DOM.
        expect(container.querySelector('textarea')).toBeNull();

        // Step 5: pointerup on B. The real consumeDirtySwitchHandled returns true
        // → activate(B) is suppressed. B does NOT open yet.
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // Still no textarea (landing pending, not relanded yet).
        expect(container.querySelector('textarea')).toBeNull();

        // Step 6: Simulate the commit re-render. A expanded by 1 line (+6 bytes).
        // Post-commit pool: tile 2 (B) shifts to r=[18,25], line 3.
        const newPool = [
            { t: 0, r: [0, 6], d: 0 },    // para0 (unchanged)
            { t: 0, r: [6, 18], d: 0 },   // para1\nextra (A expanded)
            { t: 0, r: [18, 25], d: 0 },  // para2\n\n (B shifted)
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

        // Re-mock rects for the new tile elements.
        mockTileRects(container);

        // Reland layout effect fires → B's editor opens.
        // B (tile 2 in new pool) has anchorR0=18.
        const textareaAfterReland = container.querySelector('textarea');
        expect(textareaAfterReland).not.toBeNull();

        // B's tile (data-block-pool-id="2") should be in edit mode.
        // The pool entry for pool[2] in the NEW pool is r=[18,25] → anchorSlice="para2".
        // The textarea value is seeded from anchorSlice.
        expect(textareaAfterReland!.value).toBe('para2');

    });
});

/* ─── 2. Unmodified click-switch (regression) ───────────────────────────────
 *
 * Edit A (no typing), click B → B opens directly via pointerup activate.
 * No setAst call (dirty guard: draft === anchorSlice → no commit).
 */
describe('P2.4d — unmodified click-switch', () => {
    it('opens B directly via activate (no commit, no reland)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para1');

        // Do NOT type — draft stays equal to anchorSlice.

        // pointerdown on B (records click-switch).
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // blur — unmodified path: clickSwitchRef cleared, normal close (no commit).
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // No commit.
        expect(setAst).not.toHaveBeenCalled();
        // A's editor closed.
        expect(container.querySelector('textarea')).toBeNull();

        // pointerup on B — activate(B) proceeds (consumeDirtySwitchHandled returns false).
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // B's editor opens immediately (pool[2] → anchorSlice="para2").
        const taB = container.querySelector('textarea');
        expect(taB).not.toBeNull();
        expect(taB!.value).toBe('para2');
    });
});

/* ─── 3. Click inside active region is caret-move (P1 preserved) ────────────
 *
 * useBlockEditHover.onPointerDown: if click lands inside activeEditRegionRef,
 * no click-switch is set. Subsequent blur takes the plain-close path, no commit.
 */
describe('P2.4d — click inside active region does not switch', () => {
    it('pointerdown inside editor does NOT set clickSwitch; blur takes plain-close path', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();

        // pointerdown ON the textarea itself (inside the active edit region).
        // useBlockEditHover.onPointerDown: activeEditRegionRef.contains(target) → true → return early.
        await act(async () => {
            fireEvent(textarea!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // blur — no clickSwitch set → plain-close path (no commit since unmodified).
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // No commit.
        expect(setAst).not.toHaveBeenCalled();
        // A's editor closed.
        expect(container.querySelector('textarea')).toBeNull();
    });
});

/* ─── 4. Click to empty area is plain close (P2.4c preserved) ───────────────
 *
 * pointerdown on empty area (no tile) → no click-switch recorded.
 * blur: plain-close → requestFocusRestore(A.anchorR0).
 * No commit, no click-switch.
 */
describe('P2.4d — click to empty area is plain close (P2.4c preserved)', () => {
    it('pointerdown on empty area → plain close, no switch, no commit', async () => {
        vi.useFakeTimers();
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();

        // pointerdown on the host (PreviewDocument's root div) — NOT on a tile.
        // findEditTarget looks for closest('[data-block-pool-id]') which returns null.
        // → no click-switch recorded.
        const host = container.firstElementChild!;
        await act(async () => {
            fireEvent(host, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // blur — plain-close (no click-switch → requestFocusRestore path).
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // No commit (draft unmodified).
        expect(setAst).not.toHaveBeenCalled();
        // Editor closed.
        expect(container.querySelector('textarea')).toBeNull();

        // Focus-restore timeout fires (P2.4c). Should not throw.
        await act(async () => {
            vi.advanceTimersByTime(300);
        });
    });
});

/* ─── 6. Dirty click-switch delta off-by-one discriminator ──────────────────
 *
 * Fixture geometry is designed so that the wrong delta (0, produced by the
 * `+ '\n'` bug in anchorSliceLineCount) and the correct delta (+1, produced
 * by the fix) resolve to DIFFERENT tiles in the post-commit pool.
 *
 * Pre-commit pool (4 tiles):
 *   pool[0]: "para0\n"   r=[0,6]   line 0
 *   pool[1]: "paraA\n"   r=[6,12]  line 1  ← A (1-line anchorSlice)
 *   pool[2]: "paraX\n"   r=[12,18] line 2  ← undershoot tile (wrong delta lands here)
 *   pool[3]: "paraB\n\n" r=[18,26] line 3  ← B (correct landing target)
 *
 * Draft of A → "paraA\nextra" (2 lines). draftLineCount=2, anchorSliceLineCount=1.
 *   Correct delta = +1.
 *   L_B (pre-commit) = 3 (line of pool[3].r[0]=18 in "para0\nparaA\nparaX\nparaB\n\n")
 *   With wrong  delta (0): destLine = L_B + 0 = 3
 *   With correct delta (1): destLine = L_B + 1 = 4
 *
 * Post-commit pool (A grew by 1 line):
 *   pool[0]: [0,6]   line 0
 *   pool[1]: [6,18]  line 1  ("paraA\nextra\n" — spans lines 1-2)
 *   pool[2]: [18,24] line 3  ("paraX\n" — undershoot tile)
 *   pool[3]: [24,32] line 4  ("paraB\n\n" — B)
 *
 * executeLanding(direction='down', destLine=3, newContent) → first tile at
 *   line >= 3 = pool[2] (line 3) — WRONG (anchorSlice = "paraX")
 * executeLanding(direction='down', destLine=4, newContent) → first tile at
 *   line >= 4 = pool[3] (line 4) — CORRECT (anchorSlice = "paraB")
 *
 * This test FAILS with the `+ '\n'` bug (opens paraX instead of paraB)
 * and PASSES after the fix (opens paraB correctly).
 */
describe('P2.4d — dirty click-switch delta: off-by-one discriminator', () => {
    it('lands on B (not the undershoot tile) when A grows by 1 line', async () => {
        const setAst = vi.fn();

        // Pre-commit content and pool.
        const content = 'para0\nparaA\nparaX\nparaB\n\n';
        const pool = [
            { t: 0, r: [0, 6],  d: 0 },   // pool[0]: para0\n   line 0
            { t: 0, r: [6, 12], d: 0 },   // pool[1]: paraA\n   line 1  (A)
            { t: 0, r: [12, 18], d: 0 },  // pool[2]: paraX\n   line 2  (undershoot)
            { t: 0, r: [18, 26], d: 0 },  // pool[3]: paraB\n\n line 3  (B)
        ] as typeof POOL;

        const { container, rerender } = mountPreviewRoot({ setAst, pool, content });
        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileA).not.toBeNull();
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('paraA');

        // Make A dirty: 1-line → 2-line (delta should be +1, not 0).
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'paraA\nextra' } });
        });

        // pointerdown on B (pool[3]).
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="3"]');
        expect(tileB).not.toBeNull();
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // blur — dirty switch: commits A, stashes pendingLanding for B, closes.
        const ta = container.querySelector('textarea')!;
        await act(async () => {
            fireEvent.blur(ta);
        });

        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // pointerup on B — suppressed (dirty switch handled).
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // Simulate re-render: A grew by 1 line.
        // New content: "para0\nparaA\nextra\nparaX\nparaB\n\n"
        // New pool:
        //   pool[0]: [0,6]   line 0
        //   pool[1]: [6,18]  line 1  (paraA\nextra)
        //   pool[2]: [18,24] line 3  (paraX — undershoot tile if delta=0)
        //   pool[3]: [24,32] line 4  (paraB — correct if delta=+1)
        const newContent = 'para0\nparaA\nextra\nparaX\nparaB\n\n';
        const newPool = [
            { t: 0, r: [0, 6],  d: 0 },   // para0
            { t: 0, r: [6, 18], d: 0 },   // paraA\nextra
            { t: 0, r: [18, 24], d: 0 },  // paraX (undershoot)
            { t: 0, r: [24, 32], d: 0 },  // paraB (B)
        ] as typeof POOL;
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
        mockTileRects(container);

        // Reland fires. Correct delta=+1 → destLine=4 → lands on paraB (pool[3]).
        // Bug delta=0 → destLine=3 → lands on paraX (pool[2]).
        const textareaAfter = container.querySelector('textarea');
        expect(textareaAfter).not.toBeNull();

        // Must be B (paraB), NOT the undershoot tile (paraX).
        expect(textareaAfter!.value).toBe('paraB');
    });
});

/* ─── 5. Byte-identical dirty click-switch — timeout fallback ────────────────
 *
 * Dirty click-switch from A (pool[1]) to B (pool[2]).
 * No re-render (byte-identical commit). Timeout fallback relands B.
 *
 * Draft "para1x" is dirty (≠ "para1") and same line count as anchorSlice
 * → delta=0, destLine=L_B=2. executeLanding(down, destLine=2) in original
 * pool: first tile at line >= 2 = pool[2] (r0=12, "para2"). ✓
 */
describe('P2.4d — byte-identical dirty click-switch uses timeout fallback', () => {
    it('relands B via timeout when commit is byte-identical (props unchanged)', async () => {
        vi.useFakeTimers();
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (pool[1]).
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe('para1');

        // Type dirty text (same line count as anchorSlice → delta=0).
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'para1x' } });
        });
        textarea = container.querySelector('textarea')!;

        // pointerdown on B.
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // blur — dirty switch → commit + stash landing + close.
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // pointerup — suppressed.
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // No re-render (byte-identical). B not yet open.
        expect(container.querySelector('textarea')).toBeNull();

        // Timeout fires after 250ms → timeout fallback relands B.
        await act(async () => {
            vi.advanceTimersByTime(300);
        });

        // B (pool[2], r0=12, anchorSlice="para2") editor opens.
        const textareaB = container.querySelector('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('para2');
    });
});
