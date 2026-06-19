/**
 * §1 Test 1.c — UNLOCK line-anchored navigation integration tests.
 *
 * Verifies that in UNLOCK mode (unlockNestingCursor=true) the up/down arrow
 * keys navigate leaf-to-leaf via surfaceAtLine (Rule B, §1 C1/A2), with:
 *   - Skipping container-gap/blank lines (advance L by 1 until a leaf is found).
 *   - Clamp at document ends (NO wrap).
 *
 * Fixture: two sequential blockquotes, each containing one Para:
 *
 *   content = '> AAA\n\n> BBB\n'  (13 bytes)
 *    > 0 ' '1 A2 A3 A4 \n5  \n6  > 7 ' '8 B9 B10 B11 \n12
 *
 *   pool:
 *     pool[0] BQ1   r=[0,5]   "> AAA"
 *     pool[1] ParaA r=[2,5]   "AAA"    (leaf inside BQ1)
 *     pool[2] BQ2   r=[7,12]  "> BBB"
 *     pool[3] ParaB r=[9,12]  "BBB"    (leaf inside BQ2)
 *
 *   Source-index surfaces (from buildNestingSurfaces):
 *     BQ1   [0,5]  TopLevel   trimmed span [0,0]   (has child ParaA → container)
 *     ParaA [2,5]  Descendable trimmed span [0,0]  (leaf)
 *     BQ2   [7,12] TopLevel   trimmed span [2,2]   (has child ParaB → container)
 *     ParaB [9,12] Descendable trimmed span [2,2]  (leaf)
 *
 *   Line map:
 *     line 0: "> AAA"  bytes 0-4
 *     line 1: ""       byte 6  (blank — gap between blockquotes)
 *     line 2: "> BBB"  bytes 7-11
 *
 * Navigation assertions (UNLOCK):
 *   Down from ParaA (line 0):
 *     destLine = L0(0) + draftLineCount(1) = 1.
 *     surfaceAtLine(1) = null (gap — container BQ1 is over, BQ2 hasn't started).
 *     Advance L to 2. surfaceAtLine(2) = ParaB (leaf, depth 1).
 *     → Lands on ParaB, value = 'BBB'. ✓
 *
 *   Down from ParaB (line 2):
 *     destLine = 2 + 1 = 3. No surface covers line 3 → clamp (docEndLine=2).
 *     → No-op (editor stays on ParaB). ✓
 *
 *   Up from ParaB (line 2):
 *     destLine = L0(2) = 2 (up-path: destLine = L0, not L0+draftLineCount).
 *     surfaceAtLine(2) = ParaB itself — but we need to go UP from it.
 *     Wait: the up path tries destLine = L0 = 2, which finds ParaB — but that is
 *     the CURRENT surface. This is actually correct behavior: the current surface
 *     IS at line 2, so ArrowUp with destLine=2 finds a surface at/above line 2.
 *
 *     Actually, let's reconsider: up path uses `destLine = L0`, and the resolver
 *     looks for a surface at L = destLine. surfaceAtLine(set, 2) = ParaB (same
 *     surface). So in UNLOCK mode, ArrowUp from ParaB would land on ParaB itself
 *     — which is a no-op (same block).
 *
 *     Hmm. But that's not right. The spec says "up = current source line − 1".
 *     Let me re-read the destLine computation:
 *       `const destLine = direction === 'down' ? L0 + draftLineCount : L0;`
 *     Up uses L0 (the anchor line), NOT L0-1. This matches the LOCKED mode
 *     semantics: "last outer block with startLine < destLine" = last outer block
 *     before L0 (not including L0 itself).
 *
 *     But for UNLOCK, `surfaceAtLine(set, L0)` finds the current surface (ParaB).
 *     The resolver would return ParaB — same as current. That IS a no-op... but
 *     there IS a surface below L0 (line 0). We want to land on it when going up.
 *
 *     The issue: the UNLOCK up-path needs to find the surface BEFORE L0, but
 *     surfaceAtLine(set, L0=2) = ParaB (the current one). We need to go to L0-1.
 *
 *     Resolution: for the UP direction, the resolver starts from destLine=L0 and
 *     steps BACKWARD (step=-1). Starting at L=2 gives ParaB. We need to start at
 *     L=1 (destLine - 1) for up, or handle the "same surface" case.
 *
 *     Actually wait — re-reading the outerByLine locked logic:
 *       up: "last outer block with startLine < destLine (NOT <=)"
 *     This uses strict-less-than: `lineOf(entry.r[0]) < spec.destLine`.
 *     So for LOCKED, up from ParaB (L0=2, destLine=2) finds last block where
 *     startLine < 2 → BQ1 (line 0). That's the correct jump.
 *
 *     For UNLOCK, we need the same semantics: find the surface whose line is
 *     STRICTLY BEFORE L=destLine, not at or after. So the UNLOCK up-path must
 *     start at L = destLine - 1 (not destLine) when going up.
 *
 *     This is a subtlety in the resolver. The current implementation starts at
 *     L = spec.destLine and iterates with step = -1. For up, destLine = L0 = 2.
 *     surfaceAtLine(set, L=2) = ParaB — the current block! We'd land on ourselves.
 *
 *     To fix: for the UP direction, start at L = spec.destLine - 1.
 *     For DOWN, start at L = spec.destLine (the current impl is correct for down).
 *
 * Wait — let me re-read the down case too:
 *   Down from ParaA: L0=0, draftLineCount=1, destLine=0+1=1.
 *   surfaceAtLine(set, 1) = null (gap), advance to 2, finds ParaB. ✓
 *   The DOWN path correctly starts at destLine = L0+draftLineCount (the line AFTER
 *   the current block), so it skips the current block.
 *
 *   Up from ParaB: L0=2, destLine=2. Starting at 2 gives ParaB (current block).
 *   We must start at destLine - 1 = 1 for up. At L=1: gap → null, advance to 0:
 *   ParaA [2,5] span [0,0] contains L=0 → ParaA. ✓
 *
 * So the UP direction in the UNLOCK resolver must start at L = spec.destLine - 1,
 * not spec.destLine. This is an implementation correction to make up work correctly.
 *
 * This test file documents and pins this behavior.
 *
 * Fail-on-revert:
 *   ArrowDown from ParaA: if the resolver uses the LOCKED path (enumerateOuterBlocks)
 *     in UNLOCK mode, it would land on BQ2 (value='> BBB') → test RED.
 *   ArrowUp from ParaB: if the up-path starts at destLine instead of destLine-1,
 *     surfaceAtLine(set, 2) = ParaB → no-op → editor stays on ParaB → test RED.
 *   Clamp at ParaB (down from ParaB): if wrap is still present, editor would open
 *     ParaA → test RED.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
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

/* ─── PointerEvent helper (same as p2-4-real) ───────────────────────────────── */
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

/* ─── Fixture ────────────────────────────────────────────────────────────────── */

// content (13 bytes): "> AAA\n\n> BBB\n"
// line 0: "> AAA"  bytes 0-5 (incl \n at 5)
// line 1: ""       byte 6  (\n)
// line 2: "> BBB"  bytes 7-12 (incl \n at 12)
const CONTENT_1C = '> AAA\n\n> BBB\n';
const POOL_1C = [
    { t: 0, r: [0, 5],  d: 0 }, // pool[0] BQ1   slice "> AAA"  (blockquote outer)
    { t: 0, r: [2, 5],  d: 0 }, // pool[1] ParaA slice "AAA"    (leaf paragraph)
    { t: 0, r: [7, 12], d: 0 }, // pool[2] BQ2   slice "> BBB"  (blockquote outer)
    { t: 0, r: [9, 12], d: 0 }, // pool[3] ParaB slice "BBB"    (leaf paragraph)
];

function makeAst1c(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0], meta: {},
        blocks: [
            { t: 'BlockQuote', c: [{ t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 }], s: 0 },
            { t: 'BlockQuote', c: [{ t: 'Para', c: [{ t: 'Str', c: 'BBB' }], s: 3 }], s: 2 },
        ],
        astContext: { p: POOL_1C },
    });
}

/** Mount PreviewRoot with the two-blockquote fixture in UNLOCK mode. */
function mountFixture1c(
    opts: { setAst?: (ast: PandocAST) => void } = {},
) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAst1c();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT_1C,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockNestingCursor: true,
        onNavigateToDocument: () => {},
    };
    return { ...render(<PreviewRoot {...props} />), setAst };
}

/** Mock getBoundingClientRect on all [data-block-pool-id] tiles. */
function mockTileRects1c(container: HTMLElement) {
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
 * §1 Test 1.c(i): UNLOCK ArrowDown from ParaA (leaf, line 0) → ParaB (leaf, line 2)
 *
 * destLine = L0(0) + draftLineCount(1) = 1.
 * surfaceAtLine(set, 1) = null (blank gap line) → advance to 2.
 * surfaceAtLine(set, 2) = ParaB [9,12] (leaf, depth 1).
 * DOM element: pool[3] data-block-pool-id="3" → value = 'BBB'. ✓
 *
 * FAIL-ON-REVERT: switching the UNLOCK resolver to use enumerateOuterBlocks
 * (LOCKED path) opens BQ2 (value='> BBB'), not ParaA (value='BBB') → RED.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('§1 Test 1.c(i) — UNLOCK ArrowDown crosses blank gap line, lands on leaf', () => {
    it('ArrowDown from ParaA (leaf on line 0) lands on ParaB (leaf on line 2), skipping blank gap', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountFixture1c({ setAst });
        await act(async () => {});
        mockTileRects1c(container);

        // Activate ParaA (pool-id=1, leaf "AAA").
        const tileParaA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileParaA, 'pool-id=1 (ParaA) must be in DOM').not.toBeNull();
        await act(async () => {
            fireEvent(tileParaA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileParaA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta, 'editor should open on ParaA').not.toBeNull();
        expect(ta!.value).toBe('AAA');

        // ArrowDown → UNLOCK resolver: skip gap line 1, land on ParaB (line 2).
        await act(async () => {
            fireEvent.keyDown(ta!, { key: 'ArrowDown' });
        });

        expect(setAst).not.toHaveBeenCalled(); // clean move, no commit
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter, 'editor should now be on ParaB').not.toBeNull();
        // UNLOCK: leaf ParaB → 'BBB'; LOCKED would give BQ2 → '> BBB'.
        expect(taAfter!.value).toBe('BBB');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §1 Test 1.c(ii): UNLOCK ArrowUp from ParaB (line 2) → ParaA (line 0)
 *
 * destLine = L0(2). Up-path starts at L = destLine - 1 = 1.
 * surfaceAtLine(set, 1) = null (blank gap) → advance to 0.
 * surfaceAtLine(set, 0) = ParaA [2,5] (leaf, depth 1).
 * DOM element: pool[1] data-block-pool-id="1" → value = 'AAA'. ✓
 *
 * FAIL-ON-REVERT: if the UP resolver starts at destLine (= 2) instead of
 * destLine-1, surfaceAtLine(set, 2) = ParaB → same block → no-op → RED.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('§1 Test 1.c(ii) — UNLOCK ArrowUp crosses blank gap line, lands on leaf', () => {
    it('ArrowUp from ParaB (leaf on line 2) lands on ParaA (leaf on line 0), skipping blank gap', async () => {
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountFixture1c({ setAst });
        await act(async () => {});
        mockTileRects1c(container);

        // Activate ParaB (pool-id=3, leaf "BBB").
        const tileParaB = container.querySelector<HTMLElement>('[data-block-pool-id="3"]');
        expect(tileParaB, 'pool-id=3 (ParaB) must be in DOM').not.toBeNull();
        await act(async () => {
            fireEvent(tileParaB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileParaB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta, 'editor should open on ParaB').not.toBeNull();
        expect(ta!.value).toBe('BBB');

        // ArrowUp → UNLOCK resolver: start at destLine-1=1, skip gap, land on ParaA (line 0).
        await act(async () => {
            fireEvent.keyDown(ta!, { key: 'ArrowUp' });
        });

        expect(setAst).not.toHaveBeenCalled();
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter, 'editor should now be on ParaA').not.toBeNull();
        expect(taAfter!.value).toBe('AAA');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §1 Test 1.c(iii): UNLOCK clamp at top (ArrowUp from ParaA → no-op)
 *
 * destLine = L0(0). Up-path starts at L = -1 → below docStartLine(0) → clamp.
 * Returns null → no-op: editor stays on ParaA.
 *
 * FAIL-ON-REVERT: if wrap is still present, would open ParaB (last leaf) → RED.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('§1 Test 1.c(iii) — UNLOCK clamp at top (ArrowUp from first leaf is a no-op)', () => {
    it('ArrowUp from ParaA (first leaf, line 0) is a no-op — clamp, no wrap', async () => {
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountFixture1c({ setAst });
        await act(async () => {});
        mockTileRects1c(container);

        // Activate ParaA (pool-id=1, leaf "AAA") — first leaf.
        const tileParaA = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tileParaA).not.toBeNull();
        await act(async () => {
            fireEvent(tileParaA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileParaA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta).not.toBeNull();
        expect(ta!.value).toBe('AAA');

        // ArrowUp from first leaf → clamp: no-op.
        await act(async () => {
            fireEvent.keyDown(ta!, { key: 'ArrowUp' });
        });

        expect(setAst).not.toHaveBeenCalled();
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter).not.toBeNull();
        expect(taAfter!.value).toBe('AAA'); // still on ParaA
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §1 Test 1.c(iv): UNLOCK clamp at bottom (ArrowDown from ParaB → no-op)
 *
 * destLine = L0(2) + draftLineCount(1) = 3. No surface covers line 3+.
 * → clamp: no-op.
 *
 * FAIL-ON-REVERT: if wrap is still present, would open ParaA (first leaf) → RED.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('§1 Test 1.c(iv) — UNLOCK clamp at bottom (ArrowDown from last leaf is a no-op)', () => {
    it('ArrowDown from ParaB (last leaf, line 2) is a no-op — clamp, no wrap', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const setAst = vi.fn();
        const { container } = mountFixture1c({ setAst });
        await act(async () => {});
        mockTileRects1c(container);

        // Activate ParaB (pool-id=3, leaf "BBB") — last leaf.
        const tileParaB = container.querySelector<HTMLElement>('[data-block-pool-id="3"]');
        expect(tileParaB).not.toBeNull();
        await act(async () => {
            fireEvent(tileParaB!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileParaB!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta).not.toBeNull();
        expect(ta!.value).toBe('BBB');

        // ArrowDown from last leaf → clamp: no-op.
        await act(async () => {
            fireEvent.keyDown(ta!, { key: 'ArrowDown' });
        });

        expect(setAst).not.toHaveBeenCalled();
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter).not.toBeNull();
        expect(taAfter!.value).toBe('BBB'); // still on ParaB
    });
});
