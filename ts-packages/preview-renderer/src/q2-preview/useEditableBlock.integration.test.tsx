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
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { RegistryContext } from '../framework';
import type { ResolvedSource } from './sourceIndex';
import { Block } from './dispatchers';

afterEach(() => cleanup());

const MOCK_RECT: DOMRect = {
    width: 400,
    height: 80,
    top: 200,
    bottom: 280,
    left: 10,
    right: 410,
    x: 10,
    y: 200,
    toJSON: () => ({}),
};

const MOCK_RESOLVED: ResolvedSource = {
    sourceNode: { t: 'Para', c: [] } as any,
    reachabilityClass: 'TopLevel',
    // r = [0, 12] slices 'test content' (12 ASCII bytes)
    sourceEntry: { t: 0, r: [0, 12], d: 0 },
};

const POOL_ID = 42;
// A Para node whose .s field equals POOL_ID (simulates what the renderer sends).
const PARA_NODE = { t: 'Para', c: [], s: POOL_ID } as any;

function mountBlock(
    editTargetPoolId: number,
    nodePoolId: number = editTargetPoolId,
) {
    const node = { t: 'Para', c: [], s: nodePoolId } as any;
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: { poolId: editTargetPoolId, rect: MOCK_RECT, contentHeight: 72 },
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
        editTarget: { poolId: POOL_ID, rect: MOCK_RECT, contentHeight: 72 },
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
