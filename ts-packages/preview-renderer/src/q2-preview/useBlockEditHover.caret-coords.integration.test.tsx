/**
 * Caret-at-click / drag-selection capture wiring (bd-q9lyghv2, bd-abo9m23f).
 *
 * When a mouse click activates a block for rich-text editing, the opening
 * pointer state must be classified and stashed on `pendingOpenSelectionRef`:
 *  - plain click → `{ kind: 'caret', head }` (viewport coords of the click);
 *  - drag selection contained in the activated block → `{ kind: 'range',
 *    anchor, head }` (direction-aware endpoint coords);
 *  - drag selection NOT contained in the block → activation is SUPPRESSED
 *    entirely (the user's selection survives for copying).
 * Keyboard and touch activation must NOT set a payload (end-of-block), and
 * are never suppressed by a live selection.
 *
 * jsdom note: PointerEvent does not honour pointerType/clientX/clientY from the
 * constructor init dict — force them via Object.defineProperty (see ptrEvent).
 * jsdom has no layout: endpoint rects come from a mocked
 * Range.prototype.getClientRects (x = startOffset * 10, line y 100..120).
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import React, { useRef } from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import type { PendingOpenSelection } from './dragSelectionCapture';
import { useBlockEditHover } from './useBlockEditHover';

afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
});

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

const POOL_ENTRY_5 = { t: 0, r: [100, 200] as [number, number], d: 0 };
const POOL_ENTRY_6 = { t: 0, r: [210, 300] as [number, number], d: 0 };
const POOL: unknown[] = [
    ...Array.from({ length: 5 }, () => null),
    POOL_ENTRY_5,
    POOL_ENTRY_6,
];

const MOCK_RECT: DOMRect = {
    width: 200, height: 40, top: 100, bottom: 140,
    left: 0, right: 200, x: 0, y: 100, toJSON: () => ({}),
};

function Inner() {
    const { hostProps, stylesheet } = useBlockEditHover();
    return (
        <div {...hostProps} data-testid="host">
            {stylesheet}
            <p data-block-pool-id="5" tabIndex={-1} data-testid="block5">the quick brown fox</p>
            <p data-block-pool-id="6" tabIndex={-1} data-testid="block6">another paragraph here</p>
        </div>
    );
}

/** Mount the hover host, exposing the live pendingOpenSelectionRef to the test. */
function mountHost() {
    const setEditTarget = vi.fn();
    let openSelRef!: React.MutableRefObject<PendingOpenSelection | null>;
    function Host() {
        openSelRef = useRef<PendingOpenSelection | null>(null);
        const ctx: PreviewContextValue = {
            currentFilePath: '/project/test.qmd',
            setEditTarget,
            pool: POOL,
            content: '',
            pendingOpenSelectionRef: openSelRef,
        };
        return (
            <PreviewContext.Provider value={ctx}>
                <Inner />
            </PreviewContext.Provider>
        );
    }
    const utils = render(<Host />);
    return { ...utils, setEditTarget, getPayload: () => openSelRef.current };
}

/** Install a fake window.getSelection over real DOM nodes, plus synthetic
 *  endpoint rects (x = startOffset * 10, line y 100..120). */
function stubSelection(
    anchorNode: Node, anchorOffset: number,
    focusNode: Node, focusOffset: number,
) {
    const backward =
        anchorNode === focusNode
            ? focusOffset < anchorOffset
            : !!(anchorNode.compareDocumentPosition(focusNode) & Node.DOCUMENT_POSITION_PRECEDING);
    const [startContainer, endContainer] = backward
        ? [focusNode, anchorNode]
        : [anchorNode, focusNode];
    vi.spyOn(window, 'getSelection').mockReturnValue({
        isCollapsed: false,
        rangeCount: 1,
        anchorNode, anchorOffset, focusNode, focusOffset,
        getRangeAt: () => ({ startContainer, endContainer }),
    } as unknown as Selection);
    vi.spyOn(Range.prototype, 'getClientRects').mockImplementation(function (this: Range) {
        const rect = {
            left: this.startOffset * 10, right: this.startOffset * 10,
            top: 100, bottom: 120, height: 20, width: 0,
            x: this.startOffset * 10, y: 100, toJSON: () => ({}),
        } as DOMRect;
        return [rect] as unknown as DOMRectList;
    });
}

describe('useBlockEditHover — opening-selection capture', () => {
    it('stashes a caret payload with the click coords on plain mouse pointerup', () => {
        const { getByTestId, setEditTarget, getPayload } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse', clientX: 42, clientY: 117 }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse', clientX: 42, clientY: 117 }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        expect(getPayload()).toEqual({ kind: 'caret', head: { x: 42, y: 117 } });
    });

    it('stashes a direction-aware range payload for a drag contained in the block', () => {
        const { getByTestId, setEditTarget, getPayload } = mountHost();
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);
        const text = block.firstChild as Text;
        // Backward drag: anchor at offset 15, released (focus) at offset 4.
        stubSelection(text, 15, text, 4);

        fireEvent(block, ptrEvent('pointerdown', { pointerType: 'mouse', clientX: 150, clientY: 110 }));
        fireEvent(block, ptrEvent('pointerup', { pointerType: 'mouse', clientX: 40, clientY: 110 }));

        expect(setEditTarget).toHaveBeenCalledOnce();
        expect(getPayload()).toEqual({
            kind: 'range',
            anchor: { x: 150, y: 110 },
            head: { x: 40, y: 110 },
        });
    });

    it('suppresses activation entirely for a cross-block drag (selection preserved)', () => {
        const { getByTestId, setEditTarget, getPayload } = mountHost();
        const block5 = getByTestId('block5');
        const block6 = getByTestId('block6');
        vi.spyOn(block5, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);
        // Drag started in block 5, released over block 6 → activation target is
        // block 6, but the selection spans out of it.
        stubSelection(block5.firstChild as Text, 2, block6.firstChild as Text, 7);

        fireEvent(block6, ptrEvent('pointerdown', { pointerType: 'mouse', clientX: 70, clientY: 160 }));
        fireEvent(block6, ptrEvent('pointerup', { pointerType: 'mouse', clientX: 70, clientY: 160 }));

        expect(setEditTarget).not.toHaveBeenCalled();
        expect(getPayload()).toBeNull();
    });

    it('does NOT stash a payload on keyboard (Enter) activation', () => {
        const { getByTestId, setEditTarget, getPayload } = mountHost();
        const host = getByTestId('host');
        const block = getByTestId('block5');
        vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

        // Hover sets hoveredRef; Enter activates via the keyboard path (no coords).
        fireEvent(block, ptrEvent('pointermove', { pointerType: 'mouse' }));
        fireEvent.keyDown(host, { key: 'Enter' });

        expect(setEditTarget).toHaveBeenCalledOnce();
        expect(getPayload()).toBeNull();
    });

    it('keyboard activation is NOT suppressed by a live selection elsewhere', () => {
        const { getByTestId, setEditTarget, getPayload } = mountHost();
        const host = getByTestId('host');
        const block5 = getByTestId('block5');
        const block6 = getByTestId('block6');
        vi.spyOn(block5, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);
        // A selection lives in block 6; keyboard-activating block 5 must still work.
        stubSelection(block6.firstChild as Text, 0, block6.firstChild as Text, 7);

        fireEvent(block5, ptrEvent('pointermove', { pointerType: 'mouse' }));
        fireEvent.keyDown(host, { key: 'Enter' });

        expect(setEditTarget).toHaveBeenCalledOnce();
        expect(getPayload()).toBeNull();
    });

    it('does NOT stash a payload on touch hold activation', () => {
        vi.useFakeTimers();
        Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
            value: vi.fn(), writable: true, configurable: true,
        });
        try {
            const { getByTestId, setEditTarget, getPayload } = mountHost();
            const block = getByTestId('block5');
            vi.spyOn(block, 'getBoundingClientRect').mockReturnValue(MOCK_RECT);

            fireEvent(block, ptrEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 10 }));
            vi.advanceTimersByTime(500);

            expect(setEditTarget).toHaveBeenCalledOnce();
            expect(getPayload()).toBeNull();
        } finally {
            delete (HTMLElement.prototype as any).setPointerCapture;
        }
    });
});
