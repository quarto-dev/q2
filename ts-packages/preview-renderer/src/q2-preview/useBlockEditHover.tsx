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
    // Last observed pointer type — read in onContextMenu to suppress the OS
    // long-press menu for touch while preserving mouse right-click. `contextmenu`
    // fires as a MouseEvent with no `pointerType`, so we cannot read it off the
    // event itself; we track it from the preceding pointerdown.
    const lastPointerTypeRef = useRef<string>('mouse');

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
        const id: string | number = /^\d+$/.test(poolId) ? Number(poolId) : poolId;
        // Dedup: if this block is already the active edit target, do nothing.
        if (ctx?.editTarget?.poolId === id) return;
        const rect = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        const contentHeight = rect.height
            - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom)
            - parseFloat(cs.borderTopWidth) - parseFloat(cs.borderBottomWidth);
        // Full computed box (margin + padding + per-side border) so the
        // measure-and-set wrapper reproduces the element's exact box — keeping
        // vertical spacing and visible decorations (e.g. an h2's Bootstrap
        // border-bottom rule) intact while editing.
        const boxStyle: Record<string, string> = {
            marginTop: cs.marginTop, marginRight: cs.marginRight,
            marginBottom: cs.marginBottom, marginLeft: cs.marginLeft,
            paddingTop: cs.paddingTop, paddingRight: cs.paddingRight,
            paddingBottom: cs.paddingBottom, paddingLeft: cs.paddingLeft,
            borderTopWidth: cs.borderTopWidth, borderRightWidth: cs.borderRightWidth,
            borderBottomWidth: cs.borderBottomWidth, borderLeftWidth: cs.borderLeftWidth,
            borderTopStyle: cs.borderTopStyle, borderRightStyle: cs.borderRightStyle,
            borderBottomStyle: cs.borderBottomStyle, borderLeftStyle: cs.borderLeftStyle,
            borderTopColor: cs.borderTopColor, borderRightColor: cs.borderRightColor,
            borderBottomColor: cs.borderBottomColor, borderLeftColor: cs.borderLeftColor,
        };
        outlineElement(null);
        ctx.setEditTarget({ poolId: id, contentHeight, boxStyle });
    }, [ctx]);

    const findEditTarget = (e: React.PointerEvent<HTMLElement> | React.MouseEvent<HTMLElement>) => {
        const target = e.target as Element;
        return target.closest('[data-block-pool-id]');
    };

    const onPointerMove = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (ctx?.editTarget != null) return;
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
        // First line, BEFORE the editing guard: keep the pointer-type record
        // fresh even when a pointerdown lands during an active edit, so a later
        // right-click is not mis-classified as touch.
        lastPointerTypeRef.current = e.pointerType;
        if (ctx?.editTarget != null) return;
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
        if (ctx?.editTarget != null) return;
        // Roving-tabindex navigation: arrows move focus through the
        // [data-block-pool-id] elements in DOM pre-order; hoveredRef is
        // updated in lock-step so the existing Enter/Space handler (which
        // reads hoveredRef) activates whichever block keyboard focus landed on.
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault();
            const host = e.currentTarget;
            const blocks = Array.from(
                host.querySelectorAll<HTMLElement>('[data-block-pool-id]'),
            );
            if (!blocks.length) return;
            const idx = blocks.indexOf(document.activeElement as HTMLElement);
            const next = e.key === 'ArrowDown'
                ? blocks[(idx + 1) % blocks.length]
                : blocks[(idx - 1 + blocks.length) % blocks.length];
            next.focus();
            hoveredRef.current = next;
            return;
        }
        if (e.key === 'Escape' && ctx?.setEditTarget) {
            ctx.setEditTarget(null);
            outlineElement(null);
        }
        if ((e.key === 'Enter' || e.key === ' ') && hoveredRef.current) {
            e.preventDefault();
            activate(hoveredRef.current);
        }
    }, [activate, ctx]);

    // Global read-only mode (bd-ov4gqk3m): return an inert surface — no
    // handlers, no affordance CSS. Placed after all hook calls so the
    // hook order stays stable if the flag ever changes between renders.
    if (ctx?.editingDisabled) {
        return { hostProps: {}, stylesheet: null };
    }

    const hostProps: React.HTMLAttributes<HTMLElement> = {
        onPointerMove,
        onPointerDown,
        onPointerUp,
        onPointerLeave,
        onKeyDown,
        // Suppress the OS context menu for touch long-press (iOS/Android),
        // which would otherwise overlap the activated textarea. Mouse
        // right-click is preserved.
        onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
            if (lastPointerTypeRef.current !== 'mouse') e.preventDefault();
        },
        // Roving tabindex: the host is the single Tab stop; arrows move a
        // programmatic focus through the (tabIndex={-1}) block elements.
        tabIndex: 0,
        role: 'region',
        'aria-label': 'Editable preview',
        'aria-describedby': 'q2-edit-hint',
    };

    const stylesheet = (
        <>
            <style>{`
                [data-block-pool-id] {
                    cursor: pointer;
                    -webkit-touch-callout: none; /* suppress iOS long-press callout */
                    touch-action: pan-y;         /* allow vertical scroll; suppress pinch-zoom/horizontal pan during hold */
                }
                [data-block-pool-id]:focus-visible { outline: 2px solid rgba(59, 130, 246, 0.8); }
            `}</style>
            {/* Visually-hidden hint announced once when the host first
                receives focus (referenced by aria-describedby). */}
            <span
                id="q2-edit-hint"
                style={{
                    position: 'absolute',
                    width: 1,
                    height: 1,
                    padding: 0,
                    margin: -1,
                    overflow: 'hidden',
                    clip: 'rect(0, 0, 0, 0)',
                    whiteSpace: 'nowrap',
                    border: 0,
                }}
            >
                Use arrow keys to navigate blocks; press Enter or Space to edit
            </span>
        </>
    );

    return { hostProps, stylesheet };
}
