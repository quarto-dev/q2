/**
 * bd-igpm0xur Phase 0 — EditTypeIndicator unit tests.
 *
 * The toolbar's type/nesting indicator. Reads editTarget/sourceIndex/
 * unlockNestingCursor from PreviewContext:
 *   - nesting OFF (default): a single, non-interactive CURRENT-type crumb
 *     (the `buildAncestorPath(...).at(-1)` entry) — no ◀/▶ nav.
 *   - nesting ON: the full interactive ancestor path via BreadcrumbCrumbs
 *     (◀/▶ present), exactly as the retired standalone chip did.
 */

// @vitest-environment jsdom

import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue, SourceIndexEntry } from '../PreviewContext';
import { EditTypeIndicator } from './EditTypeIndicator';

afterEach(() => cleanup());

function editTarget(anchorR0: number, anchorR1: number) {
    return { anchorR0, anchorR1, anchorSlice: '', contentHeight: 0, boxStyle: {} };
}

function ctxWith(
    sourceIndex: Map<string, SourceIndexEntry>,
    et: ReturnType<typeof editTarget>,
    unlockNestingCursor: boolean,
): PreviewContextValue {
    return {
        currentFilePath: '/t.qmd',
        sourceIndex,
        editTarget: et,
        unlockNestingCursor,
    } as unknown as PreviewContextValue;
}

/** A single top-level CodeBlock at bytes [0, 20]. */
function codeBlockIndex(): Map<string, SourceIndexEntry> {
    const m = new Map<string, SourceIndexEntry>();
    m.set('0:0-20:0', {
        sourceNode: { t: 'CodeBlock', c: [['', ['python'], []], 'x = 1'] } as any,
        reachabilityClass: 'TopLevel',
    });
    return m;
}

/** A Para (bytes [11,14]) nested in a Div (bytes [0,18]). */
function divParaIndex(): Map<string, SourceIndexEntry> {
    const m = new Map<string, SourceIndexEntry>();
    m.set('0:0-18:0', {
        sourceNode: { t: 'Div', c: [['', ['d'], []], []] } as any,
        reachabilityClass: 'Descendable',
    });
    m.set('0:11-14:0', {
        sourceNode: { t: 'Para', c: [{ t: 'Str', c: 'BBB' }] } as any,
        reachabilityClass: 'TopLevel',
    });
    return m;
}

describe('EditTypeIndicator', () => {
    it('nesting OFF → exactly one current-type crumb, no ◀/▶ nav', () => {
        const { container } = render(
            <PreviewContext.Provider value={ctxWith(codeBlockIndex(), editTarget(0, 20), false)}>
                <EditTypeIndicator />
            </PreviewContext.Provider>,
        );
        const crumbs = container.querySelectorAll('.q2-crumb');
        expect(crumbs).toHaveLength(1);
        // A code block abbreviates to "Cd".
        expect(crumbs[0].textContent).toBe('Cd');
        // No nesting navigation in the minimal indicator.
        expect(container.querySelector('.q2-breadcrumb-out')).toBeNull();
        expect(container.querySelector('.q2-breadcrumb-in')).toBeNull();
    });

    it('nesting ON → full BreadcrumbCrumbs with ◀/▶ present', () => {
        const { container } = render(
            <PreviewContext.Provider value={ctxWith(divParaIndex(), editTarget(11, 14), true)}>
                <EditTypeIndicator />
            </PreviewContext.Provider>,
        );
        const crumbs = Array.from(container.querySelectorAll('.q2-crumb'));
        expect(crumbs.map((c) => c.textContent)).toEqual(['Dv', '¶']);
        // The full breadcrumb includes the nesting nav arrows.
        expect(container.querySelector('.q2-breadcrumb-out')).not.toBeNull();
        expect(container.querySelector('.q2-breadcrumb-in')).not.toBeNull();
    });
});
