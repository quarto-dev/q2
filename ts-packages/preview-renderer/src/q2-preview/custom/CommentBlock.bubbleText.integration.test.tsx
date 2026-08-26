/**
 * bd-wcz4x7y0 — comment bubbles must show the comment's full text.
 *
 * `[>> Hello "world"]` parses (with smart typography) to a
 * `quarto-edit-comment` span containing
 * `[Str "Hello", Space, Quoted DoubleQuote [Str "world"]]`. The bubble
 * used a local stringifier that mapped every non-Str/Space inline to
 * the empty string, so quoted words (and Emph/Code/... content)
 * silently vanished from the rendered comment. The bubble must render
 * the Pandoc-stringify text of the span: `Hello “world”`.
 *
 * Plan: claude-notes/plans/2026-08-26-comment-bubble-quoted-inlines.md
 */

// @vitest-environment jsdom

import { describe, it, expect, afterEach, vi } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import React from 'react';
import { Ast } from '../../framework';
import { previewRegistry } from '../registry';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
    vi.restoreAllMocks();
});

function commentSpan(inlines: unknown[]): unknown {
    return { t: 'Span', c: [['', ['quarto-edit-comment'], []], inlines] };
}

function astJson(commentInlines: unknown[]): string {
    const blocks = [
        {
            t: 'Para',
            s: 0,
            c: [{ t: 'Str', c: 'Stuff.' }, commentSpan(commentInlines)],
        },
    ];
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: [{ t: 0, r: [0, 24], d: 0 }] },
    });
}

function mountWithComment(commentInlines: unknown[]) {
    return render(
        <Ast
            astJson={astJson(commentInlines)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={() => {}}
            setAst={() => {}}
            registry={previewRegistry}
        />,
    );
}

/** The compact bubble div carries title="1 comment". */
function bubbleText(container: HTMLElement): string {
    const bubble = container.querySelector('[title="1 comment"]');
    expect(bubble).not.toBeNull();
    return bubble!.textContent ?? '';
}

describe('CommentBlock bubble text (bd-wcz4x7y0)', () => {
    it('renders Quoted inline content with its quote marks', () => {
        const { container } = mountWithComment([
            { t: 'Str', c: 'Hello' },
            { t: 'Space' },
            { t: 'Quoted', c: [{ t: 'DoubleQuote' }, [{ t: 'Str', c: 'world' }]] },
        ]);
        expect(bubbleText(container)).toBe('Hello “world”');
        // The stripped paragraph keeps its own text and not the comment's.
        expect(container.querySelector('p')!.textContent).toBe('Stuff.');
    });

    it('renders single-quoted content with single quote marks', () => {
        const { container } = mountWithComment([
            { t: 'Quoted', c: [{ t: 'SingleQuote' }, [{ t: 'Str', c: 'tis' }]] },
        ]);
        expect(bubbleText(container)).toBe('‘tis’');
    });

    it('keeps the text of Emph and Code inlines', () => {
        const { container } = mountWithComment([
            { t: 'Str', c: 'use' },
            { t: 'Space' },
            { t: 'Emph', c: [{ t: 'Str', c: 'this' }] },
            { t: 'Space' },
            { t: 'Code', c: [['', [], []], 'now()'] },
        ]);
        expect(bubbleText(container)).toBe('use this now()');
    });
});

// ---------------------------------------------------------------------
// bd-y66gbfs4 — rich inline rendering in the bubble.
// Plan: claude-notes/plans/2026-08-26-rich-comment-bubbles.md

/** The bubble element (compact chip carries title="1 comment"). */
function bubbleEl(container: HTMLElement): HTMLElement {
    const bubble = container.querySelector('[title="1 comment"]');
    expect(bubble).not.toBeNull();
    return bubble as HTMLElement;
}

function mountExpandedWithComment(commentInlines: unknown[]) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: 'expand',
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson(commentInlines)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
}

describe('CommentBlock rich bubble content (bd-y66gbfs4)', () => {
    it('renders Emph and Code as real elements in the compact bubble', () => {
        const { container } = mountWithComment([
            { t: 'Str', c: 'use' },
            { t: 'Space' },
            { t: 'Emph', c: [{ t: 'Str', c: 'this' }] },
            { t: 'Space' },
            { t: 'Code', c: [['', [], []], 'now()'] },
        ]);
        const bubble = bubbleEl(container);
        const em = bubble.querySelector('em');
        const code = bubble.querySelector('code');
        expect(em?.textContent).toBe('this');
        expect(code?.textContent).toBe('now()');
        expect(bubble.textContent).toBe('use this now()');
    });

    it('still shows curly quotes for Quoted (regression guard on bd-wcz4x7y0)', () => {
        const { container } = mountWithComment([
            { t: 'Str', c: 'Hello' },
            { t: 'Space' },
            { t: 'Quoted', c: [{ t: 'DoubleQuote' }, [{ t: 'Str', c: 'world' }]] },
        ]);
        expect(bubbleEl(container).textContent).toBe('Hello “world”');
    });

    it('renders rich elements in expanded rows too', () => {
        const { container } = mountExpandedWithComment([
            { t: 'Emph', c: [{ t: 'Str', c: 'row' }] },
        ]);
        const bubble = bubbleEl(container);
        expect(bubble.querySelector('em')?.textContent).toBe('row');
    });

    describe('links', () => {
        it('renders an anchor and routes external clicks to a new tab without expanding the bubble', () => {
            const openSpy = vi.spyOn(window, 'open').mockImplementation(() => null);
            const { container } = mountWithComment([
                { t: 'Str', c: 'see' },
                { t: 'Space' },
                {
                    t: 'Link',
                    c: [['', [], []], [{ t: 'Str', c: 'docs' }], ['https://example.com/', '']],
                },
            ]);
            const bubble = bubbleEl(container);
            const a = bubble.querySelector('a');
            expect(a).not.toBeNull();
            expect(a!.getAttribute('href')).toBe('https://example.com/');

            const ev = new MouseEvent('click', { bubbles: true, cancelable: true });
            a!.dispatchEvent(ev);
            expect(openSpy).toHaveBeenCalledWith(
                'https://example.com/',
                '_blank',
                'noopener,noreferrer',
            );
            expect(ev.defaultPrevented).toBe(true);
            // The link click must not act as a bubble click (which would
            // self-expand and open the inline comment input).
            expect(container.querySelector('textarea.q2-comment-input')).toBeNull();
        });

        it('clicking the bubble chrome away from a link still expands (link guard is scoped)', () => {
            const { container } = mountWithComment([
                { t: 'Str', c: 'plain' },
            ]);
            fireEvent.click(bubbleEl(container));
            expect(container.querySelector('textarea.q2-comment-input')).not.toBeNull();
        });
    });

    describe('image containment', () => {
        const IMG_COMMENT = [
            {
                t: 'Image',
                c: [['', [], []], [{ t: 'Str', c: 'alt' }], ['sketch.png', '']],
            },
        ];

        it('renders a clamped <img> inside a width-contained bubble', () => {
            const { container } = mountWithComment(IMG_COMMENT);
            const bubble = bubbleEl(container);
            expect(bubble.classList.contains('q2-comment-bubble')).toBe(true);
            expect(bubble.querySelector('img')).not.toBeNull();
            // Chip-level containment: no descendant may widen the chip.
            expect(bubble.style.maxWidth).not.toBe('');
            expect(bubble.style.overflow).toBe('hidden');
            // Scoped clamp rule injected once per document.
            const styles = Array.from(
                document.head.querySelectorAll('style[data-q2-comment-styles]'),
            ).map((s) => s.textContent ?? '').join('\n');
            expect(styles).toContain('.q2-comment-bubble img');
            expect(styles).toContain('max-width: 100%');
        });

        it('schedules a bubble relayout when an image loads', async () => {
            const { container } = mountWithComment(IMG_COMMENT);
            const img = bubbleEl(container).querySelector('img')!;
            // Flush the mount-time relayout so its scheduled-guard is clear.
            await new Promise<void>((r) => requestAnimationFrame(() => r()));
            const rafSpy = vi.spyOn(window, 'requestAnimationFrame');
            fireEvent.load(img);
            expect(rafSpy).toHaveBeenCalled();
        });
    });

    describe('unsupported content chips', () => {
        const CHIP = '[unsupported content]';
        const chipCount = (bubble: HTMLElement) =>
            (bubble.textContent ?? '').split(CHIP).length - 1;

        it('replaces a nested comment span with exactly one chip and none of its text', () => {
            const { container } = mountWithComment([
                { t: 'Str', c: 'outer' },
                { t: 'Space' },
                {
                    t: 'Span',
                    c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: 'inner-secret' }]],
                },
            ]);
            const bubble = bubbleEl(container);
            expect(chipCount(bubble)).toBe(1);
            expect(bubble.textContent).not.toContain('inner-secret');
            expect(bubble.textContent).toContain('outer');
        });

        it.each([
            ['quarto-insert'],
            ['quarto-delete'],
            ['quarto-highlight'],
        ])('replaces a %s span with a chip', (cls) => {
            const { container } = mountWithComment([
                {
                    t: 'Span',
                    c: [['', [cls], []], [{ t: 'Str', c: 'hidden' }]],
                },
            ]);
            const bubble = bubbleEl(container);
            expect(chipCount(bubble)).toBe(1);
            expect(bubble.textContent).not.toContain('hidden');
        });

        it('replaces a Note with a chip', () => {
            const { container } = mountWithComment([
                {
                    t: 'Note',
                    c: [{ t: 'Para', c: [{ t: 'Str', c: 'footnote-body' }] }],
                },
            ]);
            const bubble = bubbleEl(container);
            expect(chipCount(bubble)).toBe(1);
            expect(bubble.textContent).not.toContain('footnote-body');
        });

        it('intercepts marks nested inside Emph (registry recursion, not top-level filtering)', () => {
            const { container } = mountWithComment([
                {
                    t: 'Emph',
                    c: [
                        { t: 'Str', c: 'emph' },
                        { t: 'Space' },
                        {
                            t: 'Span',
                            c: [['', ['quarto-highlight'], []], [{ t: 'Str', c: 'hidden' }]],
                        },
                    ],
                },
            ]);
            const bubble = bubbleEl(container);
            expect(chipCount(bubble)).toBe(1);
            expect(bubble.querySelector('em')).not.toBeNull();
            expect(bubble.textContent).not.toContain('hidden');
        });

        it('leaves an ordinary classed span alone (interceptor is class-scoped)', () => {
            const { container } = mountWithComment([
                {
                    t: 'Span',
                    c: [['', ['mark'], []], [{ t: 'Str', c: 'visible' }]],
                },
            ]);
            const bubble = bubbleEl(container);
            expect(chipCount(bubble)).toBe(0);
            expect(bubble.textContent).toContain('visible');
        });
    });
});
