/**
 * BreadcrumbChip.tsx — floating ancestor-path breadcrumb for the nesting cursor.
 *
 * P3.4 §3d: shows the AST ancestor path (e.g. "Section › Div › Paragraph")
 * with the current level highlighted. Rendered when unlockNestingCursor=true
 * AND an editor is open (editTarget != null). Self-gating: renders null
 * otherwise.
 *
 * Pointer-isolation note: stopPropagation/preventDefault are implemented here
 * for correct real-browser behaviour (prevent host click-switch; prevent blur-
 * commit on button press). jsdom's fireEvent.click does not simulate pointer
 * events or focus/blur, so these behaviours are NOT jsdom-tested here.
 * Pointer-isolation testing is deferred to P3.5 Playwright.
 */

import React, { useContext, useLayoutEffect, useRef, useState } from 'react';
import { PreviewContext } from './PreviewContext';
import { buildAncestorPath, detectPlatform } from './nestingNav';

export function BreadcrumbChip(): React.ReactElement | null {
    const ctx = useContext(PreviewContext);
    const chipRef = useRef<HTMLDivElement | null>(null);
    const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

    const et = ctx?.editTarget;
    const active = !!ctx?.unlockNestingCursor && !!et;

    // Geometry: anchor the chip's bottom edge to the active surface's top
    // (negative offset, sits ABOVE the first line). jsdom returns zero rects, so
    // real placement is verified in P3.5 Playwright; this never throws on zeros.
    useLayoutEffect(() => {
        if (!active) { setPos(null); return; }
        const surface = ctx?.activeEditRegionRef?.current;
        const host = surface?.offsetParent as HTMLElement | null;
        if (!surface) return;
        const sRect = surface.getBoundingClientRect();
        const hRect = host?.getBoundingClientRect() ?? { top: 0, left: 0 } as DOMRect;
        const chipH = chipRef.current?.getBoundingClientRect().height ?? 0;
        setPos({ top: sRect.top - hRect.top - chipH, left: sRect.left - hRect.left });
    }, [active, et?.anchorR0, et?.anchorR1, ctx?.activeEditRegionRef]);

    if (!active || !et) return null;

    const crumbs = buildAncestorPath(ctx?.sourceIndex, et.anchorR0, et.anchorR1);
    const platform = detectPlatform();
    const outTip = platform === 'mac' ? 'Out (⌘⌃←)' : 'Out (Alt+Shift+←)';
    const inTip = platform === 'mac' ? 'In (⌘⌃→)' : 'In (Alt+Shift+→)';

    // stopPropagation: the host (#quarto-content) carries delegated pointer
    // handlers (useBlockEditHover); the chip must fully intercept its own pointer
    // events so a chip click is never read as a leaf-reset/click-switch.
    // preventDefault on pointerdown keeps the textarea focused (no blur-commit on
    // a button press). [Real focus/blur + pointer-ordering: verified in P3.5.]
    const eat = (e: React.PointerEvent) => { e.stopPropagation(); e.preventDefault(); };

    return (
        <div
            ref={chipRef}
            className="q2-breadcrumb-chip"
            data-testid="q2-breadcrumb-chip"
            role="toolbar"
            aria-label="Nesting breadcrumb"
            onPointerDown={eat}
            onPointerUp={(e) => e.stopPropagation()}
            style={{
                position: 'absolute',
                top: pos ? `${pos.top}px` : undefined,
                left: pos ? `${pos.left}px` : undefined,
                zIndex: 50,
                display: 'flex',
                alignItems: 'center',
                gap: '2px',
                pointerEvents: 'auto',
            }}
        >
            <button
                type="button"
                className="q2-breadcrumb-out"
                title={outTip}
                aria-label={outTip}
                onPointerDown={(e) => e.preventDefault()}
                onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('out'); }}
            >◀</button>
            {crumbs.map((c, i) => (
                <React.Fragment key={`${c.r0}-${c.r1}`}>
                    {i > 0 && <span className="q2-breadcrumb-sep" aria-hidden="true">›</span>}
                    <button
                        type="button"
                        className={c.isCurrent ? 'q2-crumb q2-crumb-current' : 'q2-crumb'}
                        aria-current={c.isCurrent ? 'true' : undefined}
                        onPointerDown={(e) => e.preventDefault()}
                        onClick={(e) => { e.stopPropagation(); ctx?.requestNestingSelect?.(c.r0, c.r1); }}
                    >{c.label}</button>
                </React.Fragment>
            ))}
            <button
                type="button"
                className="q2-breadcrumb-in"
                title={inTip}
                aria-label={inTip}
                onPointerDown={(e) => e.preventDefault()}
                onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('in'); }}
            >▶</button>
        </div>
    );
}
