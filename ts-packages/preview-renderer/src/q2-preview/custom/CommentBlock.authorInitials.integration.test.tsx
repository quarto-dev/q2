/**
 * GH #977 — expanded comment rows pair the author's color dot with
 * initials derived from the attribution identity's name, so authorship
 * doesn't rely on color alone.
 *
 * Plan: claude-notes/plans/2026-10-31-comment-author-initials.md
 */

// @vitest-environment jsdom

import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { Ast } from '../../framework';
import { previewRegistry } from '../registry';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
});

const ACTOR = 'b4fa8892a6ed9656a2c2f390ef22b92d';

function mount(identity: { name: string; color: string } | null) {
    const astJson = JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'Para',
                s: 0,
                c: [
                    { t: 'Str', c: 'Stuff.' },
                    {
                        t: 'Span',
                        s: 1,
                        c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: 'Hello' }]],
                    },
                ],
            },
        ],
        astContext: {
            p: [
                { t: 0, r: [0, 24], d: 0 },
                { t: 0, r: [6, 20], d: 0 },
            ],
            ...(identity
                ? {
                      attribution: [{ s: 1, actor: ACTOR, time: 1 }],
                      attributionActors: { [ACTOR]: identity },
                  }
                : {}),
        },
    });
    const preview: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: 'expand',
    };
    return render(
        <PreviewContext.Provider value={preview}>
            <Ast
                astJson={astJson}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
}

const initialsEl = () => document.body.querySelector('[data-q2-comment-author-initials]');

describe('CommentBlock author initials (GH #977)', () => {
    it('shows initials derived from the identity name', () => {
        mount({ name: 'Charlie Gao', color: '#7B1FA2' });
        expect(initialsEl()?.textContent).toBe('CG');
        expect(initialsEl()?.getAttribute('title')).toBe('Charlie Gao');
    });

    it('keeps the dot alone for an actor with no profile name', () => {
        mount({ name: ACTOR.slice(0, 8), color: '#7B1FA2' });
        expect(initialsEl()).toBeNull();
        expect(document.body.querySelector(`[title="${ACTOR.slice(0, 8)}"]`)).not.toBeNull();
    });

    it('shows no initials when attribution is off', () => {
        mount(null);
        expect(document.body.textContent).toContain('Hello');
        expect(initialsEl()).toBeNull();
    });
});
