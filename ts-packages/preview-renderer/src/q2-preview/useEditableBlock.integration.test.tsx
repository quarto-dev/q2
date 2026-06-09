/**
 * RTL tests for useEditableBlock (Plan 2b).
 *
 * P1: given a mocked `editTarget`, the textarea renders with:
 *     - `width: 100%` (inherits from wrapper element, not rect.width)
 *     - `height: contentHeight` (content-area height, not rect.height)
 * P2: textarea font is `fontFamily: 'monospace'` and `fontSize: '0.9em'`.
 *
 * jsdom 26 supports DOMRect natively. Em units are NOT resolved to px
 * by getComputedStyle — assert the raw '0.9em' string.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { useEditableBlock } from './useEditableBlock';
import type { ResolvedSource } from './sourceIndex';

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

function TestEditor({ poolId }: { poolId: number }) {
    const editor = useEditableBlock({ poolId, resolved: MOCK_RESOLVED });
    if (editor) return <>{editor}</>;
    return <div data-testid="no-editor" />;
}

function mountEditor(
    editTargetPoolId: number,
    editorPoolId: number = editTargetPoolId,
) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        content: 'test content',
        editTarget: { poolId: editTargetPoolId, rect: MOCK_RECT, contentHeight: 72 },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <TestEditor poolId={editorPoolId} />
        </PreviewContext.Provider>,
    );
}

describe('useEditableBlock — P1 sizing', () => {
    it('renders <textarea> with width: 100% (inherits from wrapper element)', () => {
        const { container } = mountEditor(1);
        const ta = container.querySelector('textarea')!;
        expect(ta).not.toBeNull();
        expect(ta.style.width).toBe('100%');
    });

    it('renders <textarea> with height matching editTarget.contentHeight', () => {
        const { container } = mountEditor(1);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.height).toBe('72px');
    });

    it('renders no <textarea> when poolId does not match editTarget.poolId', () => {
        // editTarget.poolId = 99, but TestEditor has poolId = 1 — no match
        const { container } = mountEditor(/* editTargetPoolId= */ 99, /* editorPoolId= */ 1);
        expect(container.querySelector('textarea')).toBeNull();
    });
});

describe('useEditableBlock — P2 font', () => {
    it("renders <textarea> with fontFamily: 'monospace'", () => {
        const { container } = mountEditor(1);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.fontFamily).toBe('monospace');
    });

    it("renders <textarea> with fontSize: '0.9em' (raw string, not computed px)", () => {
        const { container } = mountEditor(1);
        const ta = container.querySelector('textarea')!;
        expect(ta.style.fontSize).toBe('0.9em');
    });
});
