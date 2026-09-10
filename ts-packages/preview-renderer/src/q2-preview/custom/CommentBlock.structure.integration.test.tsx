/**
 * DOM-structure contract for the q2-preview comment chrome (bd-q2wqj24c).
 *
 * The theme CSS is written for the native HTML writer's DOM, where a block
 * is a DIRECT child of its container (`blockquote > h4`,
 * `.callout-body > :first-child`, `li > p:last-of-type`, …). The comment
 * chrome must therefore never put an element between a block and its
 * parent: `CommentBlock` renders the block untouched and portals its
 * bubble (and the block glow) into one body-level overlay layer,
 * `[data-q2-comment-layer]`, positioned from the block's measured rect.
 *
 * The DOM parity harness (`hub-client/src/services/smokeAllParity.wasm.test.tsx`)
 * mounts read-only — no `PreviewContext`, so no chrome — and is blind to
 * this class of bug; the "parity guard" block below is the check it lacks.
 */
import { describe, it, expect, afterEach } from 'vitest';
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
const plain = (...inlines: unknown[]) => pooled({ t: 'Plain', c: inlines });
const header = (level: number, id: string, ...inlines: unknown[]) =>
    pooled({ t: 'Header', c: [level, [id, [], []], inlines] });
const blockQuote = (...blocks: unknown[]) => pooled({ t: 'BlockQuote', c: blocks });
const bulletList = (...items: unknown[][]) => pooled({ t: 'BulletList', c: items });
const callout = (...content: unknown[]) => ({
    t: 'CustomBlock',
    type_name: 'Callout',
    slots: {
        title: { kind: 'inlines', value: [str('Note')] },
        content: { kind: 'blocks', value: content },
    },
    plain_data: { type: 'note', icon: true, appearance: 'default' },
    attr: ['', [], []],
});

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

/** Hover the right half of an element (jsdom rects are all zero, so any x ≥ 0 is the right half). */
function hover(el: Element) {
    fireEvent.mouseMove(el, { clientX: 10, clientY: 5 });
}

/**
 * Structural signature of a subtree: one line per element with its tag
 * and classes, indented by depth. Text, attributes other than `class`,
 * and comment nodes are ignored, so two mounts agree exactly when they
 * have the same element tree.
 */
function signature(root: Element): string {
    const out: string[] = [];
    const walk = (el: Element, depth: number) => {
        const cls = el.getAttribute('class');
        out.push(`${'  '.repeat(depth)}${el.tagName.toLowerCase()}${cls ? `.${cls.split(/\s+/).join('.')}` : ''}`);
        for (const child of el.children) walk(child, depth + 1);
    };
    walk(root, 0);
    return out.join('\n');
}

// --- Tests ------------------------------------------------------------------

describe('CommentBlock leaves the block a direct child of its parent (bd-q2wqj24c)', () => {
    it('blockquote > h4 and blockquote > p hold with no comments', () => {
        const { container } = mount([blockQuote(header(4, 'quoted', str('Quoted')), para(str('Body')))]);
        expect(container.querySelector('blockquote > h4#quoted')).not.toBeNull();
        expect(container.querySelector('blockquote > p')).not.toBeNull();
        expect(container.querySelector('blockquote > div')).toBeNull();
    });

    it('blockquote > h4 and blockquote > p hold when both carry comments', () => {
        const { container } = mount([
            blockQuote(
                header(4, 'quoted', str('Quoted'), comment('on the heading')),
                para(str('Body'), comment('on the body')),
            ),
        ]);
        expect(container.querySelector('blockquote > h4#quoted')).not.toBeNull();
        expect(container.querySelector('blockquote > p')).not.toBeNull();
        expect(container.querySelector('blockquote > div')).toBeNull();
        // Both bubbles exist — and live in the overlay layer, not in the article.
        expect(bubbles()).toHaveLength(2);
        expect(container.querySelector('.q2-comment-bubble')).toBeNull();
        for (const b of bubbles()) expect(b.closest('[data-q2-comment-layer]')).toBe(layer());
    });

    it('the comment text is stripped from the block and shown in the bubble', () => {
        const { container } = mount([para(str('Body'), comment('a remark'))]);
        expect(container.querySelector('p')!.textContent).toBe('Body');
        expect(bubbles()[0].textContent).toContain('a remark');
    });

    it('hovering a comment-less block shows the + bubble without inserting an element', () => {
        const { container } = mount([blockQuote(para(str('Body')))]);
        hover(container.querySelector('p')!);
        expect(bubbles()).toHaveLength(1);
        expect(bubbles()[0].textContent).toBe('+');
        expect(container.querySelector('blockquote > p')).not.toBeNull();
        expect(container.querySelector('blockquote > div')).toBeNull();
    });

    it('.callout-body > :first-child is the first body paragraph, comments or not', () => {
        const { container } = mount([callout(para(str('first'), comment('c')), para(str('last')))]);
        const body = container.querySelector('.callout-body-container.callout-body')!;
        expect(body).not.toBeNull();
        expect(body.firstElementChild!.tagName).toBe('P');
        expect(body.querySelector(':scope > p:first-child')!.textContent).toBe('first');
        expect(body.querySelector(':scope > p:last-child')!.textContent).toBe('last');
        hover(body.lastElementChild!);
        expect(body.children).toHaveLength(2);
        expect(bubbles()).toHaveLength(2);
    });

    it('li > p holds for loose list items with comments', () => {
        const { container } = mount([bulletList([para(str('a'))], [para(str('b'), comment('c'))])]);
        expect(container.querySelectorAll('li > p')).toHaveLength(2);
        expect(container.querySelectorAll('li > div')).toHaveLength(0);
        expect(bubbles()).toHaveLength(1);
    });

    it('a tight list item (Plain) keeps its text directly in the li and anchors its bubble to the li', () => {
        const { container } = mount([bulletList([plain(str('one'), comment('c'))], [plain(str('two'))])]);
        const items = container.querySelectorAll('li');
        expect(items).toHaveLength(2);
        expect(items[0].children).toHaveLength(0);
        expect(items[0].textContent).toBe('one');
        expect(bubbles()).toHaveLength(1);
        // Hovering the second item reveals its + affordance — the li is the
        // hover surface, and nothing is inserted into it.
        hover(items[1]);
        expect(bubbles()).toHaveLength(2);
        expect(items[1].children).toHaveLength(0);
    });

    it('a Plain with no host element renders passthrough with the comment span left in the text', () => {
        // A top-level Plain has no <li>/<dd>/<td> to anchor to. Rather than
        // stripping the comment and showing no bubble (silently losing it),
        // the block renders as-is: the span stays visible in the text.
        const { container } = mount([plain(str('loose'), comment('kept'))]);
        expect(container.querySelector('span.quarto-edit-comment')).not.toBeNull();
        expect(container.textContent).toContain('kept');
        expect(bubbles()).toHaveLength(0);
    });
});

describe('overlay layer', () => {
    it('is a single body-level element that holds every bubble and nothing else in the article', () => {
        mount([para(str('a'), comment('1')), para(str('b'), comment('2'))]);
        const layers = document.querySelectorAll('[data-q2-comment-layer]');
        expect(layers).toHaveLength(1);
        expect(layers[0].parentElement).toBe(document.body);
        expect(layers[0].querySelectorAll('.q2-comment-bubble')).toHaveLength(2);
    });

    it('does not render at all without comments or hover', () => {
        // Nothing to show, nothing mounted: the layer only appears once a
        // bubble needs it.
        mount([para(str('a'))]);
        expect(bubbles()).toHaveLength(0);
    });

    it('the glow is an overlay element, never a style on the block', () => {
        const { container } = mount([para(str('a'), comment('1'))]);
        const p = container.querySelector('p')!;
        // Pointer over the bubble: the block glows via an overlay outline.
        fireEvent.mouseMove(bubbles()[0], { clientX: 10, clientY: 5 });
        const glow = document.querySelector('[data-q2-comment-glow]');
        expect(glow).not.toBeNull();
        expect(glow!.closest('[data-q2-comment-layer]')).toBe(layer());
        expect(p.getAttribute('style')).toBeNull();
        // Pointer back on the block (outside the bubble): glow gone.
        fireEvent.mouseMove(p, { clientX: 10, clientY: 5 });
        expect(document.querySelector('[data-q2-comment-glow]')).toBeNull();
    });
});

describe('parity guard: chrome never changes the article element tree', () => {
    const DOC = [
        header(2, 'sec', str('Section')),
        blockQuote(header(4, 'quoted', str('Quoted')), para(str('Body'), comment('q'))),
        callout(para(str('first'), comment('c')), para(str('last'))),
        bulletList([plain(str('one'), comment('c'))], [plain(str('two'))]),
        bulletList([para(str('a'))], [para(str('b'), comment('c'))]),
    ];

    it('read-only mount and PreviewContext mount (comments shown, block hovered) have identical element trees', () => {
        const readOnly = mount(DOC, false);
        const readOnlySig = signature(readOnly.container);
        cleanup();
        document.body.innerHTML = '';

        const live = mount(DOC, true);
        // Every visible bubble plus a hovered comment-less block.
        hover(live.container.querySelector('li:last-child')!);
        expect(bubbles().length).toBeGreaterThanOrEqual(4);
        const liveSig = signature(live.container);

        // The read-only mount shows the comment spans inline as `span.quarto-edit-comment`;
        // the live mount strips them into bubbles. That is the one sanctioned
        // difference, so drop those spans from the read-only signature.
        const expected = readOnlySig
            .split('\n')
            .filter((line) => !line.trimStart().startsWith('span.quarto-edit-comment'))
            .join('\n');
        expect(liveSig).toBe(expected);
    });
});
