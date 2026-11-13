// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import {
    GraphvizCodeBlock,
    VIZ_VERSION,
    sanitizeGraphvizSvg,
    setGraphvizLoaderForTests,
} from './GraphvizCodeBlock';
import { DiagramCodeBlock } from './DiagramCodeBlock';
import { previewRegistry } from '../registry';
import type { NodeArgs, CodeBlock as CodeBlockType } from '../../framework';

/**
 * bd-ijlapt70 (prototype): built-in Graphviz rendering for q2-preview.
 * A ```dot fenced block reaches the React layer as a raw CodeBlock;
 * the registry's `CodeBlock` entry is the diagram dispatcher, which
 * sends `dot` blocks here and everything else to the mermaid-aware
 * wrapper.
 *
 * The Graphviz loader is injected in tests
 * (`setGraphvizLoaderForTests`), so the wasm module is never loaded
 * here.
 */

const codeBlock = (classes: string[], code: string, attrs: [string, string][] = []): CodeBlockType =>
    ({ t: 'CodeBlock', c: [['', classes, attrs], code] }) as CodeBlockType;

function renderNode(node: CodeBlockType, Component = GraphvizCodeBlock) {
    const args = { node, setLocalAst: () => {} } as NodeArgs<CodeBlockType>;
    return render(<Component {...args} />);
}

const FAKE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" data-fake-graphviz="1"></svg>';

describe('GraphvizCodeBlock', () => {
    let renderCalls: { code: string; engine: string }[];

    beforeEach(() => {
        renderCalls = [];
    });

    afterEach(() => {
        setGraphvizLoaderForTests(null);
    });

    /** Install a fake Viz instance; returns the loader spy. */
    function installFakeGraphviz() {
        const loader = vi.fn(async () => ({
            render: (code: string, options: { format: string; engine: string }) => {
                renderCalls.push({ code, engine: options.engine });
                return { status: 'success' as const, output: FAKE_SVG, errors: [] };
            },
        }));
        setGraphvizLoaderForTests(loader);
        return loader;
    }

    it('renders a diagram container and injects the rendered SVG', async () => {
        installFakeGraphviz();
        const { container } = renderNode(codeBlock(['dot'], 'digraph G { a -> b }'));

        expect(container.querySelector('[data-graphviz-diagram]')).not.toBeNull();
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-graphviz]')).not.toBeNull();
        });
        expect(renderCalls).toEqual([{ code: 'digraph G { a -> b }', engine: 'dot' }]);
        // The raw source must not be shown as code once rendered.
        expect(container.querySelector('pre')).toBeNull();
    });

    it('passes a graph-layout attribute to the layout engine', async () => {
        installFakeGraphviz();
        renderNode(codeBlock(['dot'], 'graph G { a -- b }', [['graph-layout', 'neato']]));
        await waitFor(() => {
            expect(renderCalls).toEqual([{ code: 'graph G { a -- b }', engine: 'neato' }]);
        });
    });

    it('falls back to dot for a graph-layout value that is not an engine', async () => {
        installFakeGraphviz();
        renderNode(codeBlock(['dot'], 'graph G { a -- b }', [['graph-layout', 'bogus']]));
        await waitFor(() => {
            expect(renderCalls).toEqual([{ code: 'graph G { a -- b }', engine: 'dot' }]);
        });
    });

    it('aligns the diagram from its fig-align attribute', () => {
        installFakeGraphviz();
        const { container } = renderNode(codeBlock(['dot'], 'digraph G {}', [['fig-align', 'right']]));
        const wrapper = container.querySelector<HTMLElement>('[data-graphviz-diagram]');
        expect(wrapper?.style.textAlign).toBe('right');
    });

    it('delegates non-dot code blocks to the plain CodeBlock', () => {
        const loader = installFakeGraphviz();
        const { container } = renderNode(codeBlock(['python'], "print('hi')"));

        const pre = container.querySelector('pre');
        expect(pre).not.toBeNull();
        expect(pre!.textContent).toBe("print('hi')");
        expect(container.querySelector('[data-graphviz-diagram]')).toBeNull();
        expect(loader).not.toHaveBeenCalled();
    });

    it('does not treat the brace form {dot} as a diagram', () => {
        const loader = installFakeGraphviz();
        const { container } = renderNode(codeBlock(['{dot}'], 'digraph G { a -> b }'));
        expect(container.querySelector('[data-graphviz-diagram]')).toBeNull();
        expect(loader).not.toHaveBeenCalled();
    });

    it('loads Graphviz once across multiple diagrams', async () => {
        const loader = installFakeGraphviz();
        const a = renderNode(codeBlock(['dot'], 'digraph A { a -> b }'));
        const b = renderNode(codeBlock(['dot'], 'digraph B { x -> y }'));
        await waitFor(() => {
            expect(a.container.querySelector('svg[data-fake-graphviz]')).not.toBeNull();
            expect(b.container.querySelector('svg[data-fake-graphviz]')).not.toBeNull();
        });
        expect(loader).toHaveBeenCalledTimes(1);
    });

    it('renders an error box (message + source) for a Graphviz syntax error', async () => {
        setGraphvizLoaderForTests(async () => ({
            render: () => ({
                status: 'failure' as const,
                output: undefined,
                errors: [{ level: 'error' as const, message: "syntax error in line 1 near '}'" }],
            }),
        }));
        const { container } = renderNode(codeBlock(['dot'], 'digraph G { a -> }'));
        await waitFor(() => {
            expect(container.querySelector('[data-graphviz-error]')).not.toBeNull();
        });
        const box = container.querySelector('[data-graphviz-error]')!;
        expect(box.textContent).toContain("syntax error in line 1 near '}'");
        expect(box.textContent).toContain('digraph G { a -> }');
    });

    it('renders an error box when the runtime fails to load', async () => {
        setGraphvizLoaderForTests(async () => {
            throw new Error('wasm blocked');
        });
        const { container } = renderNode(codeBlock(['dot'], 'digraph G {}'));
        await waitFor(() => {
            expect(container.querySelector('[data-graphviz-error]')).not.toBeNull();
        });
        expect(container.querySelector('[data-graphviz-error]')!.textContent).toContain('wasm blocked');
    });

    it('pins the Viz.js version recorded in the plan', () => {
        // Keep in sync with the `@viz-js/viz` version in package.json
        // and, once it exists, the Rust-side constant.
        expect(VIZ_VERSION).toBe('3.31.0');
    });
});

describe('sanitizeGraphvizSvg', () => {
    const wrap = (inner: string) =>
        `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">${inner}</svg>`;

    it('keeps ordinary shapes and text', () => {
        const svg = sanitizeGraphvizSvg(wrap('<g class="node"><ellipse cx="1" cy="1" rx="2" ry="2"/><text>a</text></g>'));
        expect(svg).not.toBeNull();
        expect(svg!.querySelector('ellipse')).not.toBeNull();
        expect(svg!.querySelector('text')!.textContent).toBe('a');
    });

    it('removes script elements and event-handler attributes', () => {
        const svg = sanitizeGraphvizSvg(wrap('<script>alert(1)</script><g onclick="alert(2)"><text>a</text></g>'));
        expect(svg!.querySelector('script')).toBeNull();
        expect(svg!.querySelector('g')!.hasAttribute('onclick')).toBe(false);
    });

    it('removes javascript: links but keeps http links', () => {
        const svg = sanitizeGraphvizSvg(
            wrap(
                '<a xlink:href="javascript:alert(1)"><text>bad</text></a>' +
                    '<a xlink:href="https://example.com/"><text>ok</text></a>',
            ),
        );
        const links = [...svg!.querySelectorAll('a')];
        expect(links[0].getAttribute('xlink:href')).toBeNull();
        expect(links[1].getAttribute('xlink:href')).toBe('https://example.com/');
    });

    it('returns null for text that is not SVG', () => {
        expect(sanitizeGraphvizSvg('<div>not svg</div>')).toBeNull();
    });
});

describe('DiagramCodeBlock', () => {
    afterEach(() => {
        setGraphvizLoaderForTests(null);
    });

    it('is registered as the previewRegistry CodeBlock entry', () => {
        expect(previewRegistry.CodeBlock).toBe(DiagramCodeBlock);
    });

    it('sends dot blocks to the Graphviz component', () => {
        setGraphvizLoaderForTests(async () => ({
            render: () => ({ status: 'success' as const, output: FAKE_SVG, errors: [] }),
        }));
        const { container } = renderNode(codeBlock(['dot'], 'digraph G {}'), DiagramCodeBlock);
        expect(container.querySelector('[data-graphviz-diagram]')).not.toBeNull();
    });

    it('sends other blocks down the mermaid-aware path', () => {
        const { container } = renderNode(codeBlock(['python'], "print('hi')"), DiagramCodeBlock);
        expect(container.querySelector('[data-graphviz-diagram]')).toBeNull();
        expect(container.querySelector('pre')!.textContent).toBe("print('hi')");
    });
});
