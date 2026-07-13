/**
 * BreadcrumbCrumbs.tsx — the interactive nesting breadcrumb (bd-9x3zbuj8 Task 2 /
 * bd-igpm0xur). Renders the inner trio (◀ · crumbs · ▶ · future placeholder) as a
 * fragment for the EditToolbar row to wrap; crumbs are content-sized and flow after
 * the other chrome. Reads nesting handlers off `PreviewContext`; injects the sheet once.
 *
 * (The standalone floating `BreadcrumbChip` and its spill/ellipsis geometry were
 * retired when the breadcrumb folded into the toolbar — inline is the only rendering.)
 */

import React, { useContext } from 'react';
import { PreviewContext } from './PreviewContext';
import { detectPlatform } from './nestingNav';
import type { AncestorCrumb } from './nestingNav';

// ── Shared stylesheet (inject-once) ─────────────────────────────────────────────

let stylesInjected = false;

/** Inject the shared breadcrumb crumb-button stylesheet once. */
export function ensureBreadcrumbStyles(): void {
    if (stylesInjected || typeof document === 'undefined') return;
    stylesInjected = true;
    const style = document.createElement('style');
    style.setAttribute('data-q2-breadcrumb', '1');
    style.textContent = CSS;
    document.head.appendChild(style);
}

const CSS = `
.q2-breadcrumb-crumbs {
    display: flex;
    align-items: center;
    gap: 0;
}
.q2-crumb {
    border: none;
    background: transparent;
    font-size: 12px;
    padding: 1px 3px;
    cursor: pointer;
    color: inherit;
    line-height: 1.4;
    white-space: nowrap;
    text-align: center;
    /* Content-sized: crumbs flow after the toolbar chrome at their natural width. */
    flex: 0 0 auto;
}
.q2-crumb-current {
    font-weight: bold;
    text-decoration: underline;
}
.q2-crumb:not(.q2-crumb-current):hover {
    text-decoration: underline;
}
.q2-breadcrumb-out,
.q2-breadcrumb-in {
    border: none;
    background: transparent;
    font-size: 11px;
    padding: 1px 4px;
    cursor: pointer;
    color: #555;
    line-height: 1.4;
    border-radius: 3px;
    flex-shrink: 0;
}
.q2-breadcrumb-out:hover,
.q2-breadcrumb-in:hover {
    background: rgba(0,0,0,0.08);
}
.q2-crumb-cat-container { color: #4f46e5; }
.q2-crumb-cat-list      { color: #15803d; }
.q2-crumb-cat-quote     { color: #b45309; }
.q2-crumb-cat-leaf-text { color: #0284c7; }
.q2-crumb-cat-embed     { color: #0f766e; }
.q2-breadcrumb-future   { opacity: 0.4; }
`;

// ── Component ──────────────────────────────────────────────────────────────────

export interface BreadcrumbCrumbsProps {
    /** The ancestor path (outermost → innermost, current = last). */
    crumbs: AncestorCrumb[];
}

/**
 * The crumb trio (◀ · band · ▶ · future placeholder), as a fragment. Reads the
 * nesting handlers off PreviewContext; injects the shared stylesheet.
 */
export function BreadcrumbCrumbs({ crumbs }: BreadcrumbCrumbsProps): React.ReactElement {
    ensureBreadcrumbStyles();
    const ctx = useContext(PreviewContext);
    const platform = detectPlatform();
    const outTip = platform === 'mac' ? 'Out (⌘⌃←)' : 'Out (Alt+Shift+←)';
    const inTip = platform === 'mac' ? 'In (⌘⌃→)' : 'In (Alt+Shift+→)';

    return (
        <>
            {/* ◀ out-arrow */}
            <button
                type="button"
                className="q2-breadcrumb-out"
                title={outTip}
                aria-label={outTip}
                onPointerDown={(e) => e.preventDefault()}
                onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('out'); }}
            >◀</button>
            {/* Crumb band */}
            <div className="q2-breadcrumb-crumbs">
                {crumbs.map((c) => (
                    <button
                        key={`${c.r0}-${c.r1}`}
                        type="button"
                        className={[
                            'q2-crumb',
                            `q2-crumb-cat-${c.category}`,
                            c.isCurrent ? 'q2-crumb-current' : '',
                        ].filter(Boolean).join(' ')}
                        title={c.label}
                        aria-label={c.label}
                        aria-current={c.isCurrent ? 'true' : undefined}
                        onPointerDown={(e) => e.preventDefault()}
                        onClick={(e) => { e.stopPropagation(); ctx?.requestNestingSelect?.(c.r0, c.r1); }}
                    >{c.abbrev}</button>
                ))}
            </div>
            {/* ▶ in-arrow + future-crumb placeholder */}
            <button
                type="button"
                className="q2-breadcrumb-in"
                title={inTip}
                aria-label={inTip}
                style={{ flex: '0 0 auto' }}
                onPointerDown={(e) => e.preventDefault()}
                onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('in'); }}
            >▶</button>
            <span className="q2-breadcrumb-future" />
        </>
    );
}
