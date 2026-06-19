/**
 * RTL tests for in-place block editing (Plan 3: textarea in Block dispatcher).
 *
 * P1: given a mocked `editTarget`, the textarea renders with:
 *     - `width: 100%` (inherits from wrapper element, not rect.width)
 *     - `height: contentHeight` (content-area height, not rect.height)
 * P2: textarea font is `fontFamily: 'monospace'` and `fontSize: '0.9em'`.
 *
 * These tests exercise the `Block` dispatcher directly (the logic moved here
 * from `useEditableBlock` in Plan 3).  The dispatcher is rendered with a Para
 * node, a minimal registry, and a PreviewContext whose `editTarget.anchorR0`
 * matches the node's resolved source entry's `r[0]` (P2.3a: identity by byte offset).
 *
 * jsdom 26 supports DOMRect natively. Em units are NOT resolved to px
 * by getComputedStyle — assert the raw '0.9em' string.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { RegistryContext } from '../framework';
import type { NodeArgs } from '../framework';
import type { ResolvedSource } from './sourceIndex';
import { Block } from './dispatchers';
import { Para } from './blocks/Para';

afterEach(() => cleanup());

const MOCK_RESOLVED: ResolvedSource = {
    sourceNode: { t: 'Para', c: [] } as any,
    reachabilityClass: 'TopLevel',
    // r = [0, 12] slices 'test content' (12 ASCII bytes)
    sourceEntry: { t: 0, r: [0, 12], d: 0 },
};

/**
 * A representative computed box for `editTarget.boxStyle`: a non-zero margin,
 * a bottom padding + a visible bottom border (mimicking a Bootstrap h2 rule),
 * and a large left padding (mimicking a list's marker gutter) so the
 * list-left-inset-strip behavior can be asserted.
 */
const MOCK_BOX_STYLE: Record<string, string> = {
    marginTop: '16px', marginRight: '0px', marginBottom: '1em', marginLeft: '0px',
    paddingTop: '0px', paddingRight: '0px', paddingBottom: '8px', paddingLeft: '40px',
    borderTopWidth: '0px', borderRightWidth: '0px', borderBottomWidth: '1px', borderLeftWidth: '0px',
    borderTopStyle: 'none', borderRightStyle: 'none', borderBottomStyle: 'solid', borderLeftStyle: 'none',
    borderTopColor: 'rgb(0, 0, 0)', borderRightColor: 'rgb(0, 0, 0)',
    borderBottomColor: 'rgb(222, 226, 230)', borderLeftColor: 'rgb(0, 0, 0)',
};

// P2.3a: editTarget now uses anchorR0/anchorR1/anchorSlice instead of poolId.
// MOCK_RESOLVED.sourceEntry.r = [0, 12], so anchorR0 = 0, anchorR1 = 12.
const POOL_ID = 42;
// anchorR0 matches MOCK_RESOLVED.sourceEntry.r[0] = 0
const ANCHOR_R0 = 0;
const ANCHOR_R1 = 12;
const ANCHOR_SLICE = 'test content'; // sliceBytes('test content', 0, 12).trimEnd()

function mountBlock(
    matchingR0: number = ANCHOR_R0,
    nodePoolId: number = POOL_ID,
) {
    const node = { t: 'Para', c: [], s: nodePoolId } as any;
    // isBlockEditTarget matches by anchorR0 === resolved.sourceEntry.r[0].
    // MOCK_RESOLVED.sourceEntry.r[0] = 0. When matchingR0 ≠ 0, no textarea renders.
    const editDraftRef = { current: ANCHOR_SLICE as string | null };
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: {
            anchorR0: matchingR0,
            anchorR1: matchingR0 === ANCHOR_R0 ? ANCHOR_R1 : matchingR0 + 12,
            anchorSlice: ANCHOR_SLICE,
            contentHeight: 72,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: (n: any) =>
            n.s === nodePoolId ? MOCK_RESOLVED : null,
        editDraftRef,
    };
    const registry = {};
    return render(
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry }}>
                <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>,
    );
}

describe('Block dispatcher — P1 sizing', () => {
    it('renders <textarea> with width: 100% (inherits from wrapper element)', () => {
        const { container } = mountBlock(ANCHOR_R0);
        const ta = container.querySelector('textarea')!;
        expect(ta).not.toBeNull();
        expect(ta.style.width).toBe('100%');
    });

    it('renders <textarea> with height matching editTarget.contentHeight', () => {
        const { container } = mountBlock(ANCHOR_R0);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.height).toBe('72px');
    });

    it('renders no <textarea> when anchorR0 does not match resolved.sourceEntry.r[0]', () => {
        // anchorR0 = 99, but MOCK_RESOLVED.sourceEntry.r[0] = 0 — no match
        // P2.3a: identity is now matched by anchorR0, not poolId
        const { container } = mountBlock(/* matchingR0= */ 99);
        expect(container.querySelector('textarea')).toBeNull();
    });
});

describe('Block dispatcher — P2 font', () => {
    it("renders <textarea> with fontFamily: 'monospace'", () => {
        const { container } = mountBlock(ANCHOR_R0);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.fontFamily).toBe('monospace');
    });

    it('renders <textarea> with fontSize in em units (raw string, not computed px)', () => {
        const { container } = mountBlock(ANCHOR_R0);
        const ta = container.querySelector('textarea')!;
        // G15-font: loosened from '0.9em' to accept any em value so the test
        // stays green after EDITOR_FONT_SIZE was tuned (0.9em → 0.825em).
        expect(ta.style.fontSize).toMatch(/^[0-9.]+em$/);
    });
});

// ---------------------------------------------------------------------------
// G15 — one-line editing surface rows attribute
// ---------------------------------------------------------------------------

describe('Block dispatcher — G15 rows=1 structural fix', () => {
    it('renders <textarea> with rows=1 (prevents HTML default rows=2 inflating the autosize)', () => {
        // G15-0 binding: ta.rows === 1 with the fix; HTML default is 2 without it.
        // Named revert hunk: remove rows={1} from the textarea in dispatchers.tsx
        // → ta.rows reads the HTML default 2 → expected 1, got 2 → RED.
        const { container } = mountBlock(ANCHOR_R0);
        const ta = container.querySelector('textarea')!;
        expect(ta).not.toBeNull();
        expect(ta.rows).toBe(1);
    });
});

// ---------------------------------------------------------------------------
// Plan 3 — gate widening tests
// ---------------------------------------------------------------------------

/** Mount the Block dispatcher with a specific reachabilityClass override. */
function mountBlockWithClass(
    reachabilityClass: ResolvedSource['reachabilityClass'] | null,
) {
    const node = { t: 'Para', c: [], s: POOL_ID } as any;
    const resolved: ResolvedSource | null =
        reachabilityClass === null
            ? null
            : { ...MOCK_RESOLVED, reachabilityClass };
    // P2.3a: anchorR0 must match resolved.sourceEntry.r[0] = 0 for the gate to pass.
    const editDraftRef = { current: ANCHOR_SLICE as string | null };
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: {
            anchorR0: ANCHOR_R0,
            anchorR1: ANCHOR_R1,
            anchorSlice: ANCHOR_SLICE,
            contentHeight: 72,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: () => resolved,
        editDraftRef,
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>,
    );
}

describe('Block dispatcher — Plan 3 gate widening', () => {
    it('renders <textarea> for a Descendable block (widened from TopLevel-only)', () => {
        const { container } = mountBlockWithClass('Descendable');
        expect(container.querySelector('textarea')).not.toBeNull();
    });

    it('does NOT render <textarea> for an Opaque block', () => {
        const { container } = mountBlockWithClass('Opaque');
        expect(container.querySelector('textarea')).toBeNull();
    });

    it('does NOT render <textarea> when resolveSource returns null (C2 safety)', () => {
        // null?.reachabilityClass !== 'Opaque' would be true — the explicit
        // `resolved != null` check in isBlockEditTarget must catch this.
        const { container } = mountBlockWithClass(null);
        expect(container.querySelector('textarea')).toBeNull();
    });
});

// ---------------------------------------------------------------------------
// Measure-and-set edit wrapper — structural invariants
//
// Editing any block replaces it with a synthetic <div> whose inline style
// reproduces the original element's full computed box (margin + padding +
// per-side border from editTarget.boxStyle). The original element is NOT kept
// in the DOM. List types additionally have their left inset (marker gutter)
// stripped so the textarea starts at column 0.
// ---------------------------------------------------------------------------

function mountBlockStructural(
    nodeType: string,
    registryComponent: ((args: NodeArgs<any>) => React.ReactNode) | null,
) {
    const node = { t: nodeType, c: [], s: POOL_ID } as any;
    // P2.3a: anchorR0 must match MOCK_RESOLVED.sourceEntry.r[0] = 0.
    const editDraftRef = { current: ANCHOR_SLICE as string | null };
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: {
            anchorR0: ANCHOR_R0,
            anchorR1: ANCHOR_R1,
            anchorSlice: ANCHOR_SLICE,
            contentHeight: 72,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: () => MOCK_RESOLVED,
        editDraftRef,
    };
    const registry = registryComponent ? { [nodeType]: registryComponent } : {};
    return render(
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry }}>
                <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>,
    );
}

describe('Block dispatcher — measure-and-set edit wrapper', () => {
    it('replaces the original element with a synthetic <div> that reproduces the box', () => {
        // Even with the real Para component registered, edit mode does NOT keep
        // the <p>; the dispatcher substitutes a measure-and-set wrapper.
        const { container } = mountBlockStructural('Para', Para);
        expect(container.querySelector('p'), 'original <p> must be replaced').toBeNull();

        const ta = container.querySelector('textarea');
        expect(ta).not.toBeNull();

        // The textarea's parent <div> carries the captured box, including the
        // bottom margin, the bottom padding, and the visible bottom border
        // (an h2's "rule" survives this way).
        const wrapper = ta!.parentElement as HTMLElement;
        expect(wrapper.tagName).toBe('DIV');
        expect(wrapper.style.marginBottom).toBe(MOCK_BOX_STYLE.marginBottom);
        expect(wrapper.style.paddingBottom).toBe(MOCK_BOX_STYLE.paddingBottom);
        expect(wrapper.style.borderBottomWidth).toBe(MOCK_BOX_STYLE.borderBottomWidth);
        expect(wrapper.style.borderBottomStyle).toBe(MOCK_BOX_STYLE.borderBottomStyle);
        // Non-list type: the left padding is preserved (not stripped).
        expect(wrapper.style.paddingLeft).toBe(MOCK_BOX_STYLE.paddingLeft);
    });

    it('strips the left inset (marker gutter) for list types, keeping the vertical box', () => {
        // BulletList: <ul> cannot contain <textarea>, and its big left padding
        // would indent the source. The wrapper drops the left inset but keeps
        // the vertical box.
        const { container } = mountBlockStructural('BulletList', null);
        expect(container.querySelector('ul')).toBeNull();

        const ta = container.querySelector('textarea');
        expect(ta).not.toBeNull();
        const wrapper = ta!.parentElement as HTMLElement;
        expect(wrapper.tagName).toBe('DIV');
        // Vertical box preserved.
        expect(wrapper.style.marginBottom).toBe(MOCK_BOX_STYLE.marginBottom);
        expect(wrapper.style.paddingBottom).toBe(MOCK_BOX_STYLE.paddingBottom);
        // Left inset stripped to zero.
        expect(wrapper.style.paddingLeft).toBe('0px');
        expect(wrapper.style.marginLeft).toBe('0px');
    });
});
