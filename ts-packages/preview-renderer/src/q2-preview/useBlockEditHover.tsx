import React, { useCallback, useContext, useRef } from 'react';
import { PreviewContext } from './PreviewContext';
import { resolveOuterBlock, enumerateOuterBlocks, enumerateNestingSurfaces, captureEditTarget, measureBlockBox, seedForRange } from './outerBlocks';

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
    // §3: `hoveredRef` is the RESOLVED element that carries the box-shadow and is
    // the Enter/Space activation target (mode-aware: the leaf in unlock mode, the
    // outer block in locked mode). `rawLeafRef` is the raw `closest(pool-id)` —
    // used ONLY as the cheap pointermove dedupe key. Splitting the two lets the
    // outline promise exactly the granularity a click will deliver.
    const hoveredRef = useRef<Element | null>(null);
    const rawLeafRef = useRef<Element | null>(null);
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
        // P3.3: mode branch — unlocked uses the leaf (deepest pool-id),
        // locked uses resolveOuterBlock (collapses to outermost prefixing container).
        const outerBlock = ctx?.unlockNestingCursor
            ? el.closest('[data-block-pool-id]')      // leaf: deepest pool-id, no collapse
            : resolveOuterBlock(el);
        if (!outerBlock) return;
        if (!ctx?.setEditTarget) return;

        // P2.4b: use captureEditTarget for the identity triple (anchorR0/R1/anchorSlice).
        // This is the canonical extraction site; the nav reland path uses the same helper.
        const content = ctx?.content ?? '';
        const identity = captureEditTarget(outerBlock, ctx?.pool ?? [], content);
        if (!identity) return;

        const { anchorR0, anchorR1, anchorSlice } = identity;

        // Dedup: if this block is already the active edit target (same byte range), do nothing.
        if (ctx?.editTarget?.anchorR0 === anchorR0) return;

        // P2.4b: use measureBlockBox (shared with the move-open paths in entry.tsx)
        // so click-activation and move-opened editors both receive real box geometry.
        const { contentHeight, boxStyle } = measureBlockBox(outerBlock);
        outlineElement(null);

        // P3.3: buffer-aware draft seeding (shared with the move/reland/nest-retarget
        // sites via the pure seedForRange helper). nestedEditBuffers[siKey] holds the
        // clean QMD buffer (no '> '/indent prefix); falls back to the raw anchorSlice.
        const { seededDraft } = seedForRange({ r0: anchorR0, r1: anchorR1 }, content, ctx?.nestedEditBuffers);

        // §1: capture the opened block's subtree geometry BEFORE setEditTarget swaps
        // the children to a textarea (a post-open effect would be too late). No-op in
        // locked mode or for a flat block.
        ctx.captureGeometry?.(outerBlock, { r0: anchorR0, r1: anchorR1 });

        // Seed the draft ref BEFORE calling setEditTarget (the fresh-open site).
        // This is the single canonical place where the draft is seeded — setEditTarget
        // itself no longer reseeds, so a P2.3b self-heal re-anchor via setEditTarget
        // can preserve the in-flight draft without clobbering it.
        if (ctx.editDraftRef) ctx.editDraftRef.current = seededDraft;
        ctx.setEditTarget({
            anchorR0,
            anchorR1,
            anchorSlice,
            contentHeight,
            boxStyle,
            seededDraft,
            leafAnchorR0: anchorR0,
        });
    }, [ctx]);


    const findEditTarget = (e: React.PointerEvent<HTMLElement> | React.MouseEvent<HTMLElement>) => {
        const target = e.target as Element;
        return target.closest('[data-block-pool-id]');
    };

    const onPointerMove = useCallback((e: React.PointerEvent<HTMLElement>) => {
        // §3 latest-ref guard (Reflection #11/#12): this handler is useCallback(…,[])
        // so `ctx` is frozen at render 0; read the LIVE edit state / mode off the
        // stable refs, never the per-render scalars (which would be stale).
        if (ctx?.editTargetRef?.current != null) return;
        if (e.pointerType !== 'mouse') {
            // Touch: cancel hold if moved beyond threshold.
            if (pointerDownPosRef.current) {
                const dx = e.clientX - pointerDownPosRef.current.x;
                const dy = e.clientY - pointerDownPosRef.current.y;
                if (Math.hypot(dx, dy) > MOVE_THRESHOLD_PX) {
                    clearHold();
                    outlineElement(null);
                    rawLeafRef.current = null;
                    pointerDownPosRef.current = null;
                }
            }
            return;
        }
        // Mouse hover. Dedupe on the RAW leaf so pointermove stays cheap; only
        // re-resolve + re-outline when the leaf under the pointer changes.
        const rawLeaf = findEditTarget(e);
        if (rawLeaf === rawLeafRef.current) return;
        rawLeafRef.current = rawLeaf;
        // §3 mode-aware outline: highlight the surface a click would ACTIVATE —
        // the leaf in unlock mode, the outer block (resolveOuterBlock) in locked.
        const resolved = !rawLeaf
            ? null
            : ctx?.unlockNestingCursorRef?.current
                ? rawLeaf
                : resolveOuterBlock(rawLeaf);
        outlineElement(resolved);
        hoveredRef.current = resolved;
    }, []);

    const onPointerDown = useCallback((e: React.PointerEvent<HTMLElement>) => {
        // First line, BEFORE the editing guard: keep the pointer-type record
        // fresh even when a pointerdown lands during an active edit, so a later
        // right-click is not mis-classified as touch.
        lastPointerTypeRef.current = e.pointerType;

        if (ctx?.editTarget != null) {
            // P2.4d: mouse click while editing — classify the target.
            // Touch path is NOT a click-switch (touch uses hold-to-edit, handled below).
            if (e.pointerType === 'mouse') {
                const target = e.target as Element;
                // Active-region guard: click inside the open editor → caret-move, not switch.
                if (ctx.activeEditRegionRef?.current?.contains(target)) {
                    return; // P1 caret-move path; no click-switch
                }
                // Resolve the outer block the click landed on.
                const el = findEditTarget(e);
                if (el) {
                    // P3.3: mode-aware outer block resolution (same as activate() does).
                    const clickedEl = ctx.unlockNestingCursor
                        ? (el.closest('[data-block-pool-id]') ?? el)
                        : (resolveOuterBlock(el) ?? el);
                    // Check if this tile is different from the current edit target.
                    const pidAttr = clickedEl.getAttribute('data-block-pool-id');
                    const pool = ctx.pool ?? [];
                    const entry = pidAttr != null
                        ? pool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined
                        : undefined;
                    const clickedAnchorR0 = entry?.t === 0 ? entry.r[0] : undefined;
                    if (clickedAnchorR0 !== undefined && clickedAnchorR0 !== ctx.editTarget.anchorR0) {
                        ctx.requestClickSwitch?.(clickedEl);
                    }
                }
                // else: empty area click — no click-switch, plain close on blur.
            }
            return;
        }

        const el = findEditTarget(e);
        if (!el) return;

        if (e.pointerType !== 'mouse') {
            // Touch: outline on pointerdown, activate after hold. §3: outline the
            // mode-aware resolved surface (ctx is fresh here — [activate, ctx] deps);
            // the hold timer's activate(el) re-resolves the raw leaf itself.
            rawLeafRef.current = el;
            const resolved = ctx?.unlockNestingCursor ? el : (resolveOuterBlock(el) ?? el);
            outlineElement(resolved);
            hoveredRef.current = resolved;
            pointerDownPosRef.current = { x: e.clientX, y: e.clientY };
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
            holdTimerRef.current = setTimeout(() => {
                holdTimerRef.current = null;
                pointerDownPosRef.current = null;
                activate(el);
            }, HOLD_MS);
        }
    }, [activate, ctx]);

    const onPointerUp = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (e.pointerType !== 'mouse') {
            clearHold();
            pointerDownPosRef.current = null;
            return;
        }
        // Active-region guard (Phase 1 fix): if the click landed inside the
        // open editor's wrapper div, suppress activation — the textarea keeps
        // focus and handles the caret-move itself. Only clicks OUTSIDE the
        // region should resolve-and-switch.
        if (ctx?.activeEditRegionRef?.current?.contains(e.target as Node)) {
            return;
        }
        // P2.4d: if a dirty click-switch was handled in blur (landing for B is pending),
        // skip activate(B) — the reland will open B with the projected anchorR0.
        if (ctx?.consumeDirtySwitchHandled?.()) {
            return;
        }
        // Mouse click: activate.
        const el = findEditTarget(e);
        if (el) {
            activate(el);
        }
    }, [activate, ctx]);

    const onPointerLeave = useCallback((e: React.PointerEvent<HTMLElement>) => {
        if (e.pointerType === 'mouse') {
            outlineElement(null);
            rawLeafRef.current = null;
        }
    }, []);

    const onKeyDown = useCallback((e: React.KeyboardEvent<HTMLElement>) => {
        if (ctx?.editTarget != null) return;
        // P2.2 Roving-tabindex navigation: arrows move focus through the
        // outer-block list (enumerateOuterBlocks), not the raw
        // querySelectorAll result. This means:
        //   - Hidden outer blocks (zero-area rect) are skipped automatically.
        //   - Coincident lone-child wrappers dedupe their child away.
        //   - Enter on the focused outer block resolves idempotently → same outer block.
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault();
            const host = e.currentTarget;
            // §2 mode-aware roving: focus the C1 visible-surface partition in unlock
            // mode (incl. <li>/<dd> proxies for §0 multi-block leading text), or
            // outer blocks in locked mode — so roving visits exactly what a click
            // would activate.
            const blocks = (ctx?.unlockNestingCursor
                ? enumerateNestingSurfaces(host)
                : enumerateOuterBlocks(host)) as HTMLElement[];
            if (!blocks.length) return;
            const idx = blocks.indexOf(document.activeElement as HTMLElement);
            const next = e.key === 'ArrowDown'
                ? blocks[(idx + 1) % blocks.length]
                : blocks[(idx - 1 + blocks.length) % blocks.length];
            next.focus();
            // Set BOTH refs so a following hover doesn't redundantly re-resolve,
            // and Enter/Space activates the focused element.
            rawLeafRef.current = next;
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
