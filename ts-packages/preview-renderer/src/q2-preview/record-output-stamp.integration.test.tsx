/**
 * RIU-3b: spliced record output is stamped `Generated { by: jupyter-output }`, and the viewer
 * must keep behaving. Ported from RIU-0c spike 1 (`spike/riu-s10-viewer-edit`).
 *
 * The edit affordance on a block is the attribute `data-block-pool-id`, present only when
 * `resolveSource(node)` finds the node's SourceInfo entry in the index of the UNTRANSFORMED AST
 * (entries with t=0 and d=0).
 */
import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';

afterEach(() => cleanup());

const CODE = 'x = 1';
const codeBlock = (s: number) => ({
    t: 'CodeBlock', c: [['', ['python'], []], CODE], s,
});

function untransformed(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        // The author's source: a Div (range 0..40) around the code (range 0..20).
        blocks: [{ t: 'Div', c: [['', ['note'], []], [codeBlock(0)]], s: 1 }],
        astContext: { p: [{ t: 0, r: [0, 20], d: 0 }, { t: 0, r: [0, 40], d: 0 }] },
    });
}

/**
 * Pool layout: 0 = author code range, 1 = reparse-slot code range, 2 = Div as an author
 * Original, 3 = Generated jupyter-output.
 */
function transformed(opts: { divPool: number; codePool: number; divClasses?: string[] }): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'Div',
                c: [['', opts.divClasses ?? ['cell'], []], [codeBlock(opts.codePool)]],
                s: opts.divPool,
            },
        ],
        astContext: {
            p: [
                { t: 0, r: [0, 20], d: 0 },
                { t: 0, r: [5, 25], d: 1 },
                { t: 0, r: [0, 40], d: 0 },
                { t: 4, r: [0, 0], d: { by: { kind: 'jupyter-output', data: { source: 'engine' } }, from: [] } },
            ],
        },
    });
}

function mount(t: string) {
    const props: PreviewRootProps = {
        astJson: t,
        untransformedAstJson: untransformed(),
        renderedContent: 'x = 1\n',
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst: () => {},
        onNavigateToDocument: () => {},
    };
    return render(<PreviewRoot {...props} />);
}

function codeAffordance(container: HTMLElement): boolean {
    const pre = container.querySelector('pre');
    if (!pre) throw new Error('no <pre> rendered');
    return pre.hasAttribute('data-block-pool-id');
}

function divAffordance(container: HTMLElement): boolean {
    const div = container.querySelector('div[data-loc], div.cell, div.note');
    if (!div) throw new Error('no <div> rendered');
    return div.hasAttribute('data-block-pool-id');
}

describe('riu-3b: editing around a stamped cell', () => {
    it('baseline: an author-owned code block (Original, file 0) inside an unstamped Div is editable', () => {
        const { container } = mount(transformed({ divPool: 2, codePool: 0, divClasses: ['note'] }));
        expect(codeAffordance(container)).toBe(true);
    });

    it('echoed code resolving into the reparse slot (file 1) is not editable', () => {
        const { container } = mount(transformed({ divPool: 2, codePool: 1 }));
        expect(codeAffordance(container)).toBe(false);
    });

    it('an atomic cell Div (jupyter-output) leaves an author-owned code block affordance unchanged', () => {
        const { container } = mount(transformed({ divPool: 3, codePool: 0 }));
        expect(codeAffordance(container)).toBe(true);
    });

    it('an atomic cell Div with slot-owned code stays not editable', () => {
        const { container } = mount(transformed({ divPool: 3, codePool: 1 }));
        expect(codeAffordance(container)).toBe(false);
    });
});

// interim_: the splice cannot name its own nodes, so reconcile can pair the author's next Div
// with a spliced `.cell` Div, which then keeps the author's source info and is not stamped. The
// viewer refuses to edit any Div whose class includes `cell`. When engines return provenance for
// the nodes they produce, replace these tests (and the rule in blocks/Div.tsx).
describe('interim_: a `.cell` Div is never editable', () => {
    it('interim_ a .cell Div that resolves to the author\'s bytes gets no edit affordance', () => {
        const { container } = mount(transformed({ divPool: 2, codePool: 0, divClasses: ['cell'] }));
        expect(divAffordance(container)).toBe(false);
    });

    it('interim_ an author-owned Div that is not a .cell keeps its edit affordance', () => {
        const { container } = mount(transformed({ divPool: 2, codePool: 0, divClasses: ['note'] }));
        expect(divAffordance(container)).toBe(true);
    });
});
