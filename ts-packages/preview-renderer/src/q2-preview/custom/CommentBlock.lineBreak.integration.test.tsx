/**
 * bd-ov4jydyf — Shift+Enter in the comment input starts a new line,
 * committed as a Pandoc `LineBreak` (qmd `\`+newline) inside the
 * `[>> …]` span.
 *
 *  - Enter (no Shift) still submits; Shift+Enter is left to the browser,
 *    which inserts "\n" into the textarea.
 *  - The Enter that confirms an IME composition does not submit.
 *  - Inside a heading (one source line), Shift+Enter does nothing and a
 *    pasted new line commits as a space.
 *  - The compact bubble shows a line break as a space; the expanded
 *    bubble shows it as a <br>.
 *
 * Plan: claude-notes/plans/2026-11-16-comment-shift-enter-linebreak.md
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

// --- AST builders -----------------------------------------------------------

type Pool = { t: 0; r: [number, number]; d: number }[];

const S = (c: string) => ({ t: 'Str', c });
const SP = { t: 'Space' };
const LB = { t: 'LineBreak' };
const comment = (...inlines: unknown[]) => ({
    t: 'Span',
    c: [['', ['quarto-edit-comment'], []], inlines],
});
const isCommentSpan = (n: any) =>
    n?.t === 'Span' && (n.c?.[0]?.[1] ?? []).includes('quarto-edit-comment');

/** Give every node that has `s: 'auto'` its own pool entry. */
function withPool(blocks: any[]): { blocks: any[]; pool: Pool } {
    const pool: Pool = [];
    const walk = (x: any): any => {
        if (Array.isArray(x)) return x.map(walk);
        if (x && typeof x === 'object') {
            const out: any = {};
            for (const [k, v] of Object.entries(x)) out[k] = walk(v);
            if (out.s === 'auto') {
                out.s = pool.length;
                pool.push({ t: 0, r: [pool.length * 100, pool.length * 100 + 99], d: 0 });
            }
            return out;
        }
        return x;
    };
    return { blocks: walk(blocks), pool };
}

function mount(rawBlocks: any[]) {
    const { blocks, pool } = withPool(rawBlocks);
    const commitSubtreeEdit = vi.fn();
    const resolveSource = (node: any): ResolvedSource | null => {
        if (typeof node?.s !== 'number') return null;
        return {
            sourceNode: node,
            reachabilityClass: 'TopLevel',
            sourceEntry: pool[node.s],
            sourcePool: pool,
        } as ResolvedSource;
    };
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: 'show',
        sourceIndex: new Map(),
        pool,
        resolveSource,
        commitSubtreeEdit,
    } as PreviewContextValue;
    const astJson = JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: pool },
    });
    const result = render(
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
    return { ...result, commitSubtreeEdit };
}

// --- DOM helpers ------------------------------------------------------------

const bubbles = () => [...document.body.querySelectorAll<HTMLElement>('.q2-comment-bubble')];
const input = () => document.body.querySelector<HTMLTextAreaElement>('textarea.q2-comment-input');

/** Hover `el` so its `+` appears, then click the (only) bubble to open the input. */
function openInputOn(el: Element) {
    fireEvent.mouseMove(el, { clientX: 10, clientY: 5 });
    const b = bubbles();
    expect(b).toHaveLength(1);
    fireEvent.click(b[0]);
    expect(input()).not.toBeNull();
    return input()!;
}

/** Click the (only) existing bubble, which expands it and opens the input. */
function openInputOnBubble() {
    const b = bubbles();
    expect(b).toHaveLength(1);
    fireEvent.click(b[0]);
    expect(input()).not.toBeNull();
    return input()!;
}

/** Simulate typing: the textarea's value is what the user typed (with "\n" for Shift+Enter). */
function type(ta: HTMLTextAreaElement, value: string) {
    fireEvent.change(ta, { target: { value } });
}

/** keyDown; returns true when the handler did NOT prevent the default. */
const key = (ta: HTMLTextAreaElement, init: Record<string, unknown>) => fireEvent.keyDown(ta, init);

/** The comment spans (by inlines) in the block committed by the n-th commit. */
function committedComments(commit: ReturnType<typeof vi.fn>, n = 0): any[][] {
    const block = commit.mock.calls[n][1];
    const found: any[] = [];
    const walk = (x: any) => {
        if (Array.isArray(x)) x.forEach(walk);
        else if (x && typeof x === 'object') {
            if (isCommentSpan(x)) found.push(x);
            else walk(x.c);
        }
    };
    walk(block);
    return found.map((s) => s.c[1]);
}

const hint = () =>
    [...document.body.querySelectorAll('div')].find((d) => /add · /.test(d.textContent ?? '') && d.children.length === 0)
        ?.textContent;

// --- Tests: block comments --------------------------------------------------

describe('comment input: Shift+Enter (bd-ov4jydyf)', () => {
    it('B1: Shift+Enter does not submit and leaves the newline to the browser', () => {
        const { commitSubtreeEdit, container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        const ta = openInputOn(container.querySelector('p')!);
        type(ta, 'line one');
        const notPrevented = key(ta, { key: 'Enter', shiftKey: true });
        expect(notPrevented).toBe(true);
        expect(commitSubtreeEdit).not.toHaveBeenCalled();
    });

    it('B2: Enter after a new line commits a LineBreak between the lines', () => {
        const { commitSubtreeEdit, container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        const ta = openInputOn(container.querySelector('p')!);
        type(ta, 'line one\nline two');
        const notPrevented = key(ta, { key: 'Enter' });
        expect(notPrevented).toBe(false);
        expect(commitSubtreeEdit).toHaveBeenCalledTimes(1);
        expect(committedComments(commitSubtreeEdit)).toEqual([
            [S('line'), SP, S('one'), LB, S('line'), SP, S('two')],
        ]);
    });

    it('B3: a one-line comment has no LineBreak', () => {
        const { commitSubtreeEdit, container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        const ta = openInputOn(container.querySelector('p')!);
        type(ta, 'just one line');
        key(ta, { key: 'Enter' });
        expect(committedComments(commitSubtreeEdit)).toEqual([
            [S('just'), SP, S('one'), SP, S('line')],
        ]);
    });

    it('B3b: whitespace-only input does not commit', () => {
        const { commitSubtreeEdit, container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        const ta = openInputOn(container.querySelector('p')!);
        type(ta, ' \n  \n');
        key(ta, { key: 'Enter' });
        expect(commitSubtreeEdit).not.toHaveBeenCalled();
    });

    it('B4: in a heading, Shift+Enter is ignored and a pasted new line commits as a space', () => {
        const { commitSubtreeEdit, container } = mount([
            { t: 'Header', s: 'auto', c: [1, ['h', [], []], [S('Title')]] },
        ]);
        const ta = openInputOn(container.querySelector('h1')!);
        type(ta, 'a');
        const notPrevented = key(ta, { key: 'Enter', shiftKey: true });
        expect(notPrevented).toBe(false);
        expect(commitSubtreeEdit).not.toHaveBeenCalled();
        expect(hint()).toBe('↵ add · Esc close');
        type(ta, 'a\nb');
        key(ta, { key: 'Enter' });
        expect(committedComments(commitSubtreeEdit)).toEqual([[S('a'), SP, S('b')]]);
    });

    it('B4b: outside a heading, the hint mentions Shift+Enter', () => {
        const { container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        openInputOn(container.querySelector('p')!);
        expect(hint()).toBe('↵ add · ⇧↵ new line · Esc close');
    });

    it('B5: a comment on a code block keeps its LineBreak inside the container', () => {
        const { commitSubtreeEdit, container } = mount([
            { t: 'CodeBlock', s: 'auto', c: [['', [], []], 'x = 1'] },
        ]);
        const ta = openInputOn(container.querySelector('pre')!);
        type(ta, 'a\nb');
        key(ta, { key: 'Enter' });
        const block = commitSubtreeEdit.mock.calls[0][1];
        expect(block.t).toBe('Div');
        expect(block.c[0][1]).toEqual(['quarto-edit-comment-container']);
        expect(committedComments(commitSubtreeEdit)).toEqual([[S('a'), LB, S('b')]]);
    });

    it('B6: appending to a paragraph that already has a comment keeps both', () => {
        const { commitSubtreeEdit } = mount([
            { t: 'Para', s: 'auto', c: [S('Text.'), comment(S('old'))] },
        ]);
        const ta = openInputOnBubble();
        type(ta, 'new\nlines');
        key(ta, { key: 'Enter' });
        expect(committedComments(commitSubtreeEdit)).toEqual([
            [S('old')],
            [S('new'), LB, S('lines')],
        ]);
    });

    it('B7: the Enter that confirms an IME composition does not submit', () => {
        const { commitSubtreeEdit, container } = mount([{ t: 'Para', s: 'auto', c: [S('Text.')] }]);
        const ta = openInputOn(container.querySelector('p')!);
        type(ta, '日本');
        key(ta, { key: 'Enter', isComposing: true });
        // Safari reports the confirming Enter as keyCode 229.
        key(ta, { key: 'Enter', keyCode: 229 });
        expect(commitSubtreeEdit).not.toHaveBeenCalled();
        // The next, ordinary Enter submits.
        key(ta, { key: 'Enter' });
        expect(commitSubtreeEdit).toHaveBeenCalledTimes(1);
    });
});

// --- Tests: display ---------------------------------------------------------

describe('comment display with line breaks (bd-ov4jydyf)', () => {
    it('B8: compact bubble shows a line break as a space; expanded shows a <br>', () => {
        mount([{ t: 'Para', s: 'auto', c: [S('Text.'), comment(S('a'), LB, S('b'))] }]);
        const compact = bubbles()[0];
        expect(compact.querySelector('br')).toBeNull();
        expect(compact.textContent).toBe('a b');
        fireEvent.click(compact);
        expect(bubbles()[0].querySelectorAll('br')).toHaveLength(1);
    });
});

// --- Tests: span comments (CommentSpan.tsx) ---------------------------------

describe('span comments: Shift+Enter (bd-ov4jydyf)', () => {
    const spanned = (...inl: unknown[]) => ({ t: 'Span', s: 'auto', c: [['', [], []], inl] });

    it('B9: a span comment in a paragraph commits a LineBreak', () => {
        const { commitSubtreeEdit } = mount([
            { t: 'Para', s: 'auto', c: [S('Before'), SP, spanned(S('range'), comment(S('old'))), SP, S('after')] },
        ]);
        const ta = openInputOnBubble();
        type(ta, 'a\nb');
        key(ta, { key: 'Enter' });
        expect(committedComments(commitSubtreeEdit)).toEqual([[S('old')], [S('a'), LB, S('b')]]);
    });

    it('B10: a span comment in a heading commits a space, and Shift+Enter is ignored', () => {
        const { commitSubtreeEdit } = mount([
            { t: 'Header', s: 'auto', c: [1, ['h', [], []], [S('T'), SP, spanned(S('range'), comment(S('old')))]] },
        ]);
        const ta = openInputOnBubble();
        type(ta, 'a');
        expect(key(ta, { key: 'Enter', shiftKey: true })).toBe(false);
        type(ta, 'a\nb');
        key(ta, { key: 'Enter' });
        expect(committedComments(commitSubtreeEdit)).toEqual([[S('old')], [S('a'), SP, S('b')]]);
    });
});
