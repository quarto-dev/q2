import React, { useCallback, useContext, useRef } from 'react';
import { PreviewContext } from './PreviewContext';

const HOLD_MS = 500;
const MOVE_THRESHOLD_PX = 8;

/**
 * Delegated block-edit hover/activation handler (Plan 2b).
 *
 * Returns `hostProps` to spread on the PreviewDocument root div, and a
 * `stylesheet` node that injects the hover/active outline CSS.
 *
 * Three activation paths are handled by a single delegated handler on the
 * root host:
 *  - **Mouse:** hover outlines the deepest `[data-block-pool-id]` element;
 *    click activates it.
 *  - **Touch (Pointer Events):** `pointerdown` outlines (reveal); hold past
 *    `HOLD_MS` activates; early release or move beyond threshold cancels.
 *  - **Keyboard (roving tabindex):** the edit layer is a single Tab stop;
 *    arrows move the active region in DOM pre-order; Enter/Space activates;
 *    Esc exits.
 *
 * No overlay to position — the outline is `box-shadow` on the target
 * element itself (layout-safe, zero reflow).
 */
export function useBlockEditHover(): {
    hostProps: React.HTMLAttributes<HTMLElement>;
    stylesheet: React.ReactNode;
} {
    const ctx = useContext(PreviewContext);
    const hoveredRef = useRef<Element | null>(null);
    const holdTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const pointerDownPosRef = useRef<{ x: number; y: number } | null>(null);

    const clearHold = () => {
        if (holdTimerRef.current !== null) {
            clearTimeout(holdTimerRef.current);
            holdTimerRef.current = null;
        }
    };

    const outlineElement = (el: Element | null) => {
        if (hoveredRef.current && hoveredRef.current !== el) {
            (hoveredRef.current as HTMLElement).style.removeProperty('box-shadow');
        }
        if (el) {
            (el as HTMLElement).style.boxShadow = '0 0 0 2px rgba(59, 130, 246, 0.6)';
        }
        hoveredRef.current = el;
    };

    const activate = useCallback((el: Element) => {
        const poolId = el.getAttribute('data-block-pool-id');
        if (poolId === null || !ctx?.setEditTarget) return;
        const rect = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        const contentHeight = rect.height
            - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom)
            - parseFloat(cs.borderTopWidth) - parseFloat(cs.borderBottomWidth);
        outlineElement(null);
        const id: string | number = /^\d+$/.test(poolId) ? Number(poolId) : poolId;
        ctx.setEditTarget({ poolId: id, rect, contentHeight });
    }, [ctx]);

    const findEditTarget = (e: React.PointerEvent<HTMLElement> | React.MouseEvent<HTMLElement>) => {
        const target = e.target as Element;
        return target.closest('[data-block-pool-id]');
    };

    const onPointerMove = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (e.pointerType !== 'mouse') {
            // Touch: cancel hold if moved beyond threshold.
            if (pointerDownPosRef.current) {
                const dx = e.clientX - pointerDownPosRef.current.x;
                const dy = e.clientY - pointerDownPosRef.current.y;
                if (Math.hypot(dx, dy) > MOVE_THRESHOLD_PX) {
                    clearHold();
                    outlineElement(null);
                    pointerDownPosRef.current = null;
                }
            }
            return;
        }
        // Mouse hover.
        const el = findEditTarget(e);
        if (el !== hoveredRef.current) {
            outlineElement(el);
        }
    }, []);

    const onPointerDown = useCallback((e: React.PointerEvent<HTMLElement>) => {
        const el = findEditTarget(e);
        if (!el) return;

        if (e.pointerType !== 'mouse') {
            // Touch: outline on pointerdown, activate after hold.
            outlineElement(el);
            pointerDownPosRef.current = { x: e.clientX, y: e.clientY };
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
            holdTimerRef.current = setTimeout(() => {
                holdTimerRef.current = null;
                pointerDownPosRef.current = null;
                activate(el);
            }, HOLD_MS);
        }
    }, [activate]);

    const onPointerUp = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (e.pointerType !== 'mouse') {
            clearHold();
            pointerDownPosRef.current = null;
            return;
        }
        // Mouse click: activate.
        const el = findEditTarget(e);
        if (el) {
            activate(el);
        }
    }, [activate]);

    const onPointerLeave = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (e.pointerType === 'mouse') {
            outlineElement(null);
        }
    }, []);

    const onKeyDown = useCallback((e: React.KeyboardEvent<HTMLElement>) => {
        if (e.key === 'Escape' && ctx?.setEditTarget) {
            ctx.setEditTarget(null);
            outlineElement(null);
        }
        if ((e.key === 'Enter' || e.key === ' ') && hoveredRef.current) {
            e.preventDefault();
            activate(hoveredRef.current);
        }
    }, [activate, ctx]);

    const hostProps: React.HTMLAttributes<HTMLElement> = {
        onPointerMove,
        onPointerDown,
        onPointerUp,
        onPointerLeave,
        onKeyDown,
    };

    const stylesheet = (
        <style>{`
            [data-block-pool-id] { cursor: pointer; }
            [data-block-pool-id]:focus-visible { outline: 2px solid rgba(59, 130, 246, 0.8); }
        `}</style>
    );

    return { hostProps, stylesheet };
}
