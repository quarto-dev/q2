/**
 * P3.2 Test 1: unlockNestingCursor + nestedEditBuffers reach PreviewContext.
 *
 * TDD: written BEFORE implementation. Mounts the real PreviewRoot with the
 * two new payload fields and asserts a context consumer sees them.
 * Also asserts that OMITTING the fields yields undefined/falsy (default-locked).
 *
 * Fail-on-revert: remove the `unlockNestingCursor` field from the
 * PreviewContext.Provider value in PreviewRoot.tsx → the
 * "context consumer sees unlockNestingCursor: true" assertion reddens.
 */

import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import React, { useContext } from 'react';
import { PreviewContext } from './PreviewContext';
import { PreviewRoot } from './PreviewRoot';

// An AST with one Para so the customRegistry Para entry fires, giving us
// a component that runs inside PreviewContext.Provider.
const PARA_AST_JSON = JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: {},
    blocks: [{ t: 'Para', c: [{ t: 'Str', c: 'hi' }] }],
    astContext: {
        p: [{ t: 0, r: [0, 10], d: 0 }],
    },
});

const SAMPLE_NESTED_BUFFERS: Record<string, string> = {
    '0:0-10:0': '- nested item\n',
};

/**
 * Render PreviewRoot with a Para override that captures the PreviewContext
 * value during render. Returns `capturedCtx` synchronously after `render()`.
 */
function mountAndCapture(props: Partial<React.ComponentProps<typeof PreviewRoot>> = {}) {
    let capturedCtx: ReturnType<typeof useContext<typeof PreviewContext>> = null;

    function ContextCapture() {
        capturedCtx = useContext(PreviewContext);
        return <span>captured</span>;
    }

    const result = render(
        <PreviewRoot
            astJson={PARA_AST_JSON}
            currentFilePath="/project/test.qmd"
            assetManifest={{}}
            renderedContent="hi"
            setAst={() => {}}
            customRegistry={{ Para: ContextCapture }}
            {...props}
        />,
    );

    return { capturedCtx: capturedCtx as any, unmount: result.unmount };
}

describe('P3.2: unlockNestingCursor + nestedEditBuffers reach PreviewContext', () => {
    it('context consumer sees unlockNestingCursor: true when provided', () => {
        const { capturedCtx, unmount } = mountAndCapture({
            unlockNestingCursor: true,
            nestedEditBuffers: SAMPLE_NESTED_BUFFERS,
        });

        expect(capturedCtx).not.toBeNull();
        // Fail-on-revert: if unlockNestingCursor is removed from the provider value,
        // this assertion reddens.
        expect(capturedCtx?.unlockNestingCursor).toBe(true);
        expect(capturedCtx?.nestedEditBuffers).toEqual(SAMPLE_NESTED_BUFFERS);

        unmount();
    });

    it('context consumer sees unlockNestingCursor: undefined (default-locked) when not passed', () => {
        // Default-locked: unlockNestingCursor not passed → should be undefined in context.
        const { capturedCtx, unmount } = mountAndCapture();

        expect(capturedCtx).not.toBeNull();
        expect(capturedCtx?.unlockNestingCursor).toBeFalsy();
        expect(capturedCtx?.nestedEditBuffers).toBeUndefined();

        unmount();
    });

    it('context consumer sees nestedEditBuffers undefined when unlockNestingCursor is false', () => {
        const { capturedCtx, unmount } = mountAndCapture({
            unlockNestingCursor: false,
        });

        expect(capturedCtx).not.toBeNull();
        // false or undefined — both are falsy and represent "locked"
        expect(capturedCtx?.unlockNestingCursor).toBeFalsy();
        expect(capturedCtx?.nestedEditBuffers).toBeUndefined();

        unmount();
    });

    it('context consumer sees nestedEditBuffers when provided without unlockNestingCursor', () => {
        // Both fields are optional — nestedEditBuffers can be provided independently.
        const { capturedCtx, unmount } = mountAndCapture({
            nestedEditBuffers: SAMPLE_NESTED_BUFFERS,
        });

        expect(capturedCtx).not.toBeNull();
        expect(capturedCtx?.nestedEditBuffers).toEqual(SAMPLE_NESTED_BUFFERS);

        unmount();
    });
});
