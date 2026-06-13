/**
 * RTL tests for useBlockEditHover (Plan 2b).
 *
 * Tests: mouse activation + outline-clear, touch progressive-press
 * (hold / early-release / move-beyond-threshold cancel), keyboard
 * Enter activation and Escape.
 *
 * jsdom gotchas addressed:
 *  - PointerEvent.pointerType is NOT set from constructor init in jsdom 26.
 *    Use Object.defineProperty via the `ptrEvent` helper for any test
 *    where the handler branches on `e.pointerType === 'mouse'`.
 *  - setPointerCapture is not implemented in jsdom — stub it via
 *    HTMLElement.prototype before firing touch pointerDown events.
 *  - getBoundingClientRect returns zeroes by default — spy whenever
 *    activation is expected so the rect in setEditTarget is non-trivial.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import { axe } from 'vitest-axe';
import React, { useRef } from 'react';
import type { ReactNode } from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { useBlockEditHover } from './useBlockEditHover';
import { RegistryContext } from '../framework';
import type { ResolvedSource } from './sourceIndex';
import { Para } from './blocks/Para';
import { Header } from './blocks/Header';

afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
});

/* ── PointerEvent helper ─────────────────────────────────────────────
 * jsdom's PointerEvent does not honour `pointerType` from the
 * constructor init dict, so React sees e.pointerType === undefined.
 * Object.defineProperty forces the value onto the event object so
 * the hook's `e.pointerType !== 'mouse'` branch evaluates correctly.
 */
function ptrEvent(
    type: string,
    opts: PointerEventInit & { clientX?: number; clientY?: number } = {},
): Event {
    const PE = (window as any).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    // jsdom's PointerEvent does not honour pointerType, clientX, or clientY
    // from the constructor init dict — force them via Object.defineProperty.
    for (const [key, val] of Object.entries({
        ...(opts.pointerType !== undefined ? { pointerType: opts.pointerType } : {}),
        ...(opts.clientX !== undefined ? { clientX: opts.clientX } : {}),
        ...(opts.clientY !== undefined ? { clientY: opts.clientY } : {}),
    } as Record<string, unknown>)) {
        Object.defineProperty(evt, key, { value: val, configurable: true });
    }
    return evt;
}

/* ── Minimal host component ─────────────────────────────────────────── */

// P2.3a: pool entries must be present for activate() to resolve anchorR0/anchorR1.
// Block with data-block-pool-id="5" → pool[5] = { t: 0, r: [100, 200], d: 0 }
// Block with data-block-pool-id="6" → pool[6] = { t: 0, r: [300, 400], d: 0 }
// anchorR0 values: block5 → 100, block6 → 300.
const POOL_ENTRY_5 = { t: 0, r: [100, 200] as [number, number], d: 0 };
const POOL_ENTRY_6 = { t: 0, r: [300, 400] as [number, number], d: 0 };
const POOL_WITH_5: unknown[] = [
    ...Array.from({ length: 5 }, () => null),
    POOL_ENTRY_5,
    POOL_ENTRY_6,
];

function Inner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <p data-block-pool-id="5" data-testid="block5">block 5</p>
        </div>
    );
}

function BlockHost({
    setEditTarget,
    editingDisabled,
}: {
    setEditTarget: (t: any) => void;
    editingDisabled?: boolean;
}) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
        pool: POOL_WITH_5,
        content: '',
        ...(editingDisabled !== undefined ? { editingDisabled } : {}),
    };
    return (
        <PreviewContext.Provider value={ctx}>
            <Inner />
        </PreviewContext.Provider>
    );
}

const MOCK_RECT: DOMRect = {
    width: 200, height: 40, top: 100, bottom: 140,
    left: 0, right: 200, x: 0, y: 100, toJSON: () => ({}),
};

function mountHost(opts: { editingDisabled?: boolean } = {}) {
    const setEditTarget = vi.fn();
    const utils = render(
        <BlockHost setEditTarget={setEditTarget} editingDisabled={opts.editingDisabled} />,
    );
    return { ...utils, setEditTarget };
}

/* ── Mouse activation ───────────────────────────────────────────────── */

describe('useBlockEditHover — mouse activation', () => {
    it('calls setEditTarget with anchorR0 and rect on mouse pointerup', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        const arg = setEditTarget.mock.calls[0][0];
        // P2.3a: identity is now anchorR0 (byte offset from pool entry), not poolId.
        // pool[5].r[0] = 100 (POOL_ENTRY_5)
        expect(arg.anchorR0).toBe(100);
        expect(arg.anchorR1).toBe(200);
        // The measure-and-set wrapper consumes boxStyle (the captured computed
        // box) and contentHeight rather than the raw rect.
        expect(arg.boxStyle).toBeTypeOf('object');
        expect(arg.contentHeight).toBeTypeOf('number');
    });

    it('clears box-shadow on the activated element after activation', () => {
        const { getByTestId } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // Hover first to apply the outline
        fireEvent(block, ptrEvent('pointermove', { pointerType: 'mouse' }));
        expect(block.style.boxShadow).toBeTruthy();

        // Activate — outlineElement(null) removes the property
        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(block.style.boxShadow).toBe('');
    });
});

/* ── Touch progressive-press ────────────────────────────────────────── */

describe('useBlockEditHover — touch progressive-press', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        // setPointerCapture is not defined in jsdom — define a no-op stub
        // so the touch-path onPointerDown handler can call it without throwing,
        // allowing the hold-timer setTimeout to be reached.
        Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
            value: vi.fn(),
            writable: true,
            configurable: true,
        });
    });

    afterEach(() => {
        delete (HTMLElement.prototype as any).setPointerCapture;
    });

    it('does NOT activate immediately on pointerdown (touch)', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 10 }));

        expect(setEditTarget).not.toHaveBeenCalled();
    });

    it('activates after holding for HOLD_MS (500ms)', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 10 }));
        expect(setEditTarget).not.toHaveBeenCalled();

        vi.advanceTimersByTime(500);

        expect(setEditTarget).toHaveBeenCalledOnce();
    });

    it('does NOT activate when pointerup fires before 500ms (touch)', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 10 }));
        vi.advanceTimersByTime(200);
        // pointerUp in touch path clears the hold timer
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'touch' }));
        vi.advanceTimersByTime(500);

        expect(setEditTarget).not.toHaveBeenCalled();
    });

    it('does NOT activate when move exceeds MOVE_THRESHOLD_PX (8px) before hold', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const host = getByTestId('host');
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // pointerDown sets pointerDownPosRef = { x: 10, y: 10 }
        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 10 }));
        // dx = 20 - 10 = 10 > MOVE_THRESHOLD_PX(8) → clearHold
        fireEvent(host, ptrEvent('pointermove', { pointerType: 'touch', clientX: 20, clientY: 10 }));
        vi.advanceTimersByTime(600);

        expect(setEditTarget).not.toHaveBeenCalled();
    });
});

/* ── Keyboard ───────────────────────────────────────────────────────── */

describe('useBlockEditHover — keyboard', () => {
    it('activates the hovered element on Enter key', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const host = getByTestId('host');
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // Hover to set hoveredRef.current = block
        fireEvent(block, ptrEvent('pointermove', { pointerType: 'mouse' }));

        // Enter on the host triggers activate(hoveredRef.current)
        fireEvent.keyDown(host, { key: 'Enter' });

        expect(setEditTarget).toHaveBeenCalledOnce();
        const arg = setEditTarget.mock.calls[0][0];
        // P2.3a: identity is now anchorR0, not poolId.
        expect(arg.anchorR0).toBe(100);
        // The measure-and-set wrapper consumes boxStyle (the captured computed
        // box) and contentHeight rather than the raw rect.
        expect(arg.boxStyle).toBeTypeOf('object');
        expect(arg.contentHeight).toBeTypeOf('number');
    });

    it('calls setEditTarget(null) on Escape', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const host = getByTestId('host');

        fireEvent.keyDown(host, { key: 'Escape' });

        expect(setEditTarget).toHaveBeenCalledWith(null);
    });
});

/* ── Keyboard arrow navigation (Plan 2c §1) ─────────────────────────── */

function MultiInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <p data-block-pool-id="1" tabIndex={-1} data-testid="block1">block 1</p>
            <p data-block-pool-id="2" tabIndex={-1} data-testid="block2">block 2</p>
            <p data-block-pool-id="3" tabIndex={-1} data-testid="block3">block 3</p>
        </div>
    );
}

// P2.3a: pool entries for blocks 1, 2, 3 (at pool indices 1, 2, 3).
// anchorR0 values: block1 → 10, block2 → 20, block3 → 30.
const POOL_MULTI: unknown[] = [
    null, // index 0 unused
    { t: 0, r: [10, 15] as [number, number], d: 0 }, // block1
    { t: 0, r: [20, 25] as [number, number], d: 0 }, // block2
    { t: 0, r: [30, 35] as [number, number], d: 0 }, // block3
];

function mountMulti() {
    const setEditTarget = vi.fn();
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
        pool: POOL_MULTI,
        content: '',
    };
    const utils = render(
        <PreviewContext.Provider value={ctx}>
            <MultiInner />
        </PreviewContext.Provider>,
    );
    return { ...utils, setEditTarget };
}

// P2.2 note: these tests mock rects on all blocks so enumerateLockedTiles
// returns a non-empty tile list. Before P2.2, arrow nav used raw
// querySelectorAll (no visibility filter); now it uses enumerateLockedTiles
// which requires isVisibleTile → non-zero rect. The behavior (navigate flat
// tiles in order, wrap at boundaries) is unchanged; only the rect mocking
// requirement is new. Flagged for P2.5 corpus audit.
describe('useBlockEditHover — keyboard arrow navigation', () => {
    it('ArrowDown moves focus to the next block; ArrowUp to the previous', () => {
        const { getByTestId } = mountMulti();
        const host = getByTestId('host');
        const b1 = getByTestId('block1');
        const b2 = getByTestId('block2');
        const b3 = getByTestId('block3');
        // Mock visible rects so enumerateLockedTiles includes all three blocks.
        const r1: DOMRect = { width: 200, height: 20, top: 0, bottom: 20, left: 0, right: 200, x: 0, y: 0, toJSON: () => ({}) };
        const r2: DOMRect = { width: 200, height: 20, top: 20, bottom: 40, left: 0, right: 200, x: 0, y: 20, toJSON: () => ({}) };
        const r3: DOMRect = { width: 200, height: 20, top: 40, bottom: 60, left: 0, right: 200, x: 0, y: 40, toJSON: () => ({}) };
        vi.spyOn(b1, 'getBoundingClientRect').mockReturnValue(r1);
        vi.spyOn(b2, 'getBoundingClientRect').mockReturnValue(r2);
        vi.spyOn(b3, 'getBoundingClientRect').mockReturnValue(r3);
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(b1);
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(b2);
        fireEvent.keyDown(host, { key: 'ArrowUp' });
        expect(document.activeElement).toBe(b1);
    });

    it('ArrowDown on the last block wraps to first; ArrowUp on first wraps to last', () => {
        const { getByTestId } = mountMulti();
        const host = getByTestId('host');
        const b1 = getByTestId('block1');
        const b2 = getByTestId('block2');
        const b3 = getByTestId('block3');
        // Mock visible rects so enumerateLockedTiles includes all three blocks.
        const r1: DOMRect = { width: 200, height: 20, top: 0, bottom: 20, left: 0, right: 200, x: 0, y: 0, toJSON: () => ({}) };
        const r2: DOMRect = { width: 200, height: 20, top: 20, bottom: 40, left: 0, right: 200, x: 0, y: 20, toJSON: () => ({}) };
        const r3: DOMRect = { width: 200, height: 20, top: 40, bottom: 60, left: 0, right: 200, x: 0, y: 40, toJSON: () => ({}) };
        vi.spyOn(b1, 'getBoundingClientRect').mockReturnValue(r1);
        vi.spyOn(b2, 'getBoundingClientRect').mockReturnValue(r2);
        vi.spyOn(b3, 'getBoundingClientRect').mockReturnValue(r3);
        b3.focus();
        expect(document.activeElement).toBe(b3);
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(b1);
        fireEvent.keyDown(host, { key: 'ArrowUp' });
        expect(document.activeElement).toBe(b3);
    });

    it('arrow navigation sets hoveredRef so Enter activates the focused block', () => {
        const { getByTestId, setEditTarget } = mountMulti();
        const host = getByTestId('host');
        const b1 = getByTestId('block1');
        const b2 = getByTestId('block2');
        const b3 = getByTestId('block3');
        // Mock visible rects so enumerateLockedTiles includes all three blocks.
        const r1: DOMRect = { width: 200, height: 20, top: 0, bottom: 20, left: 0, right: 200, x: 0, y: 0, toJSON: () => ({}) };
        const r2: DOMRect = { width: 200, height: 20, top: 20, bottom: 40, left: 0, right: 200, x: 0, y: 20, toJSON: () => ({}) };
        const r3: DOMRect = { width: 200, height: 20, top: 40, bottom: 60, left: 0, right: 200, x: 0, y: 40, toJSON: () => ({}) };
        vi.spyOn(b1, 'getBoundingClientRect').mockReturnValue(r1);
        vi.spyOn(b2, 'getBoundingClientRect').mockReturnValue(r2);
        vi.spyOn(b3, 'getBoundingClientRect').mockReturnValue(r3);
        fireEvent.keyDown(host, { key: 'ArrowDown' }); // -> block1
        fireEvent.keyDown(host, { key: 'ArrowDown' }); // -> block2
        fireEvent.keyDown(host, { key: 'Enter' });
        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for block2 = pool[2].r[0] = 20
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(20);
    });
});

describe('useBlockEditHover — Space activation', () => {
    it('activates the hovered element on Space key', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const host = getByTestId('host');
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // Hover to set hoveredRef.current = block
        fireEvent(block, ptrEvent('pointermove', { pointerType: 'mouse' }));
        fireEvent.keyDown(host, { key: ' ' });

        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for block5 = pool[5].r[0] = 100
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(100);
    });
});

/* ── ARIA host attributes (Plan 2c §1) ──────────────────────────────── */

describe('useBlockEditHover — ARIA host attributes', () => {
    it('host carries tabIndex=0, role="region", aria-label, aria-describedby', () => {
        const { getByTestId } = mountHost();
        const host = getByTestId('host');
        expect(host.getAttribute('tabindex')).toBe('0');
        expect(host.getAttribute('role')).toBe('region');
        expect(host.getAttribute('aria-label')).toBe('Editable preview');
        expect(host.getAttribute('aria-describedby')).toBe('q2-edit-hint');
    });

    it('renders the visually-hidden hint element referenced by aria-describedby', () => {
        const { container } = mountHost();
        const hint = container.querySelector('#q2-edit-hint');
        expect(hint).not.toBeNull();
        expect(hint!.textContent).toMatch(/arrow keys/i);
    });

    it('axe finds no accessibility violations on the editable host', async () => {
        const { container } = mountHost();
        // color-contrast can't run under jsdom (no layout/canvas); disable it
        // so the rule doesn't emit a spurious canvas "Not implemented" error.
        const results = await axe(container, {
            rules: { 'color-contrast': { enabled: false } },
        });
        expect(results.violations.map((v) => v.id)).toEqual([]);
    });
});

/* ── Leaf-component roving tabindex (Plan 2c §1) ─────────────────────── */

const EDITABLE_RESOLVED: ResolvedSource = {
    sourceNode: { t: 'Para', c: [] } as any,
    reachabilityClass: 'TopLevel',
    sourceEntry: { t: 0, r: [0, 1], d: 0 },
};

function renderLeaf(node: any, Comp: (args: any) => ReactNode) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        resolveSource: () => EDITABLE_RESOLVED,
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <Comp node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>,
    );
}

describe('leaf components — roving tabindex', () => {
    it('Para sets tabIndex=-1 on its editable element', () => {
        const { container } = renderLeaf({ t: 'Para', c: [], s: 7 }, Para);
        const p = container.querySelector('p[data-block-pool-id]');
        expect(p).not.toBeNull();
        expect(p!.getAttribute('tabindex')).toBe('-1');
    });

    it('Header sets tabIndex=-1 on its editable element', () => {
        const node = { t: 'Header', c: [2, ['', [], []], []], s: 7 };
        const { container } = renderLeaf(node, Header);
        const h = container.querySelector('[data-block-pool-id]');
        expect(h).not.toBeNull();
        expect(h!.getAttribute('tabindex')).toBe('-1');
    });
});

/* ── Touch OS gesture suppression (Plan 2c §2) ──────────────────────── */

describe('useBlockEditHover — touch gesture suppression', () => {
    it('injects -webkit-touch-callout:none and touch-action:pan-y on blocks', () => {
        const { container } = mountHost();
        const style = container.querySelector('style');
        expect(style).not.toBeNull();
        const css = style!.textContent ?? '';
        expect(css).toContain('-webkit-touch-callout: none');
        // pan-y (not none): allows vertical document scroll while suppressing
        // pinch-zoom / horizontal pan during the hold window.
        expect(css).toContain('touch-action: pan-y');
    });

    describe('onContextMenu', () => {
        beforeEach(() => {
            // touch pointerdown calls setPointerCapture (absent in jsdom).
            Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
                value: vi.fn(),
                writable: true,
                configurable: true,
            });
        });
        afterEach(() => {
            delete (HTMLElement.prototype as any).setPointerCapture;
        });

        it('suppresses the context menu after a touch pointerdown', () => {
            const { getByTestId } = mountHost();
            const host = getByTestId('host');
            const block = getByTestId('block5');
            // Prime lastPointerTypeRef = 'touch'.
            fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 5, clientY: 5 }));
            // dispatchEvent returns false when a handler called preventDefault.
            const notCancelled = fireEvent.contextMenu(host);
            expect(notCancelled).toBe(false);
        });

        it('does NOT suppress the context menu after a mouse pointerdown', () => {
            const { getByTestId } = mountHost();
            const host = getByTestId('host');
            const block = getByTestId('block5');
            fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            const notCancelled = fireEvent.contextMenu(host);
            expect(notCancelled).toBe(true);
        });
    });
});

/* ── Global editingDisabled (bd-ov4gqk3m) ───────────────────────────── */

describe('useBlockEditHover — editingDisabled', () => {
    // Defense in depth: even if a block somehow carries
    // data-block-pool-id (here: hardcoded in the test host), a disabled
    // context must make the hook completely inert — no activation, no
    // hover outline, no affordance stylesheet.
    it('does NOT activate on mouse click when editing is disabled', () => {
        const { getByTestId, setEditTarget } = mountHost({ editingDisabled: true });
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).not.toHaveBeenCalled();
    });

    it('does NOT apply a hover outline when editing is disabled', () => {
        const { getByTestId } = mountHost({ editingDisabled: true });
        const block = getByTestId('block5');

        fireEvent(block, ptrEvent('pointermove', { pointerType: 'mouse' }));

        expect(block.style.boxShadow).toBe('');
    });

    it('does NOT inject the affordance stylesheet when editing is disabled', () => {
        const { container } = mountHost({ editingDisabled: true });
        expect(container.querySelector('style')).toBeNull();
    });

    it('still injects the stylesheet when editing is enabled (control)', () => {
        const { container } = mountHost();
        expect(container.querySelector('style')).not.toBeNull();
    });
});

/* ── Active-region guard (Phase 1 bug fix) ──────────────────────────── */
//
// Bug: clicking inside an already-open editor walks up past the affordance-
// less wrapper div (which has no data-block-pool-id) and activates the
// parent/grandparent block — "climbing" the user out of the block they're
// editing.
//
// Fix: a tracked ref (`activeEditRegionRef`) on the inner wrapper div lets
// onPointerUp detect that the click landed inside the open editor and
// suppress the spurious activation. The textarea keeps focus → caret-move.

/**
 * Inner component that calls useBlockEditHover() so it runs INSIDE the
 * PreviewContext.Provider. The wrapper div ref is threaded in from the
 * parent so the provider's activeEditRegionRef can be set after the
 * first render.
 *
 * Structure simulates an active edit session with the PARENT-CLIMB BUG:
 * - A grandparent div (poolId=99) wraps the entire editing region.
 * - Inside it: the measure-and-set wrapper div (NO pool-id) containing a textarea.
 * - A sibling block (poolId=6) is adjacent, for cross-surface click tests.
 *
 * Without the fix, clicking inside the textarea causes `closest('[data-block-pool-id]')`
 * to walk UP past the affordance-less wrapper and find the grandparent (poolId=99),
 * triggering a spurious activation → the bug.
 *
 * With the fix, the activeEditRegionRef guard detects the click is inside the
 * open editor region and suppresses activation entirely.
 */
function ActiveEditorInner({
    wrapperRef,
}: {
    wrapperRef: React.MutableRefObject<HTMLDivElement | null>;
}) {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            {/* Grandparent block — this is where the bug "climbs" to without the fix */}
            <div data-block-pool-id="99" data-testid="grandparent">
                {/* Simulates the measure-and-set wrapper: has NO data-block-pool-id */}
                <div ref={wrapperRef} data-testid="edit-wrapper">
                    <textarea data-testid="textarea" />
                </div>
            </div>
            {/* A sibling block outside the active editor */}
            <p data-block-pool-id="6" data-testid="block6">block 6</p>
        </div>
    );
}

// Pool for the active-editor climb fixture. The grandparent block (the element
// the parent-climb bug would walk UP to) carries data-block-pool-id="99", so its
// pool entry MUST exist for the climb to be observable: without an entry at index
// 99, activate() bails inside captureEditTarget (missing pool entry) and never
// calls setEditTarget — making the "not called" assertion pass *regardless* of the
// guard (green-on-revert). With a real Original entry here (anchorR0 = 700,
// distinct from the active target's 100 so the dedup guard does not short-circuit
// it), removing the onPointerUp active-region guard causes activate(grandparent)
// to fire setEditTarget(700) → the assertion goes red. This makes the test a true
// fail-on-revert sentinel for the Phase-1 guard. (bd: fail-on-revert audit 2026-06-13)
const POOL_ACTIVE: unknown[] = (() => {
    const p: unknown[] = [...POOL_WITH_5];
    p[99] = { t: 0, r: [700, 800] as [number, number], d: 0 };
    return p;
})();

function mountActiveEditor() {
    const setEditTarget = vi.fn();

    function Wrapper() {
        const wrapperRef = useRef<HTMLDivElement | null>(null);
        const activeEditRegionRef = wrapperRef; // same ref object — both the context and the inner div's ref attachment point to it
        const ctx: PreviewContextValue = {
            currentFilePath: '/project/test.qmd',
            setEditTarget,
            // P2.3a: editTarget now uses anchorR0/anchorR1/anchorSlice instead of poolId.
            editTarget: { anchorR0: 100, anchorR1: 200, anchorSlice: '', contentHeight: 40, boxStyle: {} },
            pool: POOL_ACTIVE,
            content: '',
            activeEditRegionRef,
        };
        return (
            <PreviewContext.Provider value={ctx}>
                <ActiveEditorInner wrapperRef={wrapperRef} />
            </PreviewContext.Provider>
        );
    }

    const utils = render(<Wrapper />);
    return { ...utils, setEditTarget };
}

describe('useBlockEditHover — active-region guard (Phase 1 fix)', () => {
    it('does NOT call setEditTarget when clicking inside the active edit region (no parent climb)', () => {
        const { getByTestId, setEditTarget } = mountActiveEditor();
        const textarea = getByTestId('textarea');

        // Click inside the open editor (inside the wrapper div / textarea).
        fireEvent(textarea, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(textarea, ptrEvent('pointerup', { pointerType: 'mouse' }));

        // Must NOT activate anything — no climb to parent/grandparent.
        expect(setEditTarget).not.toHaveBeenCalled();
    });

    it('DOES switch to a different surface when clicking outside the active edit region', () => {
        const { getByTestId, setEditTarget } = mountActiveEditor();
        const block6 = getByTestId('block6');
        vi.spyOn(block6, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // Click on a different block outside the open editor.
        fireEvent(block6, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block6, ptrEvent('pointerup', { pointerType: 'mouse' }));

        // Should switch to the new block (activate block6).
        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for block6 = pool[6].r[0] = 300
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(300);
    });

    it('still activates on a single click with no editor open (regression guard)', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for block5 = pool[5].r[0] = 100
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(100);
    });
});

/* ── P2.2: Locked-tile resolution wired into activate ───────────────── */
//
// After P2.2, activate() routes through resolveLockedTile() before reading
// poolId and measuring the element. The locked tile — not the raw leaf —
// is what gets activated.

/**
 * A chrome-less single-child div wrapping one <p>. Both elements carry
 * data-block-pool-id. The div and p have identical mocked rects → the div
 * is the locked tile. Clicking the <p> should activate the div's poolId.
 */
function ChromelessWrapperInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            {/* Outer chrome-less wrapper: poolId 10 */}
            <div data-block-pool-id="10" tabIndex={-1} data-testid="wrapper">
                {/* Inner leaf: poolId 11; rects will be mocked to match the wrapper */}
                <p data-block-pool-id="11" tabIndex={-1} data-testid="leaf">text</p>
            </div>
        </div>
    );
}

// P2.3a: pool entries for chromeless wrapper (poolId 10 and 11).
// anchorR0: wrapper(10) → 500, leaf(11) → 600.
const POOL_CHROMELESS: unknown[] = [
    ...Array.from({ length: 10 }, () => null),
    { t: 0, r: [500, 510] as [number, number], d: 0 }, // index 10 = wrapper
    { t: 0, r: [600, 610] as [number, number], d: 0 }, // index 11 = leaf
];

function mountChromelessWrapper() {
    const setEditTarget = vi.fn();
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
        pool: POOL_CHROMELESS,
        content: '',
    };
    const utils = render(
        <PreviewContext.Provider value={ctx}>
            <ChromelessWrapperInner />
        </PreviewContext.Provider>,
    );
    return { ...utils, setEditTarget };
}

describe('useBlockEditHover — P2.2 locked-tile resolution in activate', () => {
    it('clicking inside a chrome-less single-child div activates the wrapper (not the leaf)', () => {
        const { getByTestId, setEditTarget } = mountChromelessWrapper();
        const wrapper = getByTestId('wrapper');
        const leaf = getByTestId('leaf');
        // Both wrapper and leaf have identical rects → coincident → wrapper is the tile.
        const sharedRect: DOMRect = {
            width: 200, height: 40, top: 100, bottom: 140,
            left: 0, right: 200, x: 0, y: 100, toJSON: () => ({}),
        };
        vi.spyOn(wrapper, 'getBoundingClientRect').mockReturnValue(sharedRect);
        vi.spyOn(leaf, 'getBoundingClientRect').mockReturnValue(sharedRect);

        fireEvent(leaf, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(leaf, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: the wrapper (pool[10]) is the tile; anchorR0 = 500.
        // Not the leaf (pool[11], anchorR0 = 600).
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(500);
    });

    it('a lone <p> with no container (leaf === tile) activates its own block (regression)', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for block5 = pool[5].r[0] = 100
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(100);
    });
});

/* ── P2.2: Roving-tabindex alignment (enumerateLockedTiles) ──────────── */
//
// After P2.2, ArrowDown/ArrowUp navigate the locked-tile list from
// enumerateLockedTiles(), not the raw querySelectorAll result. This means:
//  - Coincident lone-child wrappers dedupe their child away.
//  - Hidden tiles (zero-area rect) are skipped automatically.
//  - Enter on the focused tile opens that tile (idempotent via resolveLockedTile).

/**
 * Two flat <p> blocks (tiles) with a hidden <p> in between.
 * After P2.2, ArrowDown should skip the hidden one.
 */
function HiddenTileInner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <p data-block-pool-id="1" tabIndex={-1} data-testid="t1">block 1</p>
            {/* This one will be mocked to zero-area (collapsed) */}
            <p data-block-pool-id="2" tabIndex={-1} data-testid="t2-hidden">collapsed</p>
            <p data-block-pool-id="3" tabIndex={-1} data-testid="t3">block 3</p>
        </div>
    );
}

// P2.3a: pool entries for hidden-tile test blocks 1, 2, 3.
// anchorR0 values: t1 → 10, t2hidden → 20, t3 → 30.
const POOL_HIDDEN: unknown[] = [
    null, // index 0 unused
    { t: 0, r: [10, 15] as [number, number], d: 0 }, // t1
    { t: 0, r: [20, 25] as [number, number], d: 0 }, // t2hidden
    { t: 0, r: [30, 35] as [number, number], d: 0 }, // t3
];

function mountHiddenTile() {
    const setEditTarget = vi.fn();
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
        pool: POOL_HIDDEN,
        content: '',
    };
    const utils = render(
        <PreviewContext.Provider value={ctx}>
            <HiddenTileInner />
        </PreviewContext.Provider>,
    );
    return { ...utils, setEditTarget };
}

describe('useBlockEditHover — P2.2 roving-tabindex alignment', () => {
    it('ArrowDown/ArrowUp navigate locked tiles, skipping hidden tiles', () => {
        const { getByTestId } = mountHiddenTile();
        const host = getByTestId('host');
        const t1 = getByTestId('t1');
        const t2hidden = getByTestId('t2-hidden');
        const t3 = getByTestId('t3');
        // t1 and t3 are visible; t2 is zero-rect (hidden).
        const visRect: DOMRect = {
            width: 200, height: 20, top: 0, bottom: 20,
            left: 0, right: 200, x: 0, y: 0, toJSON: () => ({}),
        };
        const zeroRect: DOMRect = {
            width: 0, height: 0, top: 0, bottom: 0,
            left: 0, right: 0, x: 0, y: 0, toJSON: () => ({}),
        };
        vi.spyOn(t1, 'getBoundingClientRect').mockReturnValue(visRect);
        vi.spyOn(t2hidden, 'getBoundingClientRect').mockReturnValue(zeroRect);
        vi.spyOn(t3, 'getBoundingClientRect').mockReturnValue(visRect);

        // ArrowDown from no selection → first tile (t1), then t3 (skipping t2).
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(t1);

        fireEvent.keyDown(host, { key: 'ArrowDown' });
        // Should skip t2-hidden and land on t3.
        expect(document.activeElement).toBe(t3);
    });

    it('Enter on the focused locked tile activates that same tile (idempotent)', () => {
        const { getByTestId, setEditTarget } = mountHiddenTile();
        const host = getByTestId('host');
        const t1 = getByTestId('t1');
        const t2hidden = getByTestId('t2-hidden');
        const t3 = getByTestId('t3');
        const visRect: DOMRect = {
            width: 200, height: 20, top: 0, bottom: 20,
            left: 0, right: 200, x: 0, y: 0, toJSON: () => ({}),
        };
        const zeroRect: DOMRect = {
            width: 0, height: 0, top: 0, bottom: 0,
            left: 0, right: 0, x: 0, y: 0, toJSON: () => ({}),
        };
        vi.spyOn(t1, 'getBoundingClientRect').mockReturnValue(visRect);
        vi.spyOn(t2hidden, 'getBoundingClientRect').mockReturnValue(zeroRect);
        vi.spyOn(t3, 'getBoundingClientRect').mockReturnValue(visRect);

        // Navigate to t1 via ArrowDown.
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(t1);

        // Press Enter — should activate t1 (the focused tile, idempotent resolve).
        fireEvent.keyDown(host, { key: 'Enter' });
        expect(setEditTarget).toHaveBeenCalledOnce();
        // P2.3a: anchorR0 for t1 = pool[1].r[0] = 10
        expect(setEditTarget.mock.calls[0][0].anchorR0).toBe(10);
    });
});
