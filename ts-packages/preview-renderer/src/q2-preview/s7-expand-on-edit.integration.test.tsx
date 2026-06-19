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
 *          grow holds: scrollHeight > contentHeight → style.height = scrollHeight px.
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
import React, { useContext } from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';
import * as caretGeometry from './caretGeometry';
import { PreviewContext } from './PreviewContext';
import type { MutableRefObject } from 'react';

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

/**
 * Context-capture probe: renders inside the PreviewContext.Provider via the
 * customRegistry Para slot. Captures ctx.editExpandedRef into a stable holder
 * so tests can read editExpandedRef.current without being inside the React tree.
 *
 * At least one Para tile is always rendered (the non-edited one), so the holder
 * is populated on every render cycle. Since editExpandedRef is a stable ref
 * object (same MutableRefObject<boolean> for the lifetime of PreviewRoot),
 * reading holder.current.current after any fireEvent gives the live value.
 *
 * Must emit data-block-pool-id to preserve the tile seam used by clickActivateTile /
 * keyboardActivateTile (the production Para.tsx does the same). Without the attribute,
 * tile queries return null and activation fails.
 */
function makeContextCapture() {
    const holder: { current: MutableRefObject<boolean> | undefined } = { current: undefined };
    function ContextCapturePara(args: { node: { s?: string | number }; [k: string]: unknown }) {
        const ctx = useContext(PreviewContext);
        holder.current = ctx?.editExpandedRef as MutableRefObject<boolean> | undefined;
        const poolId = args.node.s;
        const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node as any) : null;
        const isEditable = resolved != null
            && resolved.reachabilityClass !== 'Opaque'
            && poolId !== undefined
            && !ctx?.editingDisabled;
        return (
            <p {...(isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {})}>
                para
            </p>
        );
    }
    return { holder, ContextCapturePara };
}

function mountPreviewRoot(
    opts: {
        setAst?: (ast: PandocAST) => void;
        pool?: typeof POOL_AB;
        content?: string;
        customRegistry?: Record<string, React.ComponentType<any>>;
    } = {},
) {
    const pool = opts.pool ?? POOL_AB;
    const content = opts.content ?? CONTENT_AB;
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson(pool, content);
    const { holder, ContextCapturePara } = makeContextCapture();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
        // Inject the context-capture probe as the Para renderer so tests can
        // access editExpandedRef.current from outside the React tree.
        customRegistry: { Para: ContextCapturePara, ...(opts.customRegistry ?? {}) },
    };
    const result = render(<PreviewRoot {...props} />);
    // editExpandedRef is the ref object captured by ContextCapturePara.
    // It is the same MutableRefObject<boolean> for the lifetime of PreviewRoot.
    return { ...result, setAst, pool, content, astJson, editExpandedRefHolder: holder };
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
 *   - edge ArrowDown (onEdge=true) → navigates away, editExpandedRef stays false.
 *   - Esc → closes editor, editExpandedRef stays false.
 *   - Cmd/Ctrl+Enter → commits + closes, editExpandedRef stays false.
 *
 * Binding: assert on ctx.editExpandedRef.current (read from ContextCapturePara).
 * The DOM-attribute approach was theater: after a leave key, the textarea unmounts
 * (the leave action closes the editor), so `data-expanded` is read on a detached
 * element and is always absent — even if the expand guard is removed.
 * editExpandedRef.current is set SYNCHRONOUSLY in onKeyDown before any unmount;
 * the guard (! isLeaveKey) prevents the write for leave keys.
 *
 * Positive control: a printable key sets editExpandedRef.current = true
 * (tested per sub-case to confirm the ref is wired).
 *
 * Vacuity guard: the leave action must actually have fired (not just "nothing happened").
 *
 * FAIL-ON-REVERT:
 *   Remove the `!isLeaveKey` check from the onKeyDown expand guard in dispatchers.tsx
 *   → leave keys call `editExpandedRef.current = true` synchronously
 *   → each sub-case's `editExpandedRef.current === false` assertion fails → RED.
 */
describe('§7.c — leave keys do not expand', () => {
    it('edge ArrowDown does not set editExpandedRef; positive control: printable key does', async () => {
        const setAst = vi.fn();
        const { container, editExpandedRefHolder } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);
        vi.spyOn(caretGeometry, 'getLogicalColumn').mockReturnValue(0);

        // Open tile A via pointer (collapsed).
        const ta = await clickActivateTile(container, 0);

        // The ContextCapturePara probe has now rendered (tile B is visible).
        // editExpandedRef must be populated.
        expect(editExpandedRefHolder.current, 'editExpandedRef should be captured by probe').toBeDefined();
        const editExpandedRef = editExpandedRefHolder.current!;

        // Precondition: editExpandedRef starts false (collapsed open).
        expect(editExpandedRef.current, 'editExpandedRef.current must be false before leave key').toBe(false);

        // Make the draft DIRTY so ArrowDown takes the async commit path (not the
        // synchronous hop path). The synchronous hop path calls openEditTarget which
        // itself resets editExpandedRef.current = false, masking the revert. The
        // async path (dirty) does NOT call openEditTarget synchronously — it stashes
        // a pending landing — leaving editExpandedRef.current intact for the check.
        await act(async () => {
            fireEvent.change(ta, { target: { value: 'dirty' } });
        });
        // editExpandedRef.current must still be false (change doesn't trigger keyDown expand guard).
        expect(editExpandedRef.current, 'editExpandedRef must still be false after onChange').toBe(false);

        // Fire edge ArrowDown (leave key) with dirty draft → async commit path.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'ArrowDown' });
        });

        // Binding assertion: the leave-key guard must have prevented the expand write.
        // With the revert (!isLeaveKey removed): editExpandedRef.current = true (set
        // synchronously in the handler, not reset by the async dirty path) → RED.
        expect(editExpandedRef.current, 'edge ArrowDown must NOT set editExpandedRef').toBe(false);

        // Vacuity guard: isOnLastVisualLine was called (the edge-detection path ran).
        expect(caretGeometry.isOnLastVisualLine).toHaveBeenCalled();
        // Vacuity guard: setAst was called (dirty commit fired via the async path).
        expect(setAst, 'setAst must be called for dirty ArrowDown hop').toHaveBeenCalled();

        // ── Positive control: open a fresh editor and fire a printable key ────
        // This confirms the ref IS being written (not broken by a test setup issue).
        // Editor closed after dirty hop (pending landing stashed). Reopen tile A.
        await act(async () => {}); // flush any remaining effects
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        if (tileA) {
            await act(async () => {
                fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
                fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
            });
        }
        const ta2 = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta2, 'a textarea must be open for positive control').not.toBeNull();
        // editExpandedRef was reset to false on the new open (pointer activation).
        expect(editExpandedRef.current, 'editExpandedRef reset to false on re-open').toBe(false);
        // Fire a printable key → expand guard fires → editExpandedRef.current = true.
        fireEvent.keyDown(ta2!, { key: 'a' });
        expect(editExpandedRef.current, 'printable key MUST set editExpandedRef (positive control)').toBe(true);
    });

    it('Esc does not set editExpandedRef; positive control: printable key does', async () => {
        const setAst = vi.fn();
        const { container, editExpandedRefHolder } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // Open tile A via pointer (collapsed).
        const ta = await clickActivateTile(container, 0);

        const editExpandedRef = editExpandedRefHolder.current!;
        expect(editExpandedRef, 'editExpandedRef should be captured').toBeDefined();

        // Precondition: collapsed.
        expect(editExpandedRef.current, 'editExpandedRef.current must be false before Esc').toBe(false);

        // Fire Esc (leave key).
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Escape' });
        });

        // Binding assertion: Esc must NOT set editExpandedRef.
        expect(editExpandedRef.current, 'Esc must NOT set editExpandedRef').toBe(false);

        // Vacuity guard: editor is closed (Esc fired the leave action).
        expect(container.querySelector('textarea'), 'editor must be closed after Esc').toBeNull();

        // ── Positive control ────────────────────────────────────────────────
        // Re-open tile A via pointer.
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tileA).not.toBeNull();
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        const ta2 = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta2, 'editor should reopen').not.toBeNull();
        // editExpandedRef was reset to false on the new open.
        expect(editExpandedRef.current, 'editExpandedRef reset to false on re-open').toBe(false);
        // Fire a printable key.
        fireEvent.keyDown(ta2!, { key: 'a' });
        expect(editExpandedRef.current, 'printable key MUST set editExpandedRef (positive control)').toBe(true);
    });

    it('Cmd+Enter does not set editExpandedRef; positive control: printable key does', async () => {
        const setAst = vi.fn();
        const { container, editExpandedRefHolder } = mountPreviewRoot({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // Open tile A via pointer (collapsed).
        const ta = await clickActivateTile(container, 0);
        // Make the draft dirty so commit fires.
        await act(async () => {
            fireEvent.change(ta, { target: { value: 'newcontent' } });
        });

        const editExpandedRef = editExpandedRefHolder.current!;
        expect(editExpandedRef, 'editExpandedRef should be captured').toBeDefined();

        // Precondition: collapsed (no keyDown yet, so not expanded).
        expect(editExpandedRef.current, 'editExpandedRef.current must be false before Cmd+Enter').toBe(false);

        // Fire Cmd+Enter (leave key = commit chord).
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
        });

        // Binding assertion: Cmd+Enter must NOT set editExpandedRef.
        expect(editExpandedRef.current, 'Cmd+Enter must NOT set editExpandedRef').toBe(false);

        // Vacuity guard: setAst called (commit fired) + editor closed.
        expect(setAst, 'setAst must be called (commit fired)').toHaveBeenCalled();
        expect(container.querySelector('textarea'), 'editor must be closed after Cmd+Enter').toBeNull();

        // ── Positive control ────────────────────────────────────────────────
        // Re-open tile A via pointer.
        const tileA = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tileA).not.toBeNull();
        await act(async () => {
            fireEvent(tileA!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tileA!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        const ta2 = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta2, 'editor should reopen').not.toBeNull();
        expect(editExpandedRef.current, 'editExpandedRef reset to false on re-open').toBe(false);
        fireEvent.keyDown(ta2!, { key: 'a' });
        expect(editExpandedRef.current, 'printable key MUST set editExpandedRef (positive control)').toBe(true);
    });
});

/* ─── Test 7.d: floor holds AND grow holds ────────────────────────────────────
 *
 * Two cases, both verifying Math.max(contentHeight, ta.scrollHeight):
 *
 * GROW case: scrollHeight=200 > contentHeight=100 → height must be 200px.
 *   Reverting Math.max(...) → contentHeight → height becomes 100px (won't grow) → RED.
 *
 * FLOOR case: scrollHeight=40 < contentHeight=100 → height must be 100px.
 *   Reverting Math.max(...) → ta.scrollHeight (bare) → height becomes 40px (shrinks) → RED.
 *
 * Both reverts are needed to fully bind Math.max in both directions.
 *
 * FAIL-ON-REVERT:
 *   - Revert `Math.max(contentHeight, ta.scrollHeight)` → `contentHeight`:
 *     GROW case fails: height stays 100px instead of 200px → RED.
 *   - Revert `Math.max(contentHeight, ta.scrollHeight)` → `ta.scrollHeight`:
 *     FLOOR case fails: height becomes 40px instead of 100px → RED.
 */
describe('§7.d — height = Math.max(contentHeight, scrollHeight): floor and grow', () => {
    it('GROW: scrollHeight(200) > contentHeight(100) → style.height = 200px', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});

        // Mock tile A with height=100 → contentHeight=100.
        const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tile).not.toBeNull();
        vi.spyOn(tile!, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: 0, right: 200, bottom: 100,
            width: 200, height: 100, x: 0, y: 0, toJSON: () => ({}),
        } as DOMRect);
        // Mock other tiles (tile B).
        container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((el) => {
            if (el !== tile) {
                const pid = Number(el.getAttribute('data-block-pool-id'));
                vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
                    left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
                    width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
                } as DOMRect);
            }
        });

        // Click-activate tile A (contentHeight=100).
        const ta = await clickActivateTile(container, 0);

        // Stub scrollHeight=200 on the textarea BEFORE firing the key that
        // triggers the layoutEffect that reads scrollHeight.
        Object.defineProperty(ta, 'scrollHeight', { value: 200, configurable: true });

        // Fire a printable key to trigger expansion and the layoutEffect.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'a' });
        });

        // After expansion: data-expanded must be set.
        expect(ta.hasAttribute('data-expanded'), 'must be expanded after key').toBe(true);

        // GROW: height = max(contentHeight=100, scrollHeight=200) = 200 → '200px'.
        // Reverting Math.max → contentHeight gives '100px' → RED.
        expect(ta.style.height, 'GROW: height must be scrollHeight (200px) when scrollHeight > contentHeight').toBe('200px');
    });

    it('FLOOR: scrollHeight(40) < contentHeight(100) → style.height = 100px', async () => {
        const { container } = mountPreviewRoot();
        await act(async () => {});

        // Mock tile A with height=100 → contentHeight=100.
        const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tile).not.toBeNull();
        vi.spyOn(tile!, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: 0, right: 200, bottom: 100,
            width: 200, height: 100, x: 0, y: 0, toJSON: () => ({}),
        } as DOMRect);
        container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((el) => {
            if (el !== tile) {
                const pid = Number(el.getAttribute('data-block-pool-id'));
                vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
                    left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
                    width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
                } as DOMRect);
            }
        });

        // Click-activate tile A (contentHeight=100).
        const ta = await clickActivateTile(container, 0);

        // Stub scrollHeight=40 (below contentHeight) on the textarea.
        Object.defineProperty(ta, 'scrollHeight', { value: 40, configurable: true });

        // Fire a printable key to trigger expansion and the layoutEffect.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'a' });
        });

        // After expansion: data-expanded must be set.
        expect(ta.hasAttribute('data-expanded'), 'must be expanded after key').toBe(true);

        // FLOOR: height = max(contentHeight=100, scrollHeight=40) = 100 → '100px'.
        // Reverting Math.max → ta.scrollHeight (bare) gives '40px' → RED.
        expect(ta.style.height, 'FLOOR: height must be clamped to contentHeight (100px) when scrollHeight < contentHeight').toBe('100px');
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
 * Open A via POINTER (click), then expand it via a real in-surface printable
 * key (so setExpanded(true) fires and editExpandedRef.current=true). Then trigger
 * a collaborator-shift REMOUNT (array-index key change from block inserted above).
 * After the remount, the textarea must still be expanded.
 *
 * This test isolates the remount-preserve from §7.b (keyboard-open) by opening
 * via POINTER + expand-by-typing. If the test's open step depended on keyboard
 * activation (like the old §7.h), a revert of the useState initializer would
 * fail at the OPEN step, not at the REMOUNT step — making the binding ambiguous.
 * With pointer-open + expand-by-typing, the ONLY thing that preserves expansion
 * across the remount is `useState(() => ctx.editExpandedRef?.current ?? false)`.
 *
 * FAIL-ON-REVERT:
 *   `useState(() => ctx.editExpandedRef?.current ?? false)` → `useState(false)`:
 *   After the remount, the new EditTextarea instance initializes collapsed.
 *   The POST-REMOUNT assertion (`data-expanded` present) fails → RED.
 *   Because the open was POINTER + expand-by-typing (not keyboard-open), the test
 *   REACHES the remount step — so the RED is specifically at the remount binding.
 *
 * Scenario uses the offset-shifted KEEP fixture from p2-3b-real §3(b):
 *   A = pool[1] ("para1"), collaborator inserts NEW block before para0.
 *   A shifts from AST index 1 to index 2 → React key change → EditTextarea remounts.
 *   Self-heal re-anchors: KEEP (content "para1" found at new r0=10).
 */
describe('§7.h — self-heal remount preserves expanded state', () => {
    it('A stays expanded after pointer-open + expand-by-typing + collaborator remount', async () => {
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
        // findReanchorCandidate: nearest at/after r0=6 → pool[2] r0=10 "para1" matches → KEEP.
        const SHIFTED_CONTENT = 'NEW\npara0\npara1\npara2\n\n';
        const SHIFTED_POOL = [
            { t: 0, r: [0, 4], d: 0 },   // pool[0]: "NEW\n" (new block)
            { t: 0, r: [4, 10], d: 0 },  // pool[1]: "para0\n" (shifted)
            { t: 0, r: [10, 16], d: 0 }, // pool[2]: A "para1\n" (shifted to r0=10)
            { t: 0, r: [16, 23], d: 0 }, // pool[3]: "para2\n\n"
        ];

        const setAst = vi.fn();
        const astJson = makeAstJson(BASE_POOL_3 as typeof POOL_AB, BASE_CONTENT_3);

        // Build the ContextCapturePara probe to capture editExpandedRef.
        const { holder, ContextCapturePara } = makeContextCapture();

        const props = {
            astJson,
            untransformedAstJson: astJson,
            renderedContent: BASE_CONTENT_3,
            currentFilePath: '/test.qmd',
            assetManifest: {},
            setAst,
            onNavigateToDocument: () => {},
            customRegistry: { Para: ContextCapturePara },
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

        // ── Step 1: Open A (pool[1] = "para1") via POINTER (click) → collapsed. ──
        const taA = await clickActivateTile(container, 1);
        expect(taA.hasAttribute('data-expanded'), 'A opens collapsed via pointer').toBe(false);

        // Confirm editExpandedRef was captured by the probe.
        const editExpandedRef = holder.current!;
        expect(editExpandedRef, 'editExpandedRef must be captured').toBeDefined();
        expect(editExpandedRef.current, 'editExpandedRef.current must be false after pointer open').toBe(false);

        // ── Step 2: Expand via a real in-surface keystroke. ────────────────────
        // Fire a printable key → setExpanded(true) + editExpandedRef.current=true.
        await act(async () => {
            fireEvent.keyDown(taA, { key: 'a' });
        });

        // Confirm expanded via DOM attribute.
        expect(taA.hasAttribute('data-expanded'), 'A must be expanded after printable key').toBe(true);
        // Confirm via ref (synchronously set by onKeyDown).
        expect(editExpandedRef.current, 'editExpandedRef.current must be true after printable key').toBe(true);

        // ── Step 3: Collaborator inserts NEW block → A shifts (REMOUNT). ───────
        // A (para1) was AST index 1 → now AST index 2 → React key change → remount.
        const newAstJson = makeAstJson(SHIFTED_POOL as typeof POOL_AB, SHIFTED_CONTENT);
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
                    customRegistry={{ Para: ContextCapturePara }}
                />,
            );
        });

        // ── Step 4: POST-REMOUNT assertion — the binding step. ─────────────────
        // The new EditTextarea instance must initialize with expanded=true
        // because `useState(() => ctx.editExpandedRef?.current ?? false)` reads
        // editExpandedRef.current=true (set in step 2, preserved across the remount).
        //
        // With revert (`useState(false)`): the new instance starts collapsed →
        // data-expanded absent → this assertion FAILS → RED.
        // The RED is specifically at THIS step (not earlier) because the open was
        // pointer + expand-by-typing, not keyboard-open.
        const taAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter, 'textarea must still be open after self-heal remount').not.toBeNull();
        expect(
            taAfter!.hasAttribute('data-expanded'),
            'A must stay expanded after self-heal remount (editExpandedRef.current=true preserved)',
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
