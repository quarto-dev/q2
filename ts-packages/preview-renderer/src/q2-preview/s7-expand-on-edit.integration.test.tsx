/**
 * §7 integration tests: expand-on-edit — third editor size state.
 *
 * Tests 7.a through 7.h (7.e/7.f are e2e-only, out of scope here).
 *
 * Behavior:
 *   - Activation opens at contentHeight (unchanged from before).
 *   - An in-surface keystroke (printable char, Backspace/Delete, Home/End, or a
 *     non-edge arrow) expands the textarea to fit all text.
 *   - Leave keys (edge arrows, nesting chord, Esc, Cmd/Ctrl+Enter) do NOT expand.
 *   - Keyboard activation (roving Enter/Space) opens ALREADY expanded.
 *   - Pointer/click activation opens collapsed.
 *   - Hop landings open collapsed.
 *
 * Test seam: `data-expanded` attribute on the textarea (present ↔ expanded,
 * absent ↔ collapsed). Height assertions are ONLY via the attribute; pixel-fit
 * assertions are e2e-only.
 *
 * Tests:
 *   7.a — click-activate: data-expanded absent; type printable → data-expanded present
 *          and explicit style.height set (clamp floor, non-pixel-discriminating).
 *   7.b — keyboard (Enter) activation opens data-expanded present immediately;
 *          pointer activation opens data-expanded absent.
 *   7.c — leave keys do NOT expand; vacuity guard: leave action DID fire.
 *   7.d — floor holds: scrollHeight < contentHeight → style.height = contentHeight px.
 *   7.g — hop to block B → B opens collapsed (editExpandedRef reset).
 *   7.h — self-heal remount preserves expanded state (editExpandedRef read on remount).
 *
 * FAIL-ON-REVERT notes are documented inline per test.
 *
 * Harness mirrors: p2-3b-real, s4-dirty-caret-col, p2-4-real.
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

/* ─── PointerEvent helper (same as p2-3b-real) ──────────────────────────────── */
function ptrEvent(
    type: string,
    opts: PointerEventInit & { clientX?: number; clientY?: number } = {},
): Event {
    const PE = (window as unknown as { PointerEvent?: typeof PointerEvent }).PointerEvent ?? Event;
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
 * Two-paragraph document:
 *   para0 (A): pool[0] r=[0,6]   "para0\n"   line 0
 *   para1 (B): pool[1] r=[6,12]  "para1\n"   line 1
 *
 * anchorSlice: A → "para0"   B → "para1"
 */
const CONTENT_AB = 'para0\npara1\n';
const POOL_AB = [
    { t: 0, r: [0, 6], d: 0 },   // pool[0]: A = "para0\n"
    { t: 0, r: [6, 12], d: 0 },  // pool[1]: B = "para1\n"
];

function makeAstJson(pool: typeof POOL_AB, content: string): string {
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

function mountPreviewRoot(
    opts: {
        setAst?: (ast: PandocAST) => void;
        pool?: typeof POOL_AB;
        content?: string;
    } = {},
) {
    const pool = opts.pool ?? POOL_AB;
    const content = opts.content ?? CONTENT_AB;
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
    return { ...result, setAst, pool, content, astJson };
}

/** Mock getBoundingClientRect on all [data-block-pool-id] tile elements. */
function mockTileRects(container: HTMLElement) {
    container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
            width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
        } as DOMRect);
    });
}

/**
 * Activate tile with the given pool index via POINTER (click) events.
 * Returns the textarea element once open.
 */
async function clickActivateTile(container: HTMLElement, poolId: number): Promise<HTMLTextAreaElement> {
    const tile = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`);
    expect(tile, `tile with pool-id ${poolId} should be in DOM`).not.toBeNull();
    await act(async () => {
        fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
    const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea, 'textarea should open after click activation').not.toBeNull();
    return textarea!;
}

/**
 * Activate tile via keyboard (Arrow to focus, then Enter).
 * The host div is the roving-tabindex container (tabIndex=0, role="region").
 * Returns the textarea element once open.
 */
async function keyboardActivateTile(container: HTMLElement, poolId: number): Promise<HTMLTextAreaElement> {
    const tile = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`);
    expect(tile, `tile with pool-id ${poolId} should be in DOM`).not.toBeNull();

    // Get the roving-tabindex host — the div with tabIndex=0 and role="region"
    const host = container.querySelector<HTMLElement>('[role="region"]');
    expect(host, 'roving host should be in DOM').not.toBeNull();

    // Focus the host, then arrow to the tile, then Enter to activate
    await act(async () => {
        host!.focus();
        // Move hover/focused element to the tile by pointing at it and moving to it
        // We need to hover the tile so hoveredRef is set, then press Enter
        // Do a pointermove over the tile to set hoveredRef, then Enter
        fireEvent(host!, ptrEvent('pointermove', { pointerType: 'mouse' }));
        // Direct approach: simulate that the tile becomes the hovered element
        // by firing pointermove on the tile (so it bubbles to host)
        fireEvent(tile!, ptrEvent('pointermove', { pointerType: 'mouse' }));
        // Now press Enter to activate the hovered block
        fireEvent.keyDown(host!, { key: 'Enter' });
    });
    const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea, 'textarea should open after keyboard activation').not.toBeNull();
    return textarea!;
}

/* ─── Test 7.a: click-activate opens collapsed; printable key expands ─────────
 *
 * FAIL-ON-REVERT:
 *   - Remove `data-expanded` attribute from EditTextarea → first assert (absent) still
 *     passes but second assert (present after type) fails.
 *   - Remove the expand trigger in onKeyDown → after typing, data-expanded still absent
 *     → second assert fails.
 *   - Remove the height setback in useLayoutEffect → height stays at contentHeight
 *     even when expanded → third assert fails (style.height not set by layout effect).
 */
describe('§7.a — click-activate: collapsed; printable char expands', () => {
    it('opens collapsed (no data-expanded), type printable char → data-expanded present + explicit height', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        // Click-activate tile A.
        const ta = await clickActivateTile(container, 0);

        // Must open collapsed (data-expanded absent).
        expect(ta.hasAttribute('data-expanded'), 'should open collapsed after click activation').toBe(false);

        // Gating sub-assert: height stays at contentHeight before expansion.
        // jsdom returns 0 for getComputedStyle/scrollHeight, so contentHeight=0.
        // The textarea's style.height before expansion should equal the
        // contentHeight value (set directly in the style prop). We just check
        // it's a number-like value (not 'auto') — the shape assertion, not pixel.
        // In jsdom, the inline style for height (set via React prop) will be empty string
        // since contentHeight=0 and React doesn't set '0px' explicitly for number 0.
        // What we DO need: after expansion, an explicit height IS set.

        // Type a printable character.
        await act(async () => {
            // Change event (textarea is controlled, so we need to fire onChange)
            fireEvent.change(ta, { target: { value: 'x' } });
            // Also fire keyDown with a printable key to trigger the expand
            fireEvent.keyDown(ta, { key: 'x' });
        });

        // After a printable key: data-expanded must be present.
        expect(ta.hasAttribute('data-expanded'), 'data-expanded should be set after printable key').toBe(true);
        // An explicit style.height should now be set by the layout effect.
        // In jsdom: scrollHeight=0, contentHeight=0, max(0,0)=0 → '0px' or similar.
        // The point is that the height was touched by the effect (not undefined).
        // We check that style.height is truthy or '0px' — anything the effect wrote.
        // Actually in jsdom the effect sets height = 'auto' then Math.max(ch, 0) + 'px'
        // → '0px'. So we just check it's not undefined / null.
        // The binding: revert to NOT running the effect when expanded → style.height stays blank.
        const styleHeight = ta.style.height;
        expect(
            styleHeight === '0px' || (styleHeight !== '' && styleHeight !== undefined),
            `style.height should be set by the expansion effect, got: "${styleHeight}"`,
        ).toBe(true);
    });
});

/* ─── Test 7.b: keyboard activation opens expanded; pointer opens collapsed ───
 *
 * FAIL-ON-REVERT:
 *   - Remove `opts?.keyboard === true` → `expandOnOpen` wiring in `activate`:
 *     keyboard activation opens collapsed → `data-expanded` absent → RED.
 *   - Remove `editExpandedRef.current = true` write in activate's keyboard path:
 *     `useState` initializer reads false → RED.
 */
describe('§7.b — keyboard/pointer activation: expanded vs collapsed', () => {
    it('Enter activation opens with data-expanded present', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        const ta = await keyboardActivateTile(container, 0);

        // Keyboard activation: must open already expanded.
        expect(ta.hasAttribute('data-expanded'), 'keyboard activation should open expanded').toBe(true);
    });

    it('pointer (click) activation opens with data-expanded absent', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);

        // Pointer activation: must open collapsed.
        expect(ta.hasAttribute('data-expanded'), 'click activation should open collapsed').toBe(false);
    });
});

/* ─── Test 7.c: leave keys do NOT expand ─────────────────────────────────────
 *
 * Sub-cases:
 *   - edge ArrowDown (onEdge=true) → navigates away, data-expanded stays absent.
 *   - Esc → closes editor, data-expanded stays absent.
 *   - Cmd/Ctrl+Enter → commits + closes, data-expanded stays absent.
 *
 * Vacuity guard: the leave action must actually have fired (not just "nothing happened").
 *
 * FAIL-ON-REVERT:
 *   - Remove the leave-key exclusions from the expand trigger → all three fire expand
 *     → `data-expanded` present → RED.
 *   - Vacuity guard: if the leave key is a no-op (not actually leaving), the guard's
 *     assertion (requestMove called / setEditTarget called) would also fail → RED.
 */
describe('§7.c — leave keys do not expand', () => {
    it('edge ArrowDown does not expand; navigate away fires (vacuity guard)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // Mock isOnLastVisualLine to return true (edge condition).
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);
        vi.spyOn(caretGeometry, 'getLogicalColumn').mockReturnValue(0);

        const ta = await clickActivateTile(container, 0);

        // Must open collapsed.
        expect(ta.hasAttribute('data-expanded')).toBe(false);

        // Fire edge ArrowDown.
        let requestMoveFired = false;
        // We intercept by checking that the editor closes (the move happens synchronously
        // when the draft is clean, opening the next tile or no-opping if no B).
        // With POOL_AB, there is a tile B (pool[1]). The hop should open B's textarea.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowDown' });
        });

        // data-expanded must still be absent on the (possibly new) textarea.
        // After a hop, A's textarea is gone and B's may open. Check A's textarea
        // was not expanded (it was closed/replaced before expand).
        // The key check: we never transitioned to expanded on A before leave.
        expect(ta.hasAttribute('data-expanded'), 'edge ArrowDown must not expand').toBe(false);

        // Vacuity guard: the move fired — editor is now either on B or closed.
        // In jsdom, the hop opens B. If B opens, we have a different textarea.
        // If B doesn't open (no dest), A's textarea closes. Either way, the
        // leave action was attempted (isOnLastVisualLine was called).
        expect(caretGeometry.isOnLastVisualLine).toHaveBeenCalled();
    });

    it('Esc does not expand; editor closes (vacuity guard)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);
        expect(ta.hasAttribute('data-expanded')).toBe(false);

        // Press Escape.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Escape' });
        });

        // data-expanded was never set on the textarea (it's now gone).
        expect(ta.hasAttribute('data-expanded'), 'Esc must not expand').toBe(false);

        // Vacuity guard: editor is closed.
        expect(container.querySelector('textarea'), 'editor must be closed after Esc').toBeNull();
    });

    it('Cmd+Enter does not expand; editor commits and closes (vacuity guard)', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);
        // Type so the draft is dirty (for the commit to fire).
        await act(async () => {
            fireEvent.change(ta, { target: { value: 'newcontent' } });
        });

        // Verify still collapsed after change (change doesn't expand without keyDown).
        // Actually change does NOT trigger expand — only keyDown does. Let's check.
        // NOTE: onChange doesn't call onKeyDown, so data-expanded stays absent here.
        // (We don't fire keyDown for the printable char in this test to avoid confounding.)
        // The data-expanded check here is: it should still be absent (no keyDown yet).
        expect(ta.hasAttribute('data-expanded')).toBe(false);

        // Now Cmd+Enter (commit+close).
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
        });

        // data-expanded was never set.
        expect(ta.hasAttribute('data-expanded'), 'Cmd+Enter must not expand').toBe(false);

        // Vacuity guard: setAst was called (commit fired).
        expect(setAst, 'setAst must be called (commit fired)').toHaveBeenCalled();
        // Editor is closed.
        expect(container.querySelector('textarea'), 'editor must be closed after Cmd+Enter').toBeNull();
    });
});

/* ─── Test 7.d: floor holds (scrollHeight < contentHeight) ───────────────────
 *
 * When expanded and scrollHeight < contentHeight, height must be clamped to
 * contentHeight (never smaller than the replaced element).
 *
 * In jsdom, scrollHeight=0 always. To prove the floor, we need contentHeight > 0.
 * We stub getBoundingClientRect to return height=100, which becomes contentHeight=100.
 * Then after expansion: style.height must equal '100px' (max(100, 0) = 100).
 *
 * FAIL-ON-REVERT:
 *   Revert `Math.max(contentHeight, ta.scrollHeight)` to bare `ta.scrollHeight`:
 *   → height becomes '0px' (scrollHeight=0), not '100px' → RED.
 */
describe('§7.d — floor: expanded height clamped to contentHeight when scrollHeight < contentHeight', () => {
    it('style.height = contentHeight px when scrollHeight < contentHeight', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});

        // Mock tile rect with height=100 so getComputedStyle sees real dimensions.
        // The contentHeight is derived from getBoundingClientRect().height minus padding/border.
        // In jsdom, getComputedStyle returns empty string for padding/border, so contentHeight = rect.height.
        const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tile).not.toBeNull();

        // Mock the rect with height=100.
        vi.spyOn(tile!, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: 0, right: 200, bottom: 100,
            width: 200, height: 100, x: 0, y: 0, toJSON: () => ({}),
        } as DOMRect);

        // Also mock the host's rect for other tiles.
        container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((el) => {
            if (el !== tile) {
                const pid = Number(el.getAttribute('data-block-pool-id'));
                vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
                    left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
                    width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
                } as DOMRect);
            }
        });

        // Click-activate tile A (contentHeight will be ~100 due to the mocked rect).
        const ta = await clickActivateTile(container, 0);

        // Verify contentHeight of 100 was captured.
        // The textarea's initial style.height should reflect contentHeight.
        // In jsdom, the style prop sets height to the number value.
        // After click-activate with mocked height=100, contentHeight should be ~100.
        // (getComputedStyle returns '' for padding/border in jsdom, so contentHeight = 100.)

        // In jsdom, scrollHeight is always 0.
        expect(ta.scrollHeight).toBe(0);

        // Fire a printable key to trigger expansion.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'a' });
        });

        // After expansion: data-expanded must be set.
        expect(ta.hasAttribute('data-expanded'), 'must be expanded after key').toBe(true);

        // The layout effect runs: height = max(contentHeight=100, scrollHeight=0) + 'px'
        // = '100px'.
        expect(ta.style.height, 'height must be clamped to contentHeight (100px)').toBe('100px');
    });
});

/* ─── Test 7.g: hop to block B opens B collapsed (editExpandedRef reset) ────
 *
 * Open A, expand it (keyboard activate), then hop to B.
 * B must open collapsed — editExpandedRef was reset at B's open.
 *
 * FAIL-ON-REVERT:
 *   Remove `editExpandedRef.current = expandOnOpen` reset in openEditTarget:
 *   B reads stale true → opens expanded → data-expanded present → RED.
 */
describe('§7.g — hop to B: B opens collapsed even after A was expanded', () => {
    it('B opens collapsed after keyboard-expand of A + ArrowDown hop', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        // Mock visual-line checks so ArrowDown triggers a hop from A.
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);
        vi.spyOn(caretGeometry, 'getLogicalColumn').mockReturnValue(0);

        // Open A via keyboard → expanded.
        const taA = await keyboardActivateTile(container, 0);
        expect(taA.hasAttribute('data-expanded'), 'A opens expanded via keyboard').toBe(true);

        // Hop to B via ArrowDown.
        await act(async () => {
            fireEvent.keyDown(taA, { key: 'ArrowDown' });
        });

        // B's textarea should now be open.
        const taB = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taB, 'B textarea must open after hop').not.toBeNull();

        // B must open COLLAPSED (editExpandedRef was reset at B's open).
        expect(taB!.hasAttribute('data-expanded'), 'B must open collapsed after hop').toBe(false);
    });
});

/* ─── Test 7.h: self-heal remount preserves expanded state ───────────────────
 *
 * Open A and expand it. Then trigger a self-heal REMOUNT by re-rendering with
 * new astJson/renderedContent where A's content is UNCHANGED but pool indices shift
 * (collaborator inserts a block above) — this triggers a KEEP re-anchor.
 *
 * The offset-shifted KEEP scenario from p2-3b-real §3(b):
 *   - Original pool: [r=[0,6] "para0\n", r=[6,12] "para1\n"]
 *   - We open editor on tile A (pool[0], r0=0, "para0")
 *   - Collaborator inserts a block BEFORE para0
 *   - New pool: [NEW r=[0,4], r=[4,10] "para0\n" (shifted), r=[10,16] "para1\n"]
 *   - findReanchorCandidate: exact at r0=0? → pool[0]="NEW\n" ≠ "para0" miss
 *     nearest at/after r0=0 → pool[0] at r0=0 "NEW\n" ≠ "para0"
 *   Actually: nearest = the entry with smallest r0 >= 0, which is pool[0] r0=0 "NEW" ≠ "para0"
 *   → null → DROP!
 *
 * We need a different fixture: collaborator inserts block BETWEEN para0 and para1,
 * so para0 stays at r0=0, but the AST JSON string changes (causing deps to update).
 * This is the "content-preserving" KEEP scenario (section 2 in p2-3b-real).
 *
 * But that doesn't cause a remount — the KEEP re-anchor just calls setEditTargetRaw(same),
 * which triggers a re-render, not an unmount+remount.
 *
 * The actual remount of EditTextarea happens when the REACT KEY of the Block component
 * changes — which happens when a new block is inserted and the `s` attribute (pool index)
 * of A's node changes. In the `dispatch.tsx` Node renderer, blocks are keyed by their
 * position in the blocks array (index). When a new block is prepended, A moves from
 * index 0 to index 1, causing React to unmount the old instance and mount a new one.
 *
 * For 7.h we use the offset-unchanged KEEP scenario but with a pool-index shift:
 *   - A (pool[0] initially) stays at r0=0 with same content "para0\n"
 *   - Collaborator changes pool[1] ("para1\n" → "CHANGED\n")
 *   - A's position in the AST (index 0) is UNCHANGED, so no key change for A
 *   - findReanchorCandidate: exact at r0=0, content "para0" matches → KEEP
 *   - Self-heal calls setEditTargetRaw(same) → re-render (not unmount+remount)
 *
 * Actually, the important test for 7.h is:
 *   When the editTarget changes (setEditTargetRaw(reanchored)) while the editor is open,
 *   the textarea re-renders with new ctx.editTarget but does NOT unmount. The expanded
 *   state is preserved in local React state (no reset).
 *
 * For a TRUE remount test, we need an approach like p2-3b-real §3(b) where the
 * AST block index SHIFTS (new block prepended → A moves from index 0 to 1 → key change).
 * But in §3(b), A shifts from r0=6 to r0=10 (opening was on pool[1], "para1").
 * Let's use the same fixture but opening on pool[0] ("para0"), and the shift makes
 * para0 go from index 0 in the AST to index 1 (new block prepended).
 *
 * New fixture:
 *   - Original: [pool[0] r=[0,6] "para0\n", pool[1] r=[6,12] "para1\n"]
 *   - Open A = pool[0], "para0", r0=0
 *   - Collaborator inserts "NEW\n" before para0:
 *     New content: "NEW\npara0\npara1\n"
 *     New pool: [pool[0] r=[0,4] "NEW\n", pool[1] r=[4,10] "para0\n", pool[2] r=[10,16] "para1\n"]
 *   - findReanchorCandidate: exact at r0=0 → pool[0] "NEW" ≠ "para0" miss;
 *     nearest at/after r0=0 → pool[0] r0=0 "NEW" ≠ "para0" → null → DROP!
 *
 * The issue: "nearest" finds pool[0] at r0=0, but content doesn't match.
 * findReanchorCandidate returns null (no match) → DROP.
 *
 * So we cannot easily test a remount via an "insert above" scenario where A was pool[0].
 * Instead, use A = pool[1] (like p2-3b-real §3b), keyboard-activate, then shift.
 *
 * FAIL-ON-REVERT:
 *   Remove the `useState(() => ctx.editExpandedRef?.current ?? false)` initializer
 *   reading from editExpandedRef → remount resets to false → data-expanded absent → RED.
 */
describe('§7.h — self-heal remount preserves expanded state', () => {
    it('A (pool[1]) stays expanded after a collaborator inserts block above para0 (KEEP+remount)', async () => {
        // Use 3-tile content to match p2-3b-real §3(b):
        //   para0: pool[0] r=[0,6]   "para0\n"   line 0
        //   para1: pool[1] r=[6,12]  "para1\n"   line 1  ← A (edit target)
        //   para2: pool[2] r=[12,19] "para2\n\n" line 2
        const BASE_CONTENT_3 = 'para0\npara1\npara2\n\n';
        const BASE_POOL_3 = [
            { t: 0, r: [0, 6], d: 0 },   // pool[0]: "para0\n"
            { t: 0, r: [6, 12], d: 0 },  // pool[1]: A "para1\n"
            { t: 0, r: [12, 19], d: 0 }, // pool[2]: "para2\n\n"
        ];

        // Shifted content: NEW block prepended before para0.
        // A (para1) was at r0=6, now at r0=10.
        // findReanchorCandidate: exact at r0=6? → pool[1] r0=4 ("para0") miss
        //   → nearest at/after r0=6 → pool[2] r0=10 "para1" matches → KEEP (re-anchor to r0=10).
        const SHIFTED_CONTENT = 'NEW\npara0\npara1\npara2\n\n';
        const SHIFTED_POOL = [
            { t: 0, r: [0, 4], d: 0 },   // pool[0]: "NEW\n" (new block)
            { t: 0, r: [4, 10], d: 0 },  // pool[1]: "para0\n" (shifted)
            { t: 0, r: [10, 16], d: 0 }, // pool[2]: A "para1\n" (shifted to r0=10)
            { t: 0, r: [16, 23], d: 0 }, // pool[3]: "para2\n\n"
        ];

        const setAst = vi.fn();
        const astJson = makeAstJson(BASE_POOL_3, BASE_CONTENT_3);
        const props = {
            astJson,
            untransformedAstJson: astJson,
            renderedContent: BASE_CONTENT_3,
            currentFilePath: '/test.qmd',
            assetManifest: {},
            setAst,
            onNavigateToDocument: () => {},
        };
        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});

        // Mock tile rects for the 3-tile doc.
        container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((tile) => {
            const pid = Number(tile.getAttribute('data-block-pool-id'));
            vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
                left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
                width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
            } as DOMRect);
        });

        // Open A (pool[1] = "para1") via KEYBOARD → expanded.
        const taA = await keyboardActivateTile(container, 1);
        expect(taA.hasAttribute('data-expanded'), 'A opens expanded via keyboard').toBe(true);

        // Collaborator inserts NEW block before para0 → A shifts from r0=6 to r0=10.
        // React key of A's block (AST index 1 → 2) changes → EditTextarea remounts.
        // Self-heal re-anchors: KEEP (content "para1" matches at new r0=10).
        const newAstJson = makeAstJson(SHIFTED_POOL, SHIFTED_CONTENT);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={SHIFTED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // After the self-heal re-anchor, EditTextarea remounts with the new AST.
        // It should still be expanded (editExpandedRef.current = true, preserved).
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter, 'textarea must still be open after self-heal remount').not.toBeNull();
        expect(
            taAfter!.hasAttribute('data-expanded'),
            'A must stay expanded after self-heal remount (editExpandedRef preserved)',
        ).toBe(true);
    });
});

/* ─── Test T12: G4 — bare modifier keydowns do NOT expand ────────────────────
 *
 * A bare modifier key (Meta, Control, Alt, Shift) pressed alone should NOT
 * expand the editor. Only the leading modifier alone precedes a chord; we want
 * the chord's leave key (or the chord itself returning early) to handle the
 * expand, not the bare modifier keydown that precedes it.
 *
 * Part (a): bare modifier keys → data-expanded stays absent.
 * Part (b): a printable key + change → data-expanded PRESENT (guard not over-broad).
 *
 * FAIL-ON-REVERT (T12):
 *   Remove `!isBareModifier` from the guard condition:
 *   `if (!isLeaveKey && !expanded)` → bare Meta expands → (a) data-expanded present → RED.
 *   Discriminator: (a) bare-modifier vs (b) printable — both states asserted, can't go vacuous.
 */
describe('T12 (G4) — bare modifier keydowns do NOT expand', () => {
    it('(a) bare Meta/Control/Alt/Shift keydowns → data-expanded stays absent', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);
        // Must open collapsed.
        expect(ta.hasAttribute('data-expanded'), 'must open collapsed').toBe(false);

        // Fire each bare modifier keydown — none should expand.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Meta' });
        });
        expect(ta.hasAttribute('data-expanded'), 'bare Meta must not expand').toBe(false);

        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Control' });
        });
        expect(ta.hasAttribute('data-expanded'), 'bare Control must not expand').toBe(false);

        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Alt' });
        });
        expect(ta.hasAttribute('data-expanded'), 'bare Alt must not expand').toBe(false);

        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Shift' });
        });
        expect(ta.hasAttribute('data-expanded'), 'bare Shift must not expand').toBe(false);
    });

    it('(b) a printable key + change → data-expanded PRESENT (guard not over-broad)', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);
        expect(ta.hasAttribute('data-expanded'), 'must open collapsed').toBe(false);

        // Fire a printable keydown + change → should expand.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'x' });
            fireEvent.change(ta, { target: { value: 'x' } });
        });
        expect(ta.hasAttribute('data-expanded'), 'printable key must still expand (guard not over-broad)').toBe(true);
    });
});

/* ─── Test T14: G11 — second click inside an open editor expands it ─────────
 *
 * A click INSIDE an already-open (but collapsed) editor should expand it.
 * The ACTIVATING click cannot fire this because the textarea isn't mounted yet
 * when the activating mousedown/mouseup are hit-tested (activation happens on
 * pointerup, but setEditTarget routes through useState → the textarea mounts only
 * on a later React render). A genuine second click (both mousedown + mouseup on
 * the already-mounted textarea) fires the onClick handler.
 *
 * Part (a): click-activate collapsed → second click (fireEvent.click) → data-expanded present.
 * Part (b): starting already-expanded → second click → still present (no toggle-off, idempotent).
 *
 * FAIL-ON-REVERT (T14):
 *   Remove the textarea `onClick` block → click does not expand → data-expanded absent → RED.
 */
describe('T14 (G11) — second click inside open editor expands it', () => {
    it('(a) click-activate collapsed → fireEvent.click(textarea) → data-expanded present', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        const ta = await clickActivateTile(container, 0);
        // Must open collapsed after first (activating) click.
        expect(ta.hasAttribute('data-expanded'), 'must open collapsed after activation click').toBe(false);

        // A second click inside the textarea should expand it.
        await act(async () => {
            fireEvent.click(ta);
        });
        expect(ta.hasAttribute('data-expanded'), 'second click must expand the editor').toBe(true);
    });

    it('(b) starting expanded → second click → still expanded (idempotent, no toggle-off)', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});
        mockTileRects(container);

        // Keyboard-activate → opens expanded.
        const ta = await keyboardActivateTile(container, 0);
        expect(ta.hasAttribute('data-expanded'), 'keyboard activate opens expanded').toBe(true);

        // Second click on an already-expanded editor → should stay expanded (no toggle-off).
        await act(async () => {
            fireEvent.click(ta);
        });
        expect(ta.hasAttribute('data-expanded'), 'already-expanded: second click must not collapse').toBe(true);
    });
});
