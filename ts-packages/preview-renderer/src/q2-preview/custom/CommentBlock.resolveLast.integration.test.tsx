/**
 * bd-bpt089zw — resolving the last comment must leave no artifacts.
 *
 * Two state-lifecycle bugs in `CommentWrapper`, both surfacing after the
 * `✓` (Resolve) click removes a block's last comment and the commit
 * round-trip re-renders the (index-keyed, hence preserved) bubble
 * component:
 *
 *  1. `selfExpanded` outlived the last comment, so the bubble rendered its
 *     expanded branch with zero rows — a bordered, empty pill.
 *  2. `bubbleHovered` (the block-wrapper glow) was set from the bubble's
 *     own onMouseEnter/onMouseLeave; the resolve re-render unmounts the
 *     button under the pointer, the browser re-evaluates :hover without
 *     boundary events, and the next mouseout comes from the NEW hovered
 *     node — which the bubble is not an ancestor of — so the leave never
 *     fired and the glow stuck forever.
 *
 * Plan: claude-notes/plans/2026-09-09-preview-comment-resolve-artifacts.md
 */

// @vitest-environment jsdom

import { describe, it, expect, afterEach, vi } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import React from 'react';
import { Ast } from '../../framework';
import { previewRegistry } from '../registry';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';
import type { ResolvedSource } from '../sourceIndex';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
    vi.restoreAllMocks();
});

const POOL = [{ t: 0, r: [0, 40], d: 0 }];
const SOURCE_ENTRY = POOL[0] as { t: 0; r: [number, number]; d: number };

function commentSpan(text: string): unknown {
    return { t: 'Span', c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: text }]] };
}

function isCommentSpan(inline: any): boolean {
    return inline?.t === 'Span' && (inline.c?.[0]?.[1] ?? []).includes('quarto-edit-comment');
}

/** One Para, `s: 0`, with the given comment texts appended as spans. */
function astJson(commentTexts: string[]): string {
    const blocks = [
        {
            t: 'Para',
            s: 0,
            c: [{ t: 'Str', c: 'Stuff.' }, ...commentTexts.map(commentSpan)],
        },
    ];
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: POOL },
    });
}

const resolveSource = (node: any): ResolvedSource | null => {
    if (node?.s === undefined) return null;
    return { sourceNode: node, reachabilityClass: 'TopLevel', sourceEntry: SOURCE_ENTRY };
};

function tree(commentTexts: string[], ctx: PreviewContextValue) {
    return (
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson(commentTexts)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>
    );
}

function mount(commentTexts: string[], mode: PreviewContextValue['commentsMode'] = 'show') {
    const commitSubtreeEdit = vi.fn();
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: mode,
        resolveSource,
        commitSubtreeEdit,
    };
    const result = render(tree(commentTexts, ctx));
    // Simulate the commit round-trip: the host re-renders with the new AST.
    const rerenderWith = (texts: string[]) => result.rerender(tree(texts, ctx));
    return { ...result, commitSubtreeEdit, rerenderWith };
}

// --- DOM helpers ------------------------------------------------------------

function para(container: HTMLElement): HTMLElement {
    const p = container.querySelector('p');
    expect(p).not.toBeNull();
    return p as HTMLElement;
}

/** The CommentWrapper host div (position: relative) around the paragraph. */
function wrapper(container: HTMLElement): HTMLElement {
    const host = para(container).parentElement as HTMLElement;
    expect(host.style.position).toBe('relative');
    return host;
}

function chrome(container: HTMLElement): HTMLElement | null {
    return container.querySelector('[data-q2-owns-focus]');
}

function bubble(container: HTMLElement): HTMLElement {
    const b = container.querySelector('.q2-comment-bubble');
    expect(b).not.toBeNull();
    return b as HTMLElement;
}

function resolveButtons(container: HTMLElement): HTMLElement[] {
    return [...container.querySelectorAll<HTMLElement>('[title="Resolve comment"]')];
}

/** The wrapper glows iff its box-shadow is something other than `none`. */
function wrapperGlows(container: HTMLElement): boolean {
    return wrapper(container).style.boxShadow !== 'none';
}

/** Stub an element's layout rect (jsdom reports all-zero rects). */
function stubRect(el: HTMLElement, r: { left: number; top: number; right: number; bottom: number }) {
    el.getBoundingClientRect = () =>
        ({ ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON() {} }) as DOMRect;
}

/** Expand the bubble by clicking it, as a user does to reach the ✓ button. */
function expandByClick(container: HTMLElement) {
    fireEvent.click(bubble(container));
    expect(resolveButtons(container).length).toBeGreaterThan(0);
}

/**
 * Click the index-th ✓ and check the commit dropped exactly that span.
 * Returns the comment texts that remain in the committed block.
 */
function resolveAt(
    container: HTMLElement,
    commit: ReturnType<typeof vi.fn>,
    index: number,
): string[] {
    fireEvent.click(resolveButtons(container)[index]);
    expect(commit).toHaveBeenCalledTimes(1);
    const committed = commit.mock.calls[0][1];
    const remaining = committed.c.filter(isCommentSpan).map((s: any) => s.c[1][0].c);
    return remaining;
}

/**
 * The reported flow's first half: hover the right half → `+` → type →
 * Enter → the host re-renders with the comment landed (the inline input
 * closes; the bubble stays self-expanded to show the new comment).
 */
function addFirstCommentViaPlus(
    container: HTMLElement,
    commit: ReturnType<typeof vi.fn>,
    rerenderWith: (texts: string[]) => void,
    text: string,
) {
    fireEvent.mouseMove(para(container), { clientX: 10, clientY: 5 });
    fireEvent.click(bubble(container));
    const input = container.querySelector('textarea.q2-comment-input') as HTMLTextAreaElement;
    expect(input).not.toBeNull();
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(commit).toHaveBeenCalledTimes(1);
    expect(commit.mock.calls[0][1].c.filter(isCommentSpan)).toHaveLength(1);
    commit.mockClear();
    rerenderWith([text]);
    expect(container.querySelector('textarea.q2-comment-input')).toBeNull();
    expect(resolveButtons(container)).toHaveLength(1);
}

describe('CommentBlock after resolving the last comment (bd-bpt089zw)', () => {
    it('T1: no empty pill — chrome disappears once the last comment is gone', () => {
        // The reported flow end to end: hover → `+` → type → Enter (input
        // closes once the comment lands; the bubble stays self-expanded to
        // show it) → ✓ on that one comment.
        const { container, commitSubtreeEdit, rerenderWith } = mount([]);
        addFirstCommentViaPlus(container, commitSubtreeEdit, rerenderWith, 'Comment');

        // Pointer leaves the block before the resolve so hover can't keep
        // the chrome alive on its own.
        fireEvent.mouseLeave(wrapper(container));
        expect(resolveAt(container, commitSubtreeEdit, 0)).toEqual([]);

        rerenderWith([]);

        // Not hovering, nothing to show: no chrome at all (today: an empty
        // bubble titled "0 comments" with zero children lingers).
        expect(chrome(container)).toBeNull();
        expect(para(container).textContent).toBe('Stuff.');
    });

    it('T2: the block glow clears on the next pointer move over the block', () => {
        const { container, commitSubtreeEdit, rerenderWith } = mount(['Comment']);
        expandByClick(container);
        // Pointer enters the bubble and rests on the ✓ button.
        fireEvent.mouseEnter(bubble(container));
        fireEvent.mouseMove(resolveButtons(container)[0], { clientX: 50, clientY: 50 });
        expect(wrapperGlows(container)).toBe(true);
        // Keep the bubble's rect around the pointer so the stationary-pointer
        // re-check (T6) does not clear the glow here; this test isolates the
        // pointer-move path.
        stubRect(bubble(container), { left: 0, top: 0, right: 100, bottom: 100 });

        resolveAt(container, commitSubtreeEdit, 0);
        rerenderWith([]);

        // First movement after the re-render lands on the paragraph, not the
        // (now tiny / gone) bubble — the glow must follow the pointer.
        fireEvent.mouseMove(para(container), { clientX: 10, clientY: 50 });
        expect(wrapperGlows(container)).toBe(false);
    });

    it('T3: the block glow clears when the pointer leaves the block', () => {
        const { container, commitSubtreeEdit, rerenderWith } = mount(['Comment']);
        expandByClick(container);
        fireEvent.mouseEnter(bubble(container));
        fireEvent.mouseMove(resolveButtons(container)[0], { clientX: 50, clientY: 50 });
        expect(wrapperGlows(container)).toBe(true);
        stubRect(bubble(container), { left: 0, top: 0, right: 100, bottom: 100 });

        resolveAt(container, commitSubtreeEdit, 0);
        rerenderWith([]);

        fireEvent.mouseLeave(wrapper(container));
        expect(wrapperGlows(container)).toBe(false);
    });

    it('T4: resolving one of two comments keeps the bubble expanded', () => {
        const { container, commitSubtreeEdit, rerenderWith } = mount(['First', 'Second']);
        expandByClick(container);
        expect(resolveButtons(container)).toHaveLength(2);
        expect(resolveAt(container, commitSubtreeEdit, 0)).toEqual(['Second']);

        rerenderWith(['Second']);

        // Still self-expanded: one row with its ✓, showing the survivor.
        expect(resolveButtons(container)).toHaveLength(1);
        expect(bubble(container).textContent).toContain('Second');
        expect(bubble(container).textContent).not.toContain('First');
    });

    it('T5: the + add flow still opens an (empty, expanded) bubble with the input', () => {
        const { container, rerenderWith } = mount([]);
        // Hover the block's right half → `+` (jsdom rects are all zero, so
        // any non-negative clientX counts as the right half).
        fireEvent.mouseMove(para(container), { clientX: 10, clientY: 5 });
        expect(bubble(container).textContent).toBe('+');

        fireEvent.click(bubble(container));
        expect(container.querySelector('textarea.q2-comment-input')).not.toBeNull();

        // A host re-render with no comment (nothing committed yet) must not
        // collapse the open input: the "empty + expanded" state is legitimate
        // while the input is open.
        rerenderWith([]);
        expect(container.querySelector('textarea.q2-comment-input')).not.toBeNull();
        expect(chrome(container)).not.toBeNull();
    });

    it('T6: the block glow clears with the pointer stationary when the bubble shrinks away from it', () => {
        const { container, commitSubtreeEdit, rerenderWith } = mount(['First', 'Second']);
        expandByClick(container);
        fireEvent.mouseEnter(bubble(container));
        // Pointer rests on the first ✓ at (150, 20) inside a 200×60 bubble.
        stubRect(bubble(container), { left: 0, top: 0, right: 200, bottom: 60 });
        fireEvent.mouseMove(resolveButtons(container)[0], { clientX: 150, clientY: 20 });
        expect(wrapperGlows(container)).toBe(true);

        // After the resolve the bubble re-measures to a shape that no longer
        // contains the pointer (the row under the cursor is gone).
        resolveAt(container, commitSubtreeEdit, 0);
        stubRect(bubble(container), { left: 0, top: 0, right: 200, bottom: 10 });
        rerenderWith(['Second']);

        // No further pointer events: the glow must already be off.
        expect(wrapperGlows(container)).toBe(false);
    });

    it('T7: after the last resolve, a pointer resting inside the `+` chrome keeps the glow on', () => {
        // Mirrors the browser: the ✓ sat at the block's top-right corner,
        // exactly where the `+` affordance renders once the bubble
        // collapses. Geometry differs per intermediate state — the empty
        // expanded pill would NOT contain the pointer, the `+` does.
        const { container, commitSubtreeEdit, rerenderWith } = mount([]);
        addFirstCommentViaPlus(container, commitSubtreeEdit, rerenderWith, 'Comment');
        const b = bubble(container);
        b.getBoundingClientRect = () => {
            const r = b.textContent === '+'
                ? { left: 0, top: 0, right: 200, bottom: 60 } // contains (150, 20)
                : { left: 0, top: 0, right: 14, bottom: 6 }; // the pill: does not
            return { ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON() {} } as DOMRect;
        };
        fireEvent.mouseMove(resolveButtons(container)[0], { clientX: 150, clientY: 20 });
        expect(wrapperGlows(container)).toBe(true);

        resolveAt(container, commitSubtreeEdit, 0);
        rerenderWith([]);

        // Collapsed to the hover-only `+` (block still hovered), and the
        // glow reflects the pointer's real position inside it.
        expect(bubble(container).textContent).toBe('+');
        expect(wrapperGlows(container)).toBe(true);
    });

    it('T6b: the glow stays on when the pointer is still inside the re-measured bubble', () => {
        const { container, commitSubtreeEdit, rerenderWith } = mount(['First', 'Second']);
        expandByClick(container);
        fireEvent.mouseEnter(bubble(container));
        stubRect(bubble(container), { left: 0, top: 0, right: 200, bottom: 60 });
        fireEvent.mouseMove(resolveButtons(container)[0], { clientX: 150, clientY: 5 });
        expect(wrapperGlows(container)).toBe(true);

        resolveAt(container, commitSubtreeEdit, 0);
        stubRect(bubble(container), { left: 0, top: 0, right: 200, bottom: 30 });
        rerenderWith(['Second']);

        expect(wrapperGlows(container)).toBe(true);
    });
});
