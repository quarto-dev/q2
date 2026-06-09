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
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { useBlockEditHover } from './useBlockEditHover';

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

function Inner() {
    const { hostProps } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            <p data-block-pool-id="5" data-testid="block5">block 5</p>
        </div>
    );
}

function BlockHost({ setEditTarget }: { setEditTarget: (t: any) => void }) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
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

function mountHost() {
    const setEditTarget = vi.fn();
    const utils = render(<BlockHost setEditTarget={setEditTarget} />);
    return { ...utils, setEditTarget };
}

/* ── Mouse activation ───────────────────────────────────────────────── */

describe('useBlockEditHover — mouse activation', () => {
    it('calls setEditTarget with numeric poolId and rect on mouse pointerup', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse' }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        const arg = setEditTarget.mock.calls[0][0];
        // string attr '5' is parsed to number 5 via /^\d+$/.test
        expect(arg.poolId).toBe(5);
        expect(arg.rect).toBe(MOCK_RECT);
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
        expect(arg.poolId).toBe(5);
        expect(arg.rect).toBe(MOCK_RECT);
    });

    it('calls setEditTarget(null) on Escape', () => {
        const { getByTestId, setEditTarget } = mountHost();
        const host = getByTestId('host');

        fireEvent.keyDown(host, { key: 'Escape' });

        expect(setEditTarget).toHaveBeenCalledWith(null);
    });
});
