/**
 * P2.3a integration tests: byte-offset identity + controlled value + dirty guard.
 *
 * TDD: these tests were written BEFORE implementation. They exercised:
 * 1. Dirty guard: blur without typing → commitTextEdit NOT called
 * 2. Dirty guard: type-then-revert → not committed
 * 3. CRLF normalization: CRLF-sourced block, blur untyped → no commit
 * 4. Match by anchorR0: dispatcher renders textarea for matching block only
 * 5. Controlled value survives remount
 * 6. IME composition: no commit mid-composition, normal commit after
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { RegistryContext } from '../framework';
import type { ResolvedSource } from './sourceIndex';
import { Block } from './dispatchers';

afterEach(() => cleanup());

// A CRLF-sourced content string - "hello\r\nworld"
// In bytes: 'h'(0) 'e'(1) 'l'(2) 'l'(3) 'o'(4) '\r'(5) '\n'(6) 'w'(7) 'o'(8) 'r'(9) 'l'(10) 'd'(11)
const CRLF_CONTENT = 'hello\r\nworld';

// anchorR0=0, anchorR1=7 → "hello\r\n" which normalizes to "hello\n"
const CRLF_RESOLVED: ResolvedSource = {
    sourceNode: { t: 'Para', c: [] } as any,
    reachabilityClass: 'TopLevel',
    sourceEntry: { t: 0, r: [0, 7], d: 0 },
};

// LF-only content: "test content" (12 ASCII bytes)
const LF_CONTENT = 'test content';

const LF_RESOLVED: ResolvedSource = {
    sourceNode: { t: 'Para', c: [] } as any,
    reachabilityClass: 'TopLevel',
    sourceEntry: { t: 0, r: [0, 12], d: 0 },
};

const MOCK_BOX_STYLE: Record<string, string> = {
    marginTop: '0px', marginRight: '0px', marginBottom: '0px', marginLeft: '0px',
    paddingTop: '0px', paddingRight: '0px', paddingBottom: '0px', paddingLeft: '0px',
    borderTopWidth: '0px', borderRightWidth: '0px', borderBottomWidth: '0px', borderLeftWidth: '0px',
    borderTopStyle: 'none', borderRightStyle: 'none', borderBottomStyle: 'none', borderLeftStyle: 'none',
    borderTopColor: 'rgb(0,0,0)', borderRightColor: 'rgb(0,0,0)',
    borderBottomColor: 'rgb(0,0,0)', borderLeftColor: 'rgb(0,0,0)',
};

// Build a full PreviewContext with new editTarget shape + editDraftRef
function buildCtx(overrides: Partial<PreviewContextValue> = {}): PreviewContextValue {
    const editDraftRef = { current: null as string | null };
    return {
        currentFilePath: '/project/test.qmd',
        content: LF_CONTENT,
        editTarget: {
            anchorR0: 0,
            anchorR1: 12,
            anchorSlice: 'test content',
            contentHeight: 40,
            boxStyle: MOCK_BOX_STYLE,
        },
        setEditTarget: vi.fn(),
        commitTextEdit: vi.fn(),
        resolveSource: () => LF_RESOLVED,
        editDraftRef,
        ...overrides,
    };
}

function mountBlock(ctx: PreviewContextValue) {
    // node.s = 0 (pool id); resolveSource() returns LF_RESOLVED with r[0] matching anchorR0
    const node = { t: 'Para', c: [], s: 0 } as any;
    return render(
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>,
    );
}

// ── Dirty guard ────────────────────────────────────────────────────────────

describe('P2.3a — dirty guard: blur without typing', () => {
    it('does NOT call commitTextEdit when blurring without typing', () => {
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea');
        expect(ta).not.toBeNull();

        // Blur without changing the value
        fireEvent.blur(ta!);

        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
        // setEditTarget(null) IS expected to close the editor
        expect(ctx.setEditTarget).toHaveBeenCalledWith(null);
    });

    it('does NOT call commitTextEdit when reverting to original text', () => {
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        // Type something different, then revert back to original
        fireEvent.change(ta, { target: { value: 'modified content' } });
        fireEvent.change(ta, { target: { value: 'test content' } }); // revert exactly
        fireEvent.blur(ta);

        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
    });

    it('DOES call commitTextEdit when text has changed', () => {
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        fireEvent.change(ta, { target: { value: 'changed text' } });
        fireEvent.blur(ta);

        expect(ctx.commitTextEdit).toHaveBeenCalledOnce();
    });

    it('does NOT call commitTextEdit when clearing to empty (empty→cancel, not empty→commit)', () => {
        // Fix 1: clearing the textarea to "" must cancel (close editor), NOT commit
        // an empty string that would delete the block.
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        // Clear the textarea to empty
        fireEvent.change(ta, { target: { value: '' } });
        fireEvent.blur(ta);

        // Must NOT commit the empty string
        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
        // Editor must close (cancel)
        expect(ctx.setEditTarget).toHaveBeenCalledWith(null);
    });

    it('does NOT call commitTextEdit when draft is whitespace-only (whitespace→cancel)', () => {
        // Fix 1: whitespace-only draft (normalizes+trims to "") must also cancel.
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        fireEvent.change(ta, { target: { value: '   \n  ' } });
        fireEvent.blur(ta);

        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
        expect(ctx.setEditTarget).toHaveBeenCalledWith(null);
    });
});

describe('P2.3a — dirty guard: CRLF normalization', () => {
    it('does NOT commit when blurring a CRLF-sourced block without typing (normalization makes them equal)', () => {
        // anchorSlice will be "hello\n" (normalized from "hello\r\n".trimEnd() = "hello"),
        // and the textarea shows "hello" (LF-normalized by browser).
        // Both sides normalize → equal → no commit.
        const editDraftRef = { current: 'hello' as string | null }; // seeded with normalized value
        const ctx: PreviewContextValue = {
            currentFilePath: '/project/test.qmd',
            content: CRLF_CONTENT,
            editTarget: {
                anchorR0: 0,
                anchorR1: 7,
                // anchorSlice = normalizeLineEndings(sliceBytes(content, 0, 7)).trimEnd()
                // = normalizeLineEndings("hello\r\n").trimEnd() = "hello\n".trimEnd() = "hello"
                anchorSlice: 'hello',
                contentHeight: 40,
                boxStyle: MOCK_BOX_STYLE,
            },
            setEditTarget: vi.fn(),
            commitTextEdit: vi.fn(),
            resolveSource: () => CRLF_RESOLVED,
            editDraftRef,
        };
        const node = { t: 'Para', c: [], s: 0 } as any;
        const { container } = render(
            <PreviewContext.Provider value={ctx}>
                <RegistryContext.Provider value={{ registry: {} }}>
                    <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
                </RegistryContext.Provider>
            </PreviewContext.Provider>,
        );
        const ta = container.querySelector('textarea')!;
        // The textarea value is "hello" (no trailing \r\n since trimEnd removes it)
        // Blur without typing
        fireEvent.blur(ta);
        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
    });
});

// ── Match by anchorR0 ───────────────────────────────────────────────────────

describe('P2.3a — match by anchorR0', () => {
    it('renders textarea for block whose resolved.sourceEntry.r[0] matches anchorR0', () => {
        const ctx = buildCtx(); // anchorR0 = 0, LF_RESOLVED.sourceEntry.r = [0, 12]
        const { container } = mountBlock(ctx);
        expect(container.querySelector('textarea')).not.toBeNull();
    });

    it('does NOT render textarea for block whose resolved.sourceEntry.r[0] does NOT match anchorR0', () => {
        // Build ctx with anchorR0 = 100 (different from LF_RESOLVED.sourceEntry.r[0] = 0)
        const editDraftRef = { current: null as string | null };
        const ctx: PreviewContextValue = {
            currentFilePath: '/project/test.qmd',
            content: LF_CONTENT,
            editTarget: {
                anchorR0: 100, // different from r[0] = 0
                anchorR1: 112,
                anchorSlice: 'different block',
                contentHeight: 40,
                boxStyle: MOCK_BOX_STYLE,
            },
            setEditTarget: vi.fn(),
            commitTextEdit: vi.fn(),
            resolveSource: () => LF_RESOLVED, // r[0] = 0, not 100
            editDraftRef,
        };
        const node = { t: 'Para', c: [], s: 0 } as any;
        const { container } = render(
            <PreviewContext.Provider value={ctx}>
                <RegistryContext.Provider value={{ registry: {} }}>
                    <Block node={node} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
                </RegistryContext.Provider>
            </PreviewContext.Provider>,
        );
        expect(container.querySelector('textarea')).toBeNull();
    });
});

// ── Controlled value survives remount ──────────────────────────────────────

describe('P2.3a — controlled value survives remount', () => {
    it('preserves typed text when the textarea is remounted (ref-seeded local state)', () => {
        const editDraftRef = { current: 'test content' as string | null };
        const ctx = buildCtx({ editDraftRef });

        function Wrapper({ forceKey }: { forceKey: number }) {
            return (
                <PreviewContext.Provider value={ctx}>
                    <RegistryContext.Provider value={{ registry: {} }}>
                        <Block key={forceKey} node={{ t: 'Para', c: [], s: 0 } as any} setLocalAst={() => {}} onNavigateToDocument={() => {}} />
                    </RegistryContext.Provider>
                </PreviewContext.Provider>
            );
        }

        const { container, rerender } = render(<Wrapper forceKey={1} />);
        const ta = container.querySelector('textarea')!;

        // Type some text
        fireEvent.change(ta, { target: { value: 'typed text' } });
        // editDraftRef.current should now be 'typed text'
        expect(editDraftRef.current).toBe('typed text');

        // Force remount by changing key (simulates index shift)
        rerender(<Wrapper forceKey={2} />);

        // The new textarea should have the typed value (seeded from ref)
        const ta2 = container.querySelector('textarea')!;
        expect(ta2.value).toBe('typed text');
    });
});

// ── IME composition ─────────────────────────────────────────────────────────

describe('P2.3a — IME composition', () => {
    it('does NOT fire a commit on blur during composition', () => {
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        // Start IME composition
        fireEvent.compositionStart(ta);
        // Input while composing
        fireEvent.change(ta, { target: { value: 'こんにちは' } });
        // Blur WHILE composing (before compositionend)
        // The blur should NOT commit since composition is in progress
        fireEvent.blur(ta);

        // No commit should happen during composition
        expect(ctx.commitTextEdit).not.toHaveBeenCalled();
    });

    it('DOES commit after compositionend, when text is different', () => {
        const ctx = buildCtx();
        const { container } = mountBlock(ctx);
        const ta = container.querySelector('textarea')!;

        // Complete IME composition
        fireEvent.compositionStart(ta);
        fireEvent.change(ta, { target: { value: 'こんにちは' } });
        fireEvent.compositionEnd(ta);

        // Now blur → should commit since composition ended and text changed
        fireEvent.blur(ta);

        expect(ctx.commitTextEdit).toHaveBeenCalledOnce();
    });
});
