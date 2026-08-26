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

import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { Ast } from '../../framework';
import { previewRegistry } from '../registry';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
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
