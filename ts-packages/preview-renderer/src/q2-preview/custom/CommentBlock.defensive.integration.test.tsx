/**
 * bd-ddaqjb91 — CommentBlock must be defensive about ResolvedSource.
 *
 * `PreviewContextValue.resolveSource` is a pluggable, optional context
 * member (test harnesses and hosts can supply their own). CommentBlock
 * consumes `resolved.sourceNode` on every comment-less block render to
 * decide whether to show comment chrome; a malformed entry with no
 * `sourceNode` must degrade to a plain passthrough render, not crash
 * the whole preview with
 *   TypeError: Cannot read properties of undefined (reading 't')
 * (the failure that broke the s0-list-item-surfaces suite on main).
 *
 * Plan: claude-notes/plans/2026-07-30-commentblock-defensive-resolvesource.md
 */

// @vitest-environment jsdom

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

const POOL = [{ t: 0, r: [0, 11], d: 0 }];

const BLOCKS = [
    { t: 'Para', s: 0, c: [{ t: 'Str', c: 'hello' }, { t: 'Space' }, { t: 'Str', c: 'world' }] },
];

function astJson(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: BLOCKS,
        astContext: { p: POOL },
    });
}

function mountWithResolveSource(resolveSource: PreviewContextValue['resolveSource']) {
    const ctx: PreviewContextValue = {
        currentFilePath: '/project/test.qmd',
        pool: POOL,
        content: '',
        resolveSource,
    };
    return render(
        <PreviewContext.Provider value={ctx}>
            <Ast
                astJson={astJson()}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />
        </PreviewContext.Provider>,
    );
}

describe('CommentBlock defensiveness against malformed ResolvedSource', () => {
    it('renders passthrough (no crash) when resolveSource omits sourceNode', () => {
        // The exact stale shape that broke s0: an entry with no sourceNode.
        const malformed = (node: any): ResolvedSource | null => {
            if (node?.s === undefined) return null;
            return {
                reachabilityClass: 'Descendable',
                sourceEntry: POOL[Number(node.s)],
            } as unknown as ResolvedSource;
        };
        const { container } = mountWithResolveSource(malformed);
        expect(container.textContent).toContain('hello world');
    });

    it('well-formed ResolvedSource still gets comment chrome (guard is not over-broad)', () => {
        const wellFormed = (node: any): ResolvedSource | null => {
            if (node?.s === undefined) return null;
            return {
                sourceNode: node,
                reachabilityClass: 'TopLevel',
                sourceEntry: POOL[Number(node.s)] as { t: 0; r: [number, number]; d: number },
            };
        };
        const { container } = mountWithResolveSource(wellFormed);
        expect(container.textContent).toContain('hello world');
        // Chrome marker: the CommentWrapper's relative-positioned host div
        // wraps the paragraph when the block is commentable.
        const para = container.querySelector('p');
        expect(para).not.toBeNull();
        const host = para!.parentElement as HTMLElement;
        expect(host.style.position).toBe('relative');
    });
});
