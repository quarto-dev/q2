/**
 * Figcaption synthesis from `data-qf-*` kvs in the React renderer
 * (bd-hcp8m3ve) — the preview-side twin of pampa's
 * `test_figure_figcaption_synthesis.rs`. The crossref renderer emits
 * floats as `Div > Figure(attr: quarto-float classes + data-qf-* kvs)`;
 * the Figure component must synthesize the `<figcaption>` id/classes
 * and placement from the kvs and never emit `data-qf-*` into the DOM.
 * Contract: claude-notes/designs/float-layout-class-taxonomy.md.
 */

import { describe, it, expect, afterEach, vi } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});

function floatFigureAst(kvs: [string, string][]): string {
    const figure = {
        t: 'Figure',
        c: [
            ['', ['quarto-float', 'quarto-float-fig'], kvs],
            [null, [{ t: 'Para', c: [{ t: 'Str', c: 'Figure 1: Cap' }], s: 3 }]],
            [
                {
                    t: 'Div',
                    c: [
                        ['', [], [['aria-describedby', 'fig-1-caption']]],
                        [{ t: 'Plain', c: [{ t: 'Str', c: 'body-content' }], s: 2 }],
                    ],
                    s: 1,
                },
            ],
        ],
        s: 4,
    };
    const outer = {
        t: 'Div',
        c: [
            ['fig-1', ['quarto-float', 'quarto-figure', 'quarto-figure-center'], []],
            [figure],
        ],
        s: 0,
    };
    return JSON.stringify({
        blocks: [outer],
        meta: {},
        'pandoc-api-version': [1, 23, 1],
        astContext: {
            files: [{ line_breaks: [10], name: '/t.qmd', total_length: 40 }],
            p: [
                { d: 0, r: [0, 40], t: 0 },
                { d: 0, r: [0, 40], t: 0 },
                { d: 0, r: [5, 17], t: 0 },
                { d: 0, r: [20, 33], t: 0 },
                { d: 0, r: [0, 40], t: 0 },
            ],
        },
    });
}

function mountPreviewRoot(astJson: string) {
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: 'x\n',
        currentFilePath: '/t.qmd',
        assetManifest: {},
        setAst: () => {},
        onNavigateToDocument: () => {},
    };
    return render(<PreviewRoot {...props} />);
}

const BOTTOM_KVS: [string, string][] = [
    ['data-qf-ref-type', 'fig'],
    ['data-qf-caption-location', 'bottom'],
    ['data-qf-caption-id', 'fig-1-caption'],
];

describe('float figcaption synthesis (React renderer)', () => {
    it('synthesizes figcaption id and Q1-verbatim classes from the kvs', () => {
        const { container } = mountPreviewRoot(floatFigureAst(BOTTOM_KVS));
        const figcaption = container.querySelector('figcaption');
        expect(figcaption).not.toBeNull();
        expect(figcaption!.id).toBe('fig-1-caption');
        expect(figcaption!.className).toBe(
            'quarto-float-caption-bottom quarto-float-caption quarto-float-fig',
        );
    });

    it('never emits data-qf-* attributes into the DOM', () => {
        const { container } = mountPreviewRoot(floatFigureAst(BOTTOM_KVS));
        expect(container.innerHTML).not.toContain('data-qf-');
        // The figure keeps its taxonomy classes.
        const figure = container.querySelector('figure');
        expect(figure!.className).toBe('quarto-float quarto-float-fig');
    });

    it('caption-location top places the figcaption before the body', () => {
        const { container } = mountPreviewRoot(
            floatFigureAst([
                ['data-qf-ref-type', 'fig'],
                ['data-qf-caption-location', 'top'],
                ['data-qf-caption-id', 'fig-1-caption'],
            ]),
        );
        const figure = container.querySelector('figure')!;
        const first = figure.firstElementChild!;
        expect(first.tagName.toLowerCase()).toBe('figcaption');
        expect(first.className).toContain('quarto-float-caption-top');
    });

    it('uncaptioned kv adds the quarto-uncaptioned class', () => {
        const { container } = mountPreviewRoot(
            floatFigureAst([...BOTTOM_KVS, ['data-qf-uncaptioned', '1']]),
        );
        const figcaption = container.querySelector('figcaption')!;
        expect(figcaption.className).toContain('quarto-uncaptioned');
    });
});
