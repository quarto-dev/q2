// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import {
    MermaidCodeBlock,
    MERMAID_VERSION,
    setMermaidLoaderForTests,
} from './MermaidCodeBlock';
import { previewRegistry } from '../registry';
import type { NodeArgs, CodeBlock as CodeBlockType } from '../../framework';

/**
 * bd-5m4ga0s1: built-in mermaid rendering for q2-preview / q2-slides.
 * A ```mermaid fenced block reaches the React layer as a raw
 * CodeBlock (the Rust `mermaid-render` transform is excluded from the
 * preview pipeline); the registry's `CodeBlock` entry is the
 * mermaid-aware wrapper, which renders diagrams via mermaid.js and
 * delegates everything else to the plain built-in.
 *
 * The mermaid loader is injected in tests (`setMermaidLoaderForTests`)
 * so no network / CDN access happens here; the default loader's
 * dynamic-import path is exercised in browser e2e (Phase 4).
 */

const codeBlock = (classes: string[], code: string): CodeBlockType =>
    ({ t: 'CodeBlock', c: [['', classes, []], code] }) as CodeBlockType;

function renderNode(node: CodeBlockType, Component = MermaidCodeBlock) {
    const args = { node, setLocalAst: () => {} } as NodeArgs<CodeBlockType>;
    return render(<Component {...args} />);
}

describe('MermaidCodeBlock', () => {
    let renderCalls: string[];

    beforeEach(() => {
        renderCalls = [];
    });

    afterEach(() => {
        setMermaidLoaderForTests(null);
    });

    /** Install a fake mermaid API; returns the loader spy. */
    function installFakeMermaid() {
        const loader = vi.fn(async () => ({
            render: async (_id: string, code: string) => {
                renderCalls.push(code);
                return { svg: '<svg data-fake-mermaid="1"></svg>' };
            },
        }));
        setMermaidLoaderForTests(loader);
        return loader;
    }

    it('renders a diagram container and injects the rendered SVG', async () => {
        installFakeMermaid();
        const { container } = renderNode(codeBlock(['mermaid'], 'flowchart LR\n  a --> b'));

        const wrapper = container.querySelector('[data-mermaid-diagram]');
        expect(wrapper).not.toBeNull();
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-mermaid]')).not.toBeNull();
        });
        expect(renderCalls).toEqual(['flowchart LR\n  a --> b']);
        // The raw source must not be shown as code once rendered.
        expect(container.querySelector('pre')).toBeNull();
    });

    it('delegates non-mermaid code blocks to the plain CodeBlock', () => {
        const loader = installFakeMermaid();
        const { container } = renderNode(codeBlock(['python'], "print('hi')"));

        const pre = container.querySelector('pre');
        expect(pre).not.toBeNull();
        expect(pre!.className).toBe('python');
        expect(pre!.textContent).toBe("print('hi')");
        expect(container.querySelector('[data-mermaid-diagram]')).toBeNull();
        expect(loader).not.toHaveBeenCalled();
    });

    it('does not treat the brace form {mermaid} as a diagram', () => {
        const loader = installFakeMermaid();
        const { container } = renderNode(codeBlock(['{mermaid}'], 'flowchart LR\n  a --> b'));
        expect(container.querySelector('[data-mermaid-diagram]')).toBeNull();
        expect(loader).not.toHaveBeenCalled();
    });

    it('loads mermaid once across multiple diagrams', async () => {
        const loader = installFakeMermaid();
        const a = renderNode(codeBlock(['mermaid'], 'flowchart LR\n  a --> b'));
        const b = renderNode(codeBlock(['mermaid'], 'flowchart TD\n  x --> y'));
        await waitFor(() => {
            expect(a.container.querySelector('svg[data-fake-mermaid]')).not.toBeNull();
            expect(b.container.querySelector('svg[data-fake-mermaid]')).not.toBeNull();
        });
        expect(loader).toHaveBeenCalledTimes(1);
    });

    it('renders an error box (message + source) when mermaid rejects', async () => {
        setMermaidLoaderForTests(async () => ({
            render: async () => {
                throw new Error('Parse error on line 1');
            },
        }));
        const { container } = renderNode(codeBlock(['mermaid'], 'not a diagram'));
        await waitFor(() => {
            expect(container.querySelector('[data-mermaid-error]')).not.toBeNull();
        });
        const box = container.querySelector('[data-mermaid-error]')!;
        expect(box.textContent).toContain('Parse error on line 1');
        expect(box.textContent).toContain('not a diagram');
    });

    it('is registered as the previewRegistry CodeBlock entry', () => {
        expect(previewRegistry.CodeBlock).toBe(MermaidCodeBlock);
    });

    it('pins the same mermaid version as the Rust transform', () => {
        // Keep in sync with MERMAID_VERSION in
        // crates/quarto-core/src/transforms/mermaid.rs.
        expect(MERMAID_VERSION).toBe('11.12.0');
    });

    it('is shadowed by a user render-components CodeBlock override', () => {
        // Prototype-compat lock (bd-5m4ga0s1): the daily-log mermaid
        // prototype ships its own CodeBlock via render-components.
        // PreviewRoot merges `{ ...previewRegistry, ...customRegistry }`,
        // so a user CodeBlock must win over the mermaid-aware built-in.
        const UserCodeBlock = () => null;
        const merged = { ...previewRegistry, ...{ CodeBlock: UserCodeBlock } };
        expect(merged.CodeBlock).toBe(UserCodeBlock);
        expect(previewRegistry.CodeBlock).toBe(MermaidCodeBlock);
    });
});
