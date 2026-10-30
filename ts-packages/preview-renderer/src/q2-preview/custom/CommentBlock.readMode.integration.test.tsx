/**
 * Comments with the edit surface off (GH issue #973).
 *
 * `editingDisabled` governs CONTENT editing only. Comments are stored
 * in-band, but they are a separate UI affordance: hub-client's Edit pill
 * leaves them working, and tags their commits `scope: 'comment'` so the
 * host's read-mode guard can let them through. A host that truly cannot
 * write (`q2 preview` without `--allow-edit`) sets `commentingDisabled`,
 * which leaves existing comments readable but offers no way to add or
 * resolve one, so nothing typed can be silently lost.
 *
 * Real production path through PreviewRoot.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, act } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from '../PreviewRoot';
import type { PreviewRootProps } from '../PreviewRoot';

afterEach(() => {
    cleanup();
    document.body.innerHTML = '';
    vi.restoreAllMocks();
});

const COMMENT = { t: 'Span', c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: 'existing' }]] };
const CONTENT = 'hello tail\n';
const POOL = [
    { t: 0, r: [0, 11], d: 0 },
    { t: 0, r: [0, 5], d: 0 },
];

function astJson(inlines: unknown[]): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [{ t: 'Para', s: 0, c: inlines }],
        astContext: { p: POOL },
    });
}

function mount(inlines: unknown[], overrides: Partial<PreviewRootProps> = {}) {
    const setAst = vi.fn();
    const json = astJson(inlines);
    const result = render(
        <PreviewRoot
            astJson={json}
            untransformedAstJson={json}
            renderedContent={CONTENT}
            currentFilePath="/test.qmd"
            assetManifest={{}}
            setAst={setAst}
            onNavigateToDocument={() => {}}
            {...overrides}
        />,
    );
    return { ...result, setAst };
}

const bubble = () => document.body.querySelector<HTMLElement>('.q2-comment-bubble');
const input = () => document.body.querySelector<HTMLTextAreaElement>('textarea.q2-comment-input');
const resolveButton = () => document.body.querySelector<HTMLElement>('[title="Resolve comment"]');

/** Everything in jsdom measures zero, so any pointer position is the block's "right half". */
function hover(el: Element) {
    fireEvent.mouseMove(el, { clientX: 10, clientY: 10 });
}

async function openInput(anchor: Element) {
    await act(async () => {
        hover(anchor);
    });
    await act(async () => {
        fireEvent.click(bubble()!);
    });
    expect(input(), 'the add-comment input opens').not.toBeNull();
}

async function submit(text: string) {
    await act(async () => {
        fireEvent.change(input()!, { target: { value: text } });
    });
    await act(async () => {
        fireEvent.keyDown(input()!, { key: 'Enter' });
    });
}

describe('comments with Edit off (hub-client read mode, #973)', () => {
    const PLAIN = [{ t: 'Str', c: 'hello' }];

    it('adds a block comment through a comment-scoped commit, with no content edit surface', async () => {
        const { container, setAst } = mount(PLAIN, { editingDisabled: true });
        expect(container.querySelectorAll('[data-block-pool-id]')).toHaveLength(0);

        await openInput(container.querySelector('p')!);
        await submit('a reviewer note');

        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0];
        expect(payload).toMatchObject({ __isPreviewNodeEdit: true, channel: 'subtree', scope: 'comment' });
        expect(payload.modifiedSubtreeJson).toContain('a reviewer note');
    });

    it('resolves a comment through a comment-scoped commit', async () => {
        const { setAst } = mount([...PLAIN, COMMENT], { editingDisabled: true });
        await act(async () => {
            fireEvent.click(bubble()!);
        });
        await act(async () => {
            fireEvent.click(resolveButton()!);
        });
        expect(setAst).toHaveBeenCalledOnce();
        expect(setAst.mock.calls[0][0]).toMatchObject({ scope: 'comment' });
        expect(setAst.mock.calls[0][0].modifiedSubtreeJson).not.toContain('existing');
    });

    it('tags span comments too', async () => {
        const span = { t: 'Span', s: 1, c: [['', [], []], [{ t: 'Str', c: 'hello' }, COMMENT]] };
        const { setAst } = mount([span], { editingDisabled: true });
        await act(async () => {
            fireEvent.click(document.body.querySelectorAll<HTMLElement>('.q2-comment-bubble')[0]);
        });
        await submit('on the span');
        expect(setAst).toHaveBeenCalledOnce();
        expect(setAst.mock.calls[0][0]).toMatchObject({ scope: 'comment' });
    });
});

describe('commentingDisabled (host cannot write)', () => {
    const PLAIN = [{ t: 'Str', c: 'hello' }];
    const READ_ONLY = { editingDisabled: true, commentingDisabled: true };

    it('offers no add affordance on hover', async () => {
        const { container } = mount(PLAIN, READ_ONLY);
        await act(async () => {
            hover(container.querySelector('p')!);
        });
        expect(bubble()).toBeNull();
    });

    it('keeps existing comments readable, with no input or resolve button', async () => {
        const { setAst } = mount([...PLAIN, COMMENT], READ_ONLY);
        expect(bubble()?.textContent).toContain('existing');
        await act(async () => {
            fireEvent.click(bubble()!);
        });
        expect(bubble()?.textContent).toContain('existing');
        expect(input()).toBeNull();
        expect(resolveButton()).toBeNull();
        expect(setAst).not.toHaveBeenCalled();
    });
});
