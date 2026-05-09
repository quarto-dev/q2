/**
 * q2-preview surface contract for Plan 2A.
 *
 * 2A ships the iframe surface with an empty registry — every Pandoc
 * base-type hits the muted-gray "(not yet implemented)" placeholder,
 * but the dispatcher recurses into children via `renderChildren` so
 * nested nodes surface their own placeholders too. Without the
 * recursion, only top-level blocks would render and inline children
 * would be silently dropped.
 *
 * This test locks the contract:
 *   - Top-level block placeholder renders with the muted-gray aesthetic
 *     (color #888, italic).
 *   - Inline child of an unrecognized block also renders as a placeholder
 *     (proves recursion via `renderChildren`).
 *   - Registry containing only `{Ast, Block, Inline}` produces this
 *     output (no stray real-HTML leaves).
 */

import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { previewRegistry } from './registry';

function astJson(blocks: any[]): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: blocks as any,
    };
    return JSON.stringify(ast);
}

const noopNav = () => {};
const noopSet = () => {};

function mount(blocks: any[]) {
    return render(
        <Ast
            astJson={astJson(blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={noopNav}
            setAst={noopSet}
            registry={previewRegistry}
        />,
    );
}

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (text: string) => ({ t: 'Para', c: [STR(text)] });

describe('q2-preview placeholder dispatcher', () => {
    it('renders a top-level block as a muted-gray placeholder', () => {
        const { container } = mount([PARA('hello')]);
        expect(container.textContent).toContain('Para (not yet implemented)');
    });

    it('recurses into children so nested inlines also surface placeholders', () => {
        const { container } = mount([PARA('hello')]);
        // The Str inline child must surface its own placeholder via
        // renderChildren in the Block miss path.
        expect(container.textContent).toContain('Str (not yet implemented)');
    });

    it('uses the muted-gray aesthetic on the placeholder DOM', () => {
        const { container } = mount([PARA('hello')]);
        const block = container.querySelector('div');
        expect(block).not.toBeNull();
        expect(block!.style.color).toBe('rgb(136, 136, 136)'); // #888
        expect(block!.style.fontStyle).toBe('italic');
    });

    it('renders registry containing only {Ast, Block, Inline}', () => {
        // Validate the keys present in the skeleton — shaped as the
        // typed-format-registry contract requires.
        const keys = Object.keys(previewRegistry).sort();
        expect(keys).toEqual(['Ast', 'Block', 'Inline']);
    });
});
