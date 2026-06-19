/**
 * G19 integration test: spurious "dirty" detection on a clean nested block.
 *
 * The bug (before fix): `handleClickSwitchBlur` compared the draft against the
 * RAW `et.anchorSlice` instead of the canonical seededDraft baseline. For a
 * nested block, the raw slice carries ancestor `> `/indent prefixes that the
 * clean buffer strips. An UNTOUCHED nested editor ("oh") would read dirty
 * against its raw slice ("> oh\n> ") and fire a null commit via setAst.
 *
 * The fix (G19): route site 3 (handleClickSwitchBlur) through `isDirty(draft, et)`
 * from outerBlocks.ts, which always uses `seededDraft ?? anchorSlice` as the
 * baseline — the same canonical baseline the other four comparison sites use.
 *
 * Test harness notes:
 *  - `nestedEditBuffers: {'0:6-14:0': 'oh'}` injects the clean buffer for pool[1].
 *    Key format: `serializeSourceEntry({t:0, r:[r0,r1], d:0})` = `"0:6-14:0"`.
 *  - `unlockNestingCursor: true` enables the P3.3 seededDraft path.
 *  - After activating pool[1], `textarea.value === 'oh'` proves the clean buffer
 *    was used (NOT the raw slice "> oh\n> "). This is the non-vacuity exercise
 *    check: the divergence the fix depends on is present before the blur runs.
 *  - No typing — draft stays "oh". Then pointerdown B (pool[2]) → blur A.
 *  - Assert `setAst` NOT called: the clean editor must be detected as clean.
 *
 * Named revert → RED:
 *   Revert site 3 in handleClickSwitchBlur to compare against `et.anchorSlice`
 *   (the raw slice). The raw slice for pool[1] in this fixture is "> oh\n> "
 *   (two-line nested blockquote content). "oh" !== "> oh\n> " → isDirty true
 *   → setAst called once with newText:"oh", r:[6,14] → assertion fails.
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
 * init dict. Object.defineProperty forces the value so React sees 'mouse'.
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

/* ─── Document fixture ───────────────────────────────────────────────────────
 *
 * Document: 'intro\n> oh\n> \npara2\n'  (20 bytes)
 *
 * Pool (matching the spec):
 *   pool[0]: t=0, r=[0,6],   d=0  → "intro\n"          (top-level Para)
 *   pool[1]: t=0, r=[6,14],  d=0  → "> oh\n> \n"        (nested, A — edit target)
 *   pool[2]: t=0, r=[14,20], d=0  → "para2\n"           (top-level Para, B)
 *
 * nestedEditBuffers: { '0:6-14:0': 'oh' }
 *   The key is serializeSourceEntry({t:0, r:[6,14], d:0}) = '0:6-14:0'.
 *   The value 'oh' is the stripped clean buffer — what seedForRange returns
 *   when the pool[1] entry has a nestedEditBuffer (vs anchorSlice "> oh\n>").
 *
 * The divergence: anchorSlice for pool[1] is "> oh\n>" (raw, after trimEnd);
 * seededDraft is "oh". An untouched editor has draft === "oh".
 *   - Bug (raw slice baseline): "oh" !== "> oh\n>" → isDirty → null commit.
 *   - Fix (seededDraft baseline): "oh" === "oh" → clean → no commit.
 */
const CONTENT = 'intro\n> oh\n> \npara2\n';

const POOL = [
    { t: 0, r: [0, 6],   d: 0 },   // pool[0]: "intro\n"
    { t: 0, r: [6, 14],  d: 0 },   // pool[1]: "> oh\n> \n" — A (nested blockquote)
    { t: 0, r: [14, 20], d: 0 },   // pool[2]: "para2\n"    — B
];

// nestedEditBuffers key: serializeSourceEntry({t:0, r:[6,14], d:0})
const NESTED_EDIT_BUFFERS: Record<string, string> = {
    '0:6-14:0': 'oh',
};

function makeAstJson(pool: typeof POOL, content: string): string {
    const blocks = pool.map((entry, i) => {
        const raw = content.slice(entry.r[0], entry.r[1]);
        const text = raw.replace(/\n/g, '').replace(/^[>\s]+/, '').trim() || `tile${i}`;
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

/**
 * Mock getBoundingClientRect on all [data-block-pool-id] tiles.
 * enumerateOuterBlocks skips elements with zeroed rects (invisible);
 * we give each a distinct non-zero rect so all tiles are treated as visible.
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

function mountPreviewRoot(setAst: (ast: PandocAST) => void) {
    const astJson = makeAstJson(POOL, CONTENT);
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
        unlockNestingCursor: true,
        nestedEditBuffers: NESTED_EDIT_BUFFERS,
    };
    return render(<PreviewRoot {...props} />);
}

/* ─── G19-1: headline — no null commit on an untouched nested editor ─────────
 *
 * Sequence:
 *   1. Mount with nestedEditBuffers injecting 'oh' for pool[1].
 *   2. Activate A (pool[1]) via pointerdown + pointerup.
 *   3. Assert textarea.value === 'oh' — proves the clean buffer was seeded
 *      (exercise check: seededDraft 'oh' ≠ raw anchorSlice, divergence is live).
 *   4. Do NOT type.
 *   5. pointerdown on B (pool[2]) — records click-switch.
 *   6. blur on A's textarea — handleClickSwitchBlur runs.
 *   7. Assert setAst NOT called (the clean editor must not fire a null commit).
 *
 * Named revert → RED:
 *   In handleClickSwitchBlur, revert the dirty check to compare against
 *   et.anchorSlice (the raw slice). For pool[1], anchorSlice is "> oh\n>"
 *   (or similar after trimEnd). "oh" !== "> oh\n>" → isDirty true → setAst
 *   called once with newText "oh", r:[6,14] → `expected 0 calls, got 1` → RED.
 */
describe('G19 — spurious dirty detection on clean nested block', () => {
    it('does not call setAst when a nested editor is untouched (no null commit)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot(setAst);

        // Wait for initial render to settle, then mock tile rects.
        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate A (pool[1], nested block with clean buffer 'oh').
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileA).not.toBeNull();

        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // A's editor should be open with the clean buffer seeded.
        const textarea = container.querySelector('textarea');
        expect(textarea).not.toBeNull();

        // Exercise check: textarea.value === 'oh' proves the seededDraft is active.
        // This means the divergence (seededDraft 'oh' ≠ raw anchorSlice "> oh\n>")
        // is genuinely present — the fix is non-vacuous.
        expect(textarea!.value).toBe('oh');

        // Step 2: do NOT type — draft stays 'oh' (clean).

        // Step 3: pointerdown on B (pool[2]) — records a pending click-switch.
        const tileB = container.querySelector<HTMLElement>('[data-block-pool-id="2"]');
        expect(tileB).not.toBeNull();

        await act(async () => {
            fireEvent(tileB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        });

        // setAst not called yet (pointerdown alone never commits).
        expect(setAst).not.toHaveBeenCalled();

        // Step 4: blur on A — handleClickSwitchBlur runs.
        // With the fix: isDirty(draft='oh', et) uses editBaseline(et) = 'oh'
        //   → 'oh' === 'oh' → clean → returns false → blur takes normal close path.
        // Without the fix (revert): 'oh' !== et.anchorSlice ("> oh\n>") → dirty
        //   → setAst called once → assertion below fails.
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // Headline assertion: no null commit fired.
        expect(setAst).not.toHaveBeenCalled();
    });
});
