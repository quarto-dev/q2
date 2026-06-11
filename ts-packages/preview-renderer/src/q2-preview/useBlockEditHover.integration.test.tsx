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
        expect(arg.poolId).toBe(5);
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

function mountMulti() {
    const setEditTarget = vi.fn();
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        setEditTarget,
    };
    const utils = render(
        <PreviewContext.Provider value={ctx}>
            <MultiInner />
        </PreviewContext.Provider>,
    );
    return { ...utils, setEditTarget };
}

describe('useBlockEditHover — keyboard arrow navigation', () => {
    it('ArrowDown moves focus to the next block; ArrowUp to the previous', () => {
        const { getByTestId } = mountMulti();
        const host = getByTestId('host');
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(getByTestId('block1'));
        fireEvent.keyDown(host, { key: 'ArrowDown' });
        expect(document.activeElement).toBe(getByTestId('block2'));
        fireEvent.keyDown(host, { key: 'ArrowUp' });
        expect(document.activeElement).toBe(getByTestId('block1'));
    });

    it('ArrowDown on the last block wraps to first; ArrowUp on first wraps to last', () => {
        const { getByTestId } = mountMulti();
        const host = getByTestId('host');
        const b1 = getByTestId('block1');
        const b3 = getByTestId('block3');
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
        const b2 = getByTestId('block2');
        vi.spyOn(b2, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);
        fireEvent.keyDown(host, { key: 'ArrowDown' }); // -> block1
        fireEvent.keyDown(host, { key: 'ArrowDown' }); // -> block2
        fireEvent.keyDown(host, { key: 'Enter' });
        expect(setEditTarget).toHaveBeenCalledOnce();
        expect(setEditTarget.mock.calls[0][0].poolId).toBe(2);
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
        expect(setEditTarget.mock.calls[0][0].poolId).toBe(5);
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
