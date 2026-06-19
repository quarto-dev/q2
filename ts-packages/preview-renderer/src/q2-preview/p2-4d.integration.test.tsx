/**
 * P2.4d integration tests: click-switch between tiles.
 *
 * These tests exercise the REAL production path through PreviewRoot —
 * the actual requestClickSwitch / handleClickSwitchBlur / activate
 * machinery in PreviewRoot.tsx, the real onPointerDown /
 * onPointerUp classification in useBlockEditHover.tsx, and the real onBlur /
 * commitIfDirty flow in EditTextarea (dispatchers.tsx).
 *
 * Coverage:
 *
 *  1. Dirty click-switch (B after A): edit tile A, type (dirty), click tile B →
 *     A is committed (setAst spy called), B opens DIRECTLY via pointerup activate
 *     (no deferred reland). Assert immediately after pointerup(B).
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
 * jsdom gotchas:
 *  - getBoundingClientRect returns zeroes by default. Must be mocked on tile
 *    elements for enumerateOuterBlocks to treat them as visible.
 *  - PointerEvent.pointerType is NOT honoured from the constructor in jsdom 26.
 *    Use Object.defineProperty via the ptrEvent helper (from useBlockEditHover
 *    integration tests).
 *  - getComputedStyle returns empty strings — measureBlockBox returns { contentHeight: 0,
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
 * the container so enumerateOuterBlocks treats them as visible.
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

/* ─── 1. Dirty click-switch: B after A (direct activation — G18 Layer 1) ────
 *
 * G18 fix: when A is dirty and B is clicked, handleClickSwitchBlur commits A
 * fire-and-forget and closes A directly — it does NOT stash a pendingLanding.
 * onPointerUp then activates B unconditionally (the old deferred-reland path
 * and its dirtySwitchHandledRef flag were removed entirely, so "B opens on the
 * first click" is now true by construction).
 *
 * Sequence: pointerdown(A)/up(A) → type → pointerdown(B) → blur(A) → pointerup(B)
 * Assertions: B's textarea present IMMEDIATELY after pointerup(B), value==="para2",
 *             setAst called exactly once for A's commit.
 *
 * Fail-on-revert (the load-bearing binding): delete the `setAstRef.current({...})`
 * commit in the isDirty branch of handleClickSwitchBlur → A is never committed →
 * `expect(setAst).toHaveBeenCalledOnce()` fails with `expected 1, got 0` → RED.
 * This binds the actual G18 contract (a dirty click-switch commits A exactly
 * once). The "B present" assertion is a by-construction regression guard: with
 * the deferred-reland machinery gone there is no code path that can withhold B.
 */
describe('P2.4d — dirty click-switch: B after A (direct activation, no deferred reland)', () => {
    it('commits A and opens B directly on pointerup (no deferred reland)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

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

        // Step 2: type into A's textarea to make it dirty.
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'para1edited' } });
        });

        textarea = container.querySelector('textarea')!;
        expect(textarea!.value).toBe('para1edited');

        // Step 3: pointerdown on B (pool[2], r0=12). This fires BEFORE blur.
        // useBlockEditHover.onPointerDown classifies it as click-switch → calls requestClickSwitch.
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        expect(tileB).not.toBeNull();

        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // setAst not yet called (pointerdown alone doesn't commit).
        expect(setAst).not.toHaveBeenCalled();

        // Step 4: blur on A's textarea. G18 handleClickSwitchBlur (direct-activate path):
        //   - detects dirty draft ("para1edited" !== "para1")
        //   - commits A via setAst
        //   - does NOT stash a pendingLanding
        //   - closes editor (setEditTargetRaw(null))
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // Commit happened: setAst was called exactly once with A's edit.
        expect(setAst).toHaveBeenCalledOnce();
        const commitPayload = setAst.mock.calls[0][0] as any;
        expect(commitPayload.__isPreviewNodeEdit).toBe(true);
        expect(commitPayload.channel).toBe('text');
        expect(commitPayload.newText).toContain('para1edited');

        // Editor is now closed — no textarea in the DOM.
        expect(container.querySelector('textarea')).toBeNull();

        // Step 5: pointerup on B → activate(B) proceeds unconditionally →
        // B opens IMMEDIATELY (no reland needed).
        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // B's textarea must be present immediately (no re-render/reland needed).
        const textareaB = container.querySelector('textarea');
        expect(textareaB).not.toBeNull();
        expect(textareaB!.value).toBe('para2');
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

        // pointerup on B — activate(B) proceeds (no pending click-switch was consumed).
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
