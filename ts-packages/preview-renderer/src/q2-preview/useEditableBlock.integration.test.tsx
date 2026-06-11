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
 * node, a minimal registry, and a PreviewContext whose `editTarget.poolId`
 * matches the node's pool id.
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

const POOL_ID = 42;

function mountBlock(
    editTargetPoolId: number,
    nodePoolId: number = editTargetPoolId,
) {
    const node = { t: 'Para', c: [], s: nodePoolId } as any;
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: {
            poolId: editTargetPoolId, contentHeight: 72,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: (n: any) =>
            n.s === nodePoolId ? MOCK_RESOLVED : null,
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
        const { container } = mountBlock(POOL_ID);
        const ta = container.querySelector('textarea')!;
        expect(ta).not.toBeNull();
        expect(ta.style.width).toBe('100%');
    });

    it('renders <textarea> with height matching editTarget.contentHeight', () => {
        const { container } = mountBlock(POOL_ID);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.height).toBe('72px');
    });

    it('renders no <textarea> when poolId does not match editTarget.poolId', () => {
        // editTarget.poolId = 99, but node has poolId = POOL_ID — no match
        const { container } = mountBlock(/* editTargetPoolId= */ 99, /* nodePoolId= */ POOL_ID);
        expect(container.querySelector('textarea')).toBeNull();
    });
});

describe('Block dispatcher — P2 font', () => {
    it("renders <textarea> with fontFamily: 'monospace'", () => {
        const { container } = mountBlock(POOL_ID);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.fontFamily).toBe('monospace');
    });

    it("renders <textarea> with fontSize: '0.9em' (raw string, not computed px)", () => {
        const { container } = mountBlock(POOL_ID);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.fontSize).toBe('0.9em');
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
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: {
            poolId: POOL_ID, contentHeight: 72,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: () => resolved,
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
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: { poolId: POOL_ID, contentHeight: 72, boxStyle: MOCK_BOX_STYLE },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: () => MOCK_RESOLVED,
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
