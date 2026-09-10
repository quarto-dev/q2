/**
 * Bubble geometry under the overlay layer (bd-q2wqj24c).
 *
 * Without a per-block wrapper there is nothing for the chrome to be
 * `position: absolute` against, so the relayout pass measures each
 * block's rect and writes the bubble's `top`/`left` in the layer's own
 * coordinates (anchor rect − layer rect): the natural spot is 11px above
 * the block's top edge, right-aligned 10px past its right edge
 * (`transform: translate(-100%, nudge)`). Measuring the layer rather than
 * assuming document coordinates keeps the math right whatever the body's
 * margin or position, scrolled or not.
 *
 * jsdom has no layout, so the rects are stubbed and the rAF-batched pass
 * is flushed explicitly.
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

const POOL = [
    { t: 0, r: [0, 10], d: 0 },
    { t: 0, r: [10, 20], d: 0 },
];
const comment = (text: string) => ({
    t: 'Span',
    c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: text }]],
});

function astJson(paraCount: number): string {
    const blocks = Array.from({ length: paraCount }, (_, i) => ({
        t: 'Para',
        s: i,
        c: [{ t: 'Str', c: `Para ${i}` }, comment(`note ${i}`)],
    }));
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
        sourceEntry: POOL[Number(node.s)] as { t: 0; r: [number, number]; d: number },
    };
};

function mount(paraCount: number) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        commentsMode: 'show',
        resolveSource,
        commitSubtreeEdit: () => {},
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson(paraCount)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
}

type Box = { left: number; top: number; right: number; bottom: number };
function stubRect(el: Element, r: Box) {
    (el as HTMLElement).getBoundingClientRect = () =>
        ({ ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON() {} }) as DOMRect;
}

const chromes = () => [...document.body.querySelectorAll<HTMLElement>('[data-q2-comment-layer] [data-q2-owns-focus]')];
const layer = () => document.body.querySelector<HTMLElement>('[data-q2-comment-layer]')!;
const flushRelayout = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));

describe('bubble geometry from the block rect', () => {
    it('positions the bubble 11px above and right-aligned 10px past the block, in layer coordinates', async () => {
        const { container } = mount(1);
        const p = container.querySelector('p')!;
        stubRect(p, { left: 100, top: 200, right: 500, bottom: 240 });
        // The layer sits 300px above the viewport top: the page is scrolled.
        stubRect(layer(), { left: 0, top: -300, right: 0, bottom: -300 });
        stubRect(chromes()[0], { left: 400, top: 189, right: 600, bottom: 209 });
        // Something has to trigger a pass after the stubs are in place — a
        // hover start does, as it would in the browser.
        p.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 450, clientY: 220 }));
        await flushRelayout();

        const c = chromes()[0];
        expect(c.style.position).toBe('absolute');
        expect(c.style.top).toBe('489px'); // 200 − (−300) − 11
        expect(c.style.left).toBe('510px'); // 500 − 0 + 10
        expect(c.style.transform).toContain('translate(-100%, 0px)');
    });

    it('nudges two bubbles with the same anchor apart, earlier up and later down', async () => {
        const { container } = mount(2);
        const [p0, p1] = [...container.querySelectorAll('p')];
        // Both paragraphs report the same box (the layout pathology the
        // force layout exists for), and both bubbles are 20px tall.
        for (const p of [p0, p1]) stubRect(p, { left: 100, top: 200, right: 500, bottom: 240 });
        stubRect(layer(), { left: 0, top: 0, right: 0, bottom: 0 });
        for (const c of chromes()) stubRect(c, { left: 400, top: 189, right: 600, bottom: 209 });
        p0.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 450, clientY: 220 }));
        await flushRelayout();
        p1.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 450, clientY: 220 }));
        await flushRelayout();

        const [c0, c1] = chromes();
        // Both share the natural top (189px); they separate purely through
        // the nudge translation. The hovered (later) bubble is pinned at its
        // natural spot, so the earlier one carries the whole overlap
        // (20px bubble + 4px gap = 24px), pushed up.
        expect(c0.style.top).toBe('189px');
        expect(c1.style.top).toBe('189px');
        const nudge = (c: HTMLElement) => Number(/translate\(-100%, (-?\d+)px\)/.exec(c.style.transform)?.[1]);
        expect(nudge(c1)).toBe(0);
        expect(nudge(c0)).toBe(-24);
    });
});
