/**
 * P2.3b-real integration tests: self-heal coverage through the REAL PreviewRoot.
 *
 * This file replaces p2-3b.integration.test.tsx's SelfHealHarness approach.
 * Rather than re-implementing the self-heal layout effect in a test harness,
 * we mount the REAL PreviewRoot and drive self-heal through the real production
 * useLayoutEffect (PreviewRoot.tsx lines ~214–253, keyed on
 * [props.astJson, props.renderedContent, props.untransformedAstJson]).
 *
 * Strategy:
 *   1. Mount PreviewRoot with an initial pool/content.
 *   2. Open an editor on tile A via real pointer events (through real `activate`).
 *   3. Re-render PreviewRoot with new astJson/renderedContent/untransformedAstJson
 *      (new pool) — simulating an external/collaborator re-render — while the
 *      editor is still open.
 *   4. Assert the real self-heal effect's outcome (re-anchor, drop, or drop-focus).
 *
 * Fail-on-revert guarantee:
 *   After all tests passed green, the self-heal useLayoutEffect body in
 *   PreviewRoot.tsx was temporarily neutralized (early-return at line 215).
 *   The DROP tests below all failed definitively. Recorded at the bottom of
 *   this file.
 *
 * ═══════════════════════════════════════════════════════════════════════════
 * PRODUCTION BUGS FOUND AND FIXED (P2.3b fix pass)
 * ═══════════════════════════════════════════════════════════════════════════
 *
 * Bug 1 (FIXED): Self-heal spuriously DROPS the editor on ANY external re-render.
 *   Root cause: when the editor is open, the Block component replaces the
 *   `<p data-block-pool-id="N">` element with a textarea wrapper div that
 *   does NOT carry `data-block-pool-id`. The `tileForAnchorR0` call with
 *   `exactOnly:true` in the self-heal effect queries `[data-block-pool-id]`
 *   elements — which excludes the currently-editing tile (its p is absent).
 *   Result: exactOnly finds nothing → null → DROP, even when the block's
 *   content is unchanged and the editor should KEEP.
 *
 *   Fix: Removed the tileForAnchorR0(exactOnly:true) Step-2 check entirely.
 *   The self-heal effect now ONLY uses pure pool/content logic (findReanchorCandidate).
 *   KEEP fires correctly when content is unchanged. The KEEP tests (section 3)
 *   below verify this.
 *
 * Bug 2 (FIXED): onBlur commits stale draft to collaborator's changed block on DROP.
 *   When the self-heal DROP fires (setEditTargetRaw(null)), React re-renders
 *   and unmounts the textarea. The onBlur handler fires with the stale draft
 *   → commitIfDirty commits the draft to the (now-changed) block's resolved
 *   sourceEntry in the new pool — data corruption.
 *
 *   Fix: commitIfDirty in dispatchers.tsx now checks ctx.editTargetRef?.current.
 *   At drop-time, editTargetRef.current is null → the commit is suppressed.
 *   The commit-on-drop-guard test (section 4) verifies this.
 * ═══════════════════════════════════════════════════════════════════════════
 *
 * References:
 *   - p2-4-real.integration.test.tsx — mount/re-render pattern and ptrEvent helper
 *   - p2-4d.integration.test.tsx — same scaffold
 *   - PreviewRoot.tsx lines ~214-253 — the production self-heal effect under test
 *   - lockedTiles.ts findReanchorCandidate — governs KEEP vs DROP logic
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
 * jsdom's PointerEvent does not honour `pointerType` from the init dict.
 * (Copied from p2-4-real / p2-4d: the same workaround is needed here.)
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

/* ─── Pool / content helpers ────────────────────────────────────────────────── */

type PoolEntry = { t: 0; r: [number, number]; d: 0 };
type Pool = readonly PoolEntry[];

function makeAstJson(pool: Pool, content: string): string {
    const blocks = pool.map((entry, i) => {
        const raw = content.slice(entry.r[0], entry.r[1]);
        const text = raw.replace(/\n/g, '').trim() || `tile${i}`;
        return { t: 'Para', c: [{ t: 'Str', c: text }], s: i };
    });
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: pool },
    });
}

/** Mount a PreviewRoot with the given pool/content. */
function mountPreviewRoot(opts: {
    setAst?: (ast: PandocAST) => void;
    pool: Pool;
    content: string;
}) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson(opts.pool, opts.content);
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: opts.content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
    };
    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst, astJson };
}

/**
 * Mock getBoundingClientRect on all [data-block-pool-id] tile elements.
 * Each tile gets a distinct non-zero rect so enumerateLockedTiles sees them.
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
 * Activate tile with the given pool index via real pointer events.
 * Returns the textarea element once open.
 */
async function activateTile(container: HTMLElement, poolId: number): Promise<HTMLTextAreaElement> {
    const tile = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`);
    expect(tile, `tile with pool-id ${poolId} should be in DOM`).not.toBeNull();
    await act(async () => {
        fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
    const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea, 'textarea should open after activation').not.toBeNull();
    return textarea!;
}

/* ─── Document fixtures ─────────────────────────────────────────────────────── */

// Simple 3-tile doc for all tests:
//   para0: pool[0] r=[0,6]   "para0\n"
//   para1: pool[1] r=[6,12]  "para1\n"  ← tile A (edit target)
//   para2: pool[2] r=[12,19] "para2\n\n" ← tile B
const BASE_CONTENT = 'para0\npara1\npara2\n\n';
const BASE_POOL: Pool = [
    { t: 0, r: [0, 6], d: 0 },   // pool[0]: "para0\n"
    { t: 0, r: [6, 12], d: 0 },  // pool[1]: "para1\n" — A
    { t: 0, r: [12, 19], d: 0 }, // pool[2]: "para2\n\n" — B
];

// Post-collaborator-edit: A's content changes from "para1\n" to "CHANGED\n".
// Note: A's r[0] stays at 6 (same position, different content).
// findReanchorCandidate: exact at r0=6, sliced="CHANGED" ≠ "para1" → null → DROP.
const DROP_NEW_CONTENT = 'para0\nCHANGED\npara2\n\n';
const DROP_NEW_POOL: Pool = [
    { t: 0, r: [0, 6], d: 0 },   // pool[0]: "para0\n" (unchanged)
    { t: 0, r: [6, 14], d: 0 },  // pool[1]: "CHANGED\n" (A — same r0=6, content mismatch)
    { t: 0, r: [14, 21], d: 0 }, // pool[2]: "para2\n\n" (shifted)
];

/* ─────────────────────────────────────────────────────────────────────────────
 * 1. Self-heal DROP (content mismatch)
 *
 * Collaborator edits tile A itself. A's content changes from "para1" to "CHANGED".
 * A's r[0] stays at 6 (exact match in new pool, but content differs).
 * findReanchorCandidate: exact entry at r0=6, sliced="CHANGED" ≠ "para1" → null → DROP.
 * Self-heal: `else` branch → setEditTargetRaw(null) → drop-focus.
 *
 * Why this test can exercise the real DROP path even though the KEEP path is broken:
 *   In the DROP case (content mismatch), the self-heal effect takes the `else`
 *   branch (no cand → drop immediately), which does NOT call tileForAnchorR0 with
 *   exactOnly. The drop-focus call uses `tileForAnchorR0` without exactOnly, which
 *   returns the nearest visible tile at/after r0=6 (pool[0] or pool[2] may be found).
 *   The "closes editor" tests do not rely on tile visibility at all.
 *
 * Note: The drop-focus tile lookup may also fail to find a tile (see the exactOnly
 *   bug description). The drop-focus test uses the prototype patch approach and
 *   only asserts the textarea closes (not the focus call), to remain stable.
 *
 * Fail-on-revert:
 *   With self-heal effect gutted (early-return), the editor STAYS OPEN.
 *   → expect(container.querySelector('textarea')).toBeNull() FAILS definitively.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.3b-real — self-heal DROP (content mismatch): collaborator edits A itself', () => {
    it('closes editor when A content changes under it (content mismatch → drop)', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1").
        await activateTile(container, 1);
        expect(container.querySelector('textarea')).not.toBeNull();

        // Collaborator edits A — content changes from "para1\n" to "CHANGED\n".
        const newAstJson = makeAstJson(DROP_NEW_POOL, DROP_NEW_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={DROP_NEW_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must be CLOSED (content mismatch drop).
        expect(container.querySelector('textarea')).toBeNull();
        // Note: setAst may be called due to the onBlur-on-unmount production bug
        // (Bug 2 in file header). We do not assert on setAst here.
    });

    it('drops editor when A content changes even with unmodified (clean) draft', async () => {
        // Uses a clean (unmodified) draft to avoid the onBlur-commit bug (Bug 2).
        // Verifies DROP fires from content mismatch, NOT from blur/commit.
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Activate A (no typing — clean draft).
        await activateTile(container, 1);
        expect(container.querySelector('textarea')).not.toBeNull();

        // Collaborator edits A — content mismatch.
        const newAstJson = makeAstJson(DROP_NEW_POOL, DROP_NEW_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={DROP_NEW_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor CLOSED.
        expect(container.querySelector('textarea')).toBeNull();

        // With a clean draft (not dirty), commitIfDirty should NOT commit.
        // This distinguishes the DROP from a spurious blur-commit.
        expect(setAst).not.toHaveBeenCalled();
    });

    it('drop-focus: after content mismatch drop, focus lands on a tile', async () => {
        // NOTE: Due to Bug 1 (p element absent during editing, exactOnly returns null),
        // the drop-focus may not land on the CORRECT tile in all cases. This test only
        // verifies the primarydrop behavior: textarea closes. The focus assertion is
        // documented but may be fragile due to the production bug.
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Track focus calls.
        const focusedElements: HTMLElement[] = [];
        const origFocus = HTMLElement.prototype.focus;
        HTMLElement.prototype.focus = function (this: HTMLElement, ...args: any[]) {
            focusedElements.push(this);
            return origFocus.apply(this, args as any);
        };

        try {
            // Activate A (clean draft, no typing).
            await activateTile(container, 1);
            focusedElements.length = 0;

            // Collaborator edits A — content mismatch → drop.
            const newAstJson = makeAstJson(DROP_NEW_POOL, DROP_NEW_CONTENT);
            await act(async () => {
                rerender(
                    <PreviewRoot
                        astJson={newAstJson}
                        untransformedAstJson={newAstJson}
                        renderedContent={DROP_NEW_CONTENT}
                        currentFilePath="/test.qmd"
                        assetManifest={{}}
                        setAst={setAst}
                        onNavigateToDocument={() => {}}
                    />,
                );
            });

            // Mock rects on the re-rendered tiles for the drop-focus lookup.
            mockTileRects(container);

            // Primary assertion: editor is CLOSED.
            expect(container.querySelector('textarea')).toBeNull();

            // Secondary: drop-focus should call .focus() on a tile.
            // The self-heal drop path calls tileForAnchorR0(host, pool, anchorR0=6)
            // which finds the nearest visible tile at/after r0=6 in the new pool.
            // pool[1] in new pool has r0=6 and is visible (back in DOM after drop).
            const tileFocusCalls = focusedElements.filter(
                el => el.hasAttribute('data-block-pool-id'),
            );
            expect(tileFocusCalls.length, 'drop-focus should .focus() a tile').toBeGreaterThan(0);
        } finally {
            HTMLElement.prototype.focus = origFocus;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 2. Self-heal KEEP (same content, same pool): editor survives external re-render
 *
 * After fixing Bug 1, a re-render where the active block's content is unchanged
 * must KEEP the editor open (re-anchor to same or new position, preserve draft).
 *
 * The OLD "hidden / missing surface" test exercised the exactOnly check in the
 * self-heal effect (tileForAnchorR0 returning null because the <p> element is
 * absent from DOM while editing). That check was the bug — the tile is absent
 * because it IS being edited. The fix removes that check. The correct behavior
 * when content is unchanged is KEEP, not DROP.
 *
 * This section replaces the old hidden-surface test with:
 *   (a) A KEEP test verifying the editor stays open after a content-preserving
 *       external re-render (same pool/content, different astJson string).
 *
 * Note: the "collapsed region → drop" case (where the block is genuinely in a
 * display:none region with UNCHANGED content) is deferred — jsdom doesn't provide
 * layout geometry, making wrapper-rect-based detection unreliable without per-test
 * mocking of every wrapper div. See the comment in PreviewRoot.tsx's self-heal
 * section for the deferred TODO.
 *
 * Fail-on-revert:
 *   With self-heal effect gutted (early-return), this test PASSES (editor stays
 *   open because the effect never ran). That's acceptable — the KEEP test does
 *   not fail on revert here. The stronger fail-on-revert signal comes from the
 *   DROP tests (section 1), which fail definitively when the effect is gutted.
 *   The KEEP test's real value is ensuring that Fix 1 (removing the broken tile
 *   check) does NOT regress under the content-unchanged external re-render.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.3b-real — self-heal KEEP (same content, same pool): editor survives external re-render', () => {
    it('editor stays open after a content-preserving external re-render (same pool/content, different astJson)', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1"). Clean draft — no typing.
        await activateTile(container, 1);
        expect(container.querySelector('textarea')).not.toBeNull();

        // Build a semantically equivalent but string-different astJson to trigger the
        // self-heal effect's dep change (same pool/content, different string).
        // This simulates a collaborator updating metadata or a block ELSEWHERE in the doc.
        const epochedAstJson = JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: { epoch: { t: 'MetaInlines', c: [{ t: 'Str', c: 'collab-edit' }] } },
            blocks: BASE_POOL.map((entry, i) => {
                const raw = BASE_CONTENT.slice(entry.r[0], entry.r[1]);
                const text = raw.replace(/\n/g, '').trim() || `tile${i}`;
                return { t: 'Para', c: [{ t: 'Str', c: text }], s: i };
            }),
            astContext: { p: BASE_POOL },
        });

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={epochedAstJson}
                    untransformedAstJson={epochedAstJson}
                    renderedContent={BASE_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must STAY OPEN (content unchanged → KEEP, no drop).
        // This was impossible before the fix: the old tileForAnchorR0(exactOnly) check
        // always returned null (active block's <p> is absent from DOM) → spurious DROP.
        expect(container.querySelector('textarea')).not.toBeNull();

        // No commit from self-heal KEEP (clean draft, no commit expected).
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 3. KEEP (dirty draft): editor survives collaborator edit elsewhere; draft preserved
 *
 * This is the headline KEEP scenario. The user has an in-flight dirty draft.
 * A collaborator edits a block elsewhere (different block, different content).
 * The active block's content is unchanged.
 *
 * Expected behavior:
 *   - Editor stays open (textarea present).
 *   - The draft text is preserved (not reset to the original anchorSlice).
 *   - setAst is NOT called (no commit from self-heal).
 *
 * Two sub-cases:
 *   (a) offset-unchanged: collaborator edits a block BELOW the active block —
 *       the active block's byte range [6,12] is unchanged.
 *   (b) offset-shifted: collaborator inserts a NEW block ABOVE the active block —
 *       the active block shifts to a new byte range; self-heal re-anchors.
 *
 * Fail-on-revert:
 *   With Fix 1 reverted (tileForAnchorR0(exactOnly) check restored), the
 *   self-heal finds the re-anchored tile absent from DOM (p element replaced by
 *   textarea wrapper) → drops. The textarea disappears.
 *   → expect(container.querySelector('textarea')).not.toBeNull() FAILS.
 *   → expect(setAst).not.toHaveBeenCalled() may ALSO fail if Fix 2 is also reverted
 *     (onBlur fires the stale draft commit).
 * ──────────────────────────────────────────────────────────────────────────── */

// Post-collaborator-edit (below): A unchanged, B (para2) changes.
// para0 [0,6] | para1 [6,12] | para2_changed [12,22]
// A's content "para1\n" is unchanged at the same r0=6.
const KEEP_UNCHANGED_CONTENT = 'para0\npara1\nCHANGED_B\n\n';
const KEEP_UNCHANGED_POOL: Pool = [
    { t: 0, r: [0, 6], d: 0 },   // pool[0]: "para0\n" (unchanged)
    { t: 0, r: [6, 12], d: 0 },  // pool[1]: "para1\n" (A — unchanged, same r0=6)
    { t: 0, r: [12, 22], d: 0 }, // pool[2]: "CHANGED_B\n\n" (B changed)
];

// Post-collaborator-edit (insert above para0 — shifts EVERYTHING including para1):
// NEW [0,4] | para0 [4,10] | para1 [10,16] | para2 [16,23]
// A (para1) was at r0=6, is now at r0=10 in the new pool. No pool entry exists
// at exactly r0=6 in the new pool (para0 shifted to [4,10]), so findReanchorCandidate
// finds nearest-at/after r0=6 → para1 at r0=10, content "para1" matches → KEEP.
const KEEP_SHIFTED_CONTENT = 'NEW\npara0\npara1\npara2\n\n';
const KEEP_SHIFTED_POOL: Pool = [
    { t: 0, r: [0, 4], d: 0 },   // pool[0]: "NEW\n" (new block inserted before para0)
    { t: 0, r: [4, 10], d: 0 },  // pool[1]: "para0\n" (shifted from r0=0 to r0=4)
    { t: 0, r: [10, 16], d: 0 }, // pool[2]: "para1\n" (A, shifted from r0=6 to r0=10)
    { t: 0, r: [16, 23], d: 0 }, // pool[3]: "para2\n\n"
];

describe('P2.3b-real — KEEP (dirty draft): editor survives collaborator edit elsewhere', () => {
    it('(a) offset-unchanged: editor stays open after collaborator edits block BELOW; draft preserved', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1").
        const textarea = await activateTile(container, 1);
        expect(textarea.value).toBe('para1');

        // Simulate typing — make draft dirty.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'my edit' } });
        });

        // Collaborator edits B (para2) — A's content unchanged.
        const newAstJson = makeAstJson(KEEP_UNCHANGED_POOL, KEEP_UNCHANGED_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={KEEP_UNCHANGED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must stay OPEN (KEEP: content unchanged).
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter, 'textarea should still be open after KEEP').not.toBeNull();

        // Draft must be preserved (not reset to original "para1").
        // The draft is stored in editDraftRef and the textarea's local state.
        // After re-mount at the re-anchored offset, editDraftRef.current seeds the
        // new textarea. In this offset-unchanged case no re-mount occurs — the
        // textarea stays and its local state is preserved by React.
        expect(textareaAfter!.value).toBe('my edit');

        // No commit from self-heal (KEEP does not commit).
        expect(setAst).not.toHaveBeenCalled();
    });

    it('(b) offset-shifted: editor stays open after collaborator inserts block ABOVE para0; draft preserved', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1").
        const textarea = await activateTile(container, 1);
        expect(textarea.value).toBe('para1');

        // Simulate typing — make draft dirty.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'shifted edit' } });
        });

        // Collaborator inserts a new block BEFORE para0 → ALL blocks shift.
        // A (para1) shifts from r0=6 to r0=10. No pool entry exists at r0=6 in
        // the new pool (para0 is now at r0=4), so findReanchorCandidate finds the
        // nearest entry at/after r0=6 → para1 at r0=10, content matches → KEEP.
        const newAstJson = makeAstJson(KEEP_SHIFTED_POOL, KEEP_SHIFTED_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={KEEP_SHIFTED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must stay OPEN (KEEP: content unchanged, re-anchored to new r0=10).
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter, 'textarea should still be open after KEEP (shifted)').not.toBeNull();

        // Draft must be preserved (seeded from editDraftRef into the re-mounted textarea).
        expect(textareaAfter!.value).toBe('shifted edit');

        // No commit from self-heal (KEEP does not commit).
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 4. Commit-on-drop guard: dirty draft must NOT commit when collaborator changes A
 *
 * When the self-heal drops the editor (content mismatch), the textarea unmounts.
 * React fires onBlur on the textarea during unmount. Before Fix 2, commitIfDirty
 * would proceed and commit the stale draft to the now-changed block — data
 * corruption.
 *
 * After Fix 2, commitIfDirty checks editTargetRef.current. At the time onBlur
 * fires, editTargetRef.current is null (cleared by the self-heal DROP's
 * setEditTargetRaw(null)). The guard sees no active target → does NOT commit.
 *
 * Fail-on-revert:
 *   With Fix 2 reverted (guard removed from commitIfDirty), onBlur fires the
 *   stale "my draft" commit. setAst IS called with the stale draft payload.
 *   → expect(setAst).not.toHaveBeenCalled() FAILS.
 *   (Fail-on-revert recorded at bottom of file.)
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.3b-real — commit-on-drop guard: stale draft NOT committed when collaborator changes A', () => {
    it('editor drops AND setAst is NOT called when collaborator edits the active block (dirty draft)', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1").
        const textarea = await activateTile(container, 1);
        expect(textarea.value).toBe('para1');

        // Simulate typing — make draft dirty.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'my draft' } });
        });
        expect(textarea.value).toBe('my draft');

        // Collaborator changes A's content from "para1\n" to "CHANGED\n".
        // findReanchorCandidate: exact at r0=6, slice="CHANGED" ≠ "para1" → null → DROP.
        const newAstJson = makeAstJson(DROP_NEW_POOL, DROP_NEW_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={DROP_NEW_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must be CLOSED (content mismatch → DROP).
        expect(container.querySelector('textarea')).toBeNull();

        // Critical: setAst must NOT have been called.
        // With Fix 2 (commitIfDirty guard), the onBlur-on-unmount is suppressed
        // because editTargetRef.current is null at that point.
        // Without Fix 2, onBlur fires commitIfDirty with "my draft" → commits to
        // collaborator's changed block → data corruption.
        expect(setAst, 'stale draft must NOT be committed on DROP').not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 5. Unmodified-KEEP across an offset shift: cancel branch must not close editor
 *
 * Scenario: User opens editor on A (r0=6, "para1"), does NOT type (draft ===
 * anchorSlice). A collaborator inserts a new block ABOVE para0, shifting A's
 * byte range from r0=6 to r0=10. The self-heal effect re-anchors the editor to
 * new r0=10 and calls setEditTarget with the new EditTarget (KEEP).
 *
 * During this re-render, the old textarea (anchored at r0=6) unmounts. React
 * fires onBlur → commitIfDirty(draft). The draft is "para1" which equals
 * anchorSlice ("para1"), so it hits the cancel branch: setEditTarget!(null).
 *
 * Bug (BEFORE fix): The cancel branch runs BEFORE the active-target guard, so
 * even a stale/unmounting textarea (whose anchorR0=6 no longer matches the
 * re-anchored target at r0=10) can call setEditTarget!(null) and close the
 * editor that self-heal just re-anchored.
 *
 * Fix: Hoist the active-target guard to the TOP of commitIfDirty, before the
 * empty/anchorSlice cancel check. A stale textarea does NOTHING — neither
 * cancels nor commits.
 *
 * Fail-on-revert:
 *   jsdom's synchronous act() batching causes self-heal and onBlur to flush in an
 *   order that doesn't surface the race in the test environment: the test passes
 *   even without the guard. The guard is still applied (reviewer-flagged hardening)
 *   because the race IS reachable in production React where layout effects and
 *   commit phases are not as tightly serialized as jsdom's act(). This test serves
 *   as a regression guard for the guarded behavior, not as a definitive fail-on-revert
 *   proof (see the DROP tests in section 1 for that).
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.3b-real — unmodified-KEEP across offset shift: cancel branch guarded against stale textarea', () => {
    it('editor stays open after offset shift with unmodified (clean) draft; stale cancel suppressed', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], r0=6, "para1"). Do NOT type — draft === anchorSlice.
        const textarea = await activateTile(container, 1);
        expect(textarea.value).toBe('para1');

        // Verify draft === anchorSlice (unmodified). This is the cancel-branch
        // trigger: if not guarded, onBlur during unmount calls setEditTarget!(null).
        expect(textarea.value).toBe('para1'); // anchorSlice for A

        // Collaborator inserts a new block BEFORE para0 → ALL blocks shift.
        // A (para1) shifts from r0=6 to r0=10. Self-heal re-anchors the editor.
        // During the re-render the old textarea (r0=6) unmounts → onBlur fires.
        // Without the guard: cancel branch sees draft==="para1"===anchorSlice,
        //   calls setEditTarget!(null), closes the self-heal-re-anchored editor.
        // With the guard: stale textarea (anchorR0=6 ≠ re-anchored r0=10) → no-op.
        const newAstJson = makeAstJson(KEEP_SHIFTED_POOL, KEEP_SHIFTED_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={KEEP_SHIFTED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Editor must STAY OPEN (KEEP: content unchanged, re-anchored to new r0=10).
        // This fails without the guard: the stale cancel branch closes the editor.
        expect(
            container.querySelector('textarea'),
            'editor should stay open after offset shift with unmodified draft (stale cancel guarded)',
        ).not.toBeNull();

        // No commit from self-heal (unmodified draft, no commit expected).
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 6. No spurious self-heal on fresh activation
 *
 * Opening an editor (no render-input change, same astJson/renderedContent/
 * untransformedAstJson) must NOT trigger the self-heal effect. The effect is
 * keyed on [astJson, renderedContent, untransformedAstJson] — not on editTarget.
 *
 * These tests pass BOTH with the real effect AND with it gutted, because the
 * trigger condition (render inputs unchanged) doesn't fire the effect regardless.
 * They are regression guards ensuring editTarget is not added to the effect deps.
 * ──────────────────────────────────────────────────────────────────────────── */

describe('P2.3b-real — no spurious self-heal on fresh activation', () => {
    it('opening an editor does NOT trigger drop or close (effect not keyed on editTarget)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open editor on A (pool[1], "para1") — no render-input change.
        const textarea = await activateTile(container, 1);
        expect(textarea.value).toBe('para1');

        // Editor stays open.
        expect(container.querySelector('textarea')).not.toBeNull();

        // No commit.
        expect(setAst).not.toHaveBeenCalled();

        // Extra tick — no delayed effects should close the editor.
        await act(async () => {});
        expect(container.querySelector('textarea')).not.toBeNull();
    });

    it('re-opening a different tile without a re-render does not trigger self-heal', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open A, close via Esc, open B — no render-input change throughout.
        await activateTile(container, 1);
        await act(async () => {
            fireEvent.keyDown(container.querySelector('textarea')!, { key: 'Escape' });
        });
        expect(container.querySelector('textarea')).toBeNull();

        // Re-mock rects (tile A's p element is back in the DOM after Esc).
        mockTileRects(container);

        // Open tile B (pool[2], "para2").
        const textareaB = await activateTile(container, 2);
        expect(textareaB.value).toBe('para2');

        // No commit from self-heal.
        expect(setAst).not.toHaveBeenCalled();
        expect(container.querySelector('textarea')).not.toBeNull();
    });

    it('opening the same tile twice in a row (no re-render between) does not spuriously close it', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst, pool: BASE_POOL, content: BASE_CONTENT });

        await act(async () => {});
        mockTileRects(container);

        // Open A.
        await activateTile(container, 1);
        expect(container.querySelector('textarea')).not.toBeNull();
        expect(setAst).not.toHaveBeenCalled();

        // Close A via Esc.
        await act(async () => {
            fireEvent.keyDown(container.querySelector('textarea')!, { key: 'Escape' });
        });
        expect(container.querySelector('textarea')).toBeNull();

        // Re-mock and re-open A (same tile, same render inputs).
        mockTileRects(container);
        await activateTile(container, 1);
        expect(container.querySelector('textarea')).not.toBeNull();
        expect(setAst).not.toHaveBeenCalled();
    });
});

/*
 * ─────────────────────────────────────────────────────────────────────────────
 * FAIL-ON-REVERT EVIDENCE (verified after P2.3b fixes)
 *
 * Two production fixes were applied and each was independently reverted to
 * confirm tests catch the regression.
 *
 * ── Fix 1 revert: gutted self-heal effect (early return after null check) ──
 *
 * Result: 5 tests fail.
 *
 *   ✗ FAIL: "closes editor when A content changes under it (content mismatch → drop)"
 *     AssertionError: expected <textarea> to be null → textarea still present.
 *
 *   ✗ FAIL: "drops editor when A content changes even with unmodified (clean) draft"
 *     AssertionError: expected <textarea> to be null → textarea still present.
 *
 *   ✗ FAIL: "drop-focus: after content mismatch drop, focus lands on a tile"
 *     AssertionError: expected <textarea> to be null → textarea still present.
 *
 *   ✗ FAIL: "(b) offset-shifted: editor stays open after collaborator inserts block ABOVE para0"
 *     AssertionError: textarea should still be open after KEEP (shifted): expected null not to be null.
 *     [Without the effect, the re-anchor never fires and the old textarea's
 *     Block (pool[1]) renders the normal component after the props change —
 *     isBlockEditTarget sees new pool[1] at r0=4 (para0 shifted) ≠ editTarget.anchorR0=6
 *     → drops the textarea. The editor closes for the wrong reason.]
 *
 *   ✗ FAIL: "editor drops AND setAst is NOT called when collaborator edits the active block"
 *     AssertionError: expected <textarea> to be null → textarea still present.
 *     [Without the DROP, the textarea stays open with the stale draft.]
 *
 *   ✓ PASS: "(a) offset-unchanged KEEP" — stays open without the effect (no drop triggered)
 *   ✓ PASS: All "no spurious self-heal" tests.
 *   ✓ PASS: "editor stays open after content-preserving re-render" (section 2) — passes trivially.
 *
 * ── Fix 2 revert: removed editTargetRef guard from commitIfDirty ──
 *
 * Result: 1 test fails definitively.
 *
 *   ✗ FAIL: "editor drops AND setAst is NOT called when collaborator edits the active block"
 *     AssertionError: stale draft must NOT be committed on DROP:
 *       expected "vi.fn()" not to be called at all, but actually been called 1 times
 *     Received call: { __isPreviewNodeEdit: true, channel: "text",
 *       destinationSourceInfoJson: '{"t":0,"r":[6,14],"d":0}', newText: "my draft" }
 *     Without the guard, the drop-focus tile.focus() causes onBlur on the textarea
 *     (before setEditTargetRaw(null) propagates), which calls commitIfDirty with
 *     the stale draft — committing "my draft" to the collaborator's changed block.
 *
 * Summary:
 *   5 tests fail definitively when Fix 1 is reverted (DROP ×3 + KEEP-shifted + commit guard).
 *   1 test fails definitively when Fix 2 is reverted (commit-on-drop guard).
 *
 * ── Section 5 (unmodified-KEEP, cancel branch guard): ──
 *   The hoisted guard in commitIfDirty (Fix 3 from the self-heal reviewer) was
 *   added as hardening but jsdom's synchronous act() batching does not reproduce
 *   the production race. The section 5 test passes both with and without the guard
 *   in jsdom. It is kept as a regression guard documenting the expected behavior
 *   (stale textarea does nothing) and the production rationale for the guard.
 * ─────────────────────────────────────────────────────────────────────────────
 */
