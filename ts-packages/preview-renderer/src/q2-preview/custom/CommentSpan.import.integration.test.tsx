/**
 * P0 (temporary carry of Elliot's span comments, document import I14):
 * the shapes the Word import produces, rendered through the span-comment
 * chrome (`CommentSpan.tsx`). Import writes a Word comment as a plain span
 * whose last child is a `quarto-edit-comment` span (epic I4).
 *
 * Removed together with the rest of P0 at the epic's Close-out.
 */
import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { Ast } from '../../framework';
import { previewRegistry } from '../registry';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';
import type { ResolvedSource } from '../sourceIndex';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
});

// --- AST builders -----------------------------------------------------------

// Every commentable block gets its own pool entry so `resolveSource` can
// hand it back as a committable TopLevel node.
const POOL: { t: 0; r: [number, number]; d: number }[] = [];
function pooled<T extends object>(node: T): T & { s: number } {
    const s = POOL.length;
    POOL.push({ t: 0, r: [s * 10, s * 10 + 9], d: 0 });
    return { ...node, s };
}

const str = (c: string) => ({ t: 'Str', c });
const comment = (text: string) => ({
    t: 'Span',
    c: [['', ['quarto-edit-comment'], []], [str(text)]],
});
const para = (...inlines: unknown[]) => pooled({ t: 'Para', c: inlines });

function astJson(blocks: unknown[]): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: POOL },
    });
}

const resolveSource = (node: any): ResolvedSource | null => {
    if (node?.s === undefined) return null;
    return {
        sourceNode: node,
        reachabilityClass: 'TopLevel',
        sourceEntry: POOL[Number(node.s)],
    };
};

function mount(blocks: unknown[], withContext = true) {
    const ast = (
        <Ast
            astJson={astJson(blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={() => {}}
            setAst={() => {}}
            registry={previewRegistry}
        />
    );
    if (!withContext) return render(ast);
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: 'show',
        resolveSource,
        commitSubtreeEdit: () => {},
    };
    return render(<PreviewContext.Provider value={ctx}>{ast}</PreviewContext.Provider>);
}

// --- DOM helpers ------------------------------------------------------------

const layer = () => document.querySelector('[data-q2-comment-layer]');
const bubbles = () => [...document.querySelectorAll<HTMLElement>('.q2-comment-bubble')];


const span = (classes: string[], ...inl: unknown[]) => ({ t: 'Span', c: [['', classes, []], inl] });
const sp = { t: 'Space' };

const reply = (text: string) => comment(text);

describe('P0 import shapes: comment inside a plain span', () => {
    it('gives one bubble; the span text excludes the comment; the span is .q2-commented-span', () => {
        const { container } = mount([
            para(str('Before'), sp, span([], str('range'), sp, comment('on-plain-span')), sp, str('after')),
        ]);
        expect(bubbles().map((b) => b.textContent)).toEqual(['on-plain-span']);
        const commented = container.querySelectorAll('.q2-commented-span');
        expect(commented).toHaveLength(1);
        // The Space that preceded the stripped comment stays in the span's text.
        expect(commented[0].textContent).toBe('range ');
        expect(container.querySelector('p')?.textContent).toBe('Before range  after');
        expect(container.querySelectorAll('.quarto-edit-comment')).toHaveLength(0);
    });

    it('a comment with author/date attributes behaves the same', () => {
        const attributed = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], [['author', 'Ada'], ['date', '2026-09-01']]], [str('with-meta')]],
        };
        const { container } = mount([
            para(str('Before'), sp, span([], str('range'), sp, attributed), sp, str('after')),
        ]);
        expect(bubbles().map((b) => b.textContent)).toEqual(['with-meta']);
        const commented = container.querySelectorAll('.q2-commented-span');
        expect(commented).toHaveLength(1);
        expect(commented[0].textContent).toBe('range ');
        expect(container.querySelector('p')?.textContent).toBe('Before range  after');
    });

    // CHARACTERIZATION ONLY: Elliot's branch predates I4's reply shape (a
    // comment and its reply as two trailing comment spans), so no outcome
    // is expected. This records what renders today.
    it('characterization: a wrapper ending in two comment spans (comment + reply)', () => {
        const { container } = mount([
            para(str('Before'), sp, span([], str('range'), sp, comment('first'), reply('second')), sp, str('after')),
        ]);
        // Observed: both comments collapse into ONE bubble ("first" plus a
        // "+1 more" affordance); the span is commented; nothing stays inline.
        expect(bubbles().map((b) => b.textContent)).toEqual(['first+1 more']);
        expect(container.querySelectorAll('.q2-commented-span')).toHaveLength(1);
        expect(container.querySelectorAll('.quarto-edit-comment')).toHaveLength(0);
    });
});

describe('P0 import shapes: other placements', () => {
    it('a comment directly in a paragraph still gives a bubble', () => {
        const { container } = mount([para(str('Body'), sp, comment('direct'))]);
        expect(bubbles().map((b) => b.textContent)).toEqual(['direct']);
        expect(container.querySelectorAll('.q2-commented-span')).toHaveLength(0);
    });

    it('documents the exclusion: a comment inside a quarto-highlight span stays inline', () => {
        const { container } = mount([
            para(str('Before'), sp, span(['quarto-highlight'], str('range'), sp, comment('on-highlight')), sp, str('after')),
        ]);
        // quarto-* spans are left alone (CommentSpan.tsx scope): no bubble, no
        // commented span, and the comment text renders inline.
        expect(bubbles()).toHaveLength(0);
        expect(container.querySelectorAll('.q2-commented-span')).toHaveLength(0);
        expect(container.querySelectorAll('.quarto-edit-comment')).toHaveLength(1);
        expect(container.querySelector('p')?.textContent).toBe('Before range on-highlight after');
    });
});
