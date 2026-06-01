/**
 * Tests for the `s:`-preservation contract in `dispatch.tsx`.
 *
 * Plan 7f Phase 2 / BP precondition: when a renderer rebuilds its
 * parent in `setLocalAst` after a child edit, the rebuilt parent
 * MUST carry the original node's `s:` field forward. Without this,
 * every child-edit corrupts the source_info reference on every
 * ancestor up the chain, defeating the producer-side contract for
 * the JSON wire format.
 *
 * Each case here constructs a node with a sentinel `s: 42`, edits
 * a child via the rebuilder the framework passes down through
 * `<Node>`'s `setLocalAst`, and asserts the rebuilt parent carries
 * `s: 42` unchanged.
 */

import React from 'react';
import { describe, it, expect } from 'vitest';
import { renderChildren, Node } from './dispatch';
import type {
    BlockNode,
    InlineNode,
    PandocAST,
    Slot,
} from './types';

// --- helpers -----------------------------------------------------------

// Recurse the React element tree returned by `renderChildren` to
// collect every `<Node>` element. Renderers wrap children at various
// depths (`<li>` for lists, plain arrays for inline wrappers), so we
// can't index by position alone.
function findNodes(tree: any): React.ReactElement[] {
    const results: React.ReactElement[] = [];
    function walk(x: any): void {
        if (x == null || typeof x === 'string' || typeof x === 'number' || typeof x === 'boolean') {
            return;
        }
        if (Array.isArray(x)) {
            x.forEach(walk);
            return;
        }
        if (typeof x === 'object' && 'type' in x) {
            if (x.type === Node) {
                results.push(x as React.ReactElement);
            }
            if (x.props && x.props.children !== undefined) {
                walk(x.props.children);
            }
        }
    }
    walk(tree);
    return results;
}

function emptyAttr(): [string, string[], [string, string][]] {
    return ['', [], []];
}

function str(c: string): InlineNode {
    return { t: 'Str', c } as InlineNode;
}

function strWithS(c: string, s: number): InlineNode {
    return { t: 'Str', c, s } as any;
}

function para(...inlines: InlineNode[]): BlockNode {
    return { t: 'Para', c: inlines } as BlockNode;
}

// Each row defines a parent node carrying `s: 42`, drills into the
// first child's setLocalAst, and asserts the rebuilt parent still
// carries `s: 42`.
type Case = {
    name: string;
    node: any;
    // newChild replaces the first child seen by `findNodes`.
    newChild: BlockNode | InlineNode;
};

const inlineChildEdit: BlockNode | InlineNode = strWithS('edited', 99);
const blockChildEdit: BlockNode | InlineNode = para(strWithS('edited', 99));

const cases: Case[] = [
    {
        name: 'Ast',
        node: {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [para(str('a'))],
            s: 42,
        },
        newChild: blockChildEdit,
    },
    {
        name: 'Emph',
        node: { t: 'Emph', c: [str('a'), str('b')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Strong',
        node: { t: 'Strong', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Underline',
        node: { t: 'Underline', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Strikeout',
        node: { t: 'Strikeout', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Superscript',
        node: { t: 'Superscript', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Subscript',
        node: { t: 'Subscript', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'SmallCaps',
        node: { t: 'SmallCaps', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Link',
        node: {
            t: 'Link',
            c: [emptyAttr(), [str('text')], ['https://example.com', '']],
            s: 42,
        },
        newChild: inlineChildEdit,
    },
    {
        name: 'Image',
        node: {
            t: 'Image',
            c: [emptyAttr(), [str('alt')], ['img.png', '']],
            s: 42,
        },
        newChild: inlineChildEdit,
    },
    {
        name: 'Span',
        node: { t: 'Span', c: [emptyAttr(), [str('a')]], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Quoted',
        node: { t: 'Quoted', c: [{ t: 'SingleQuote' }, [str('a')]], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Para',
        node: { t: 'Para', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Plain',
        node: { t: 'Plain', c: [str('a')], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'Header',
        node: { t: 'Header', c: [1, emptyAttr(), [str('Heading')]], s: 42 },
        newChild: inlineChildEdit,
    },
    {
        name: 'BlockQuote',
        node: { t: 'BlockQuote', c: [para(str('a'))], s: 42 },
        newChild: blockChildEdit,
    },
    {
        name: 'Div',
        node: { t: 'Div', c: [emptyAttr(), [para(str('a'))]], s: 42 },
        newChild: blockChildEdit,
    },
    {
        name: 'BulletList',
        node: { t: 'BulletList', c: [[para(str('a'))]], s: 42 },
        newChild: blockChildEdit,
    },
    {
        name: 'OrderedList',
        node: {
            t: 'OrderedList',
            c: [[1, { t: 'Decimal' }, { t: 'Period' }], [[para(str('a'))]]],
            s: 42,
        },
        newChild: blockChildEdit,
    },
    {
        name: 'Figure',
        node: {
            t: 'Figure',
            c: [emptyAttr(), [null, []], [para(str('body'))]],
            s: 42,
        },
        newChild: blockChildEdit,
    },
    {
        name: 'CustomBlock',
        node: {
            t: 'CustomBlock',
            type_name: 'Callout',
            slots: { content: { kind: 'blocks', value: [para(str('a'))] } as Slot },
            plain_data: { type: 'note', icon: true, appearance: 'default' },
            attr: emptyAttr(),
            s: 42,
        },
        newChild: blockChildEdit,
    },
    {
        name: 'CustomInline',
        node: {
            t: 'CustomInline',
            type_name: 'CrossrefResolvedRef',
            slots: { content: { kind: 'inlines', value: [str('a')] } as Slot },
            plain_data: {},
            attr: emptyAttr(),
            s: 42,
        },
        newChild: inlineChildEdit,
    },
];

describe('renderChildren: setLocalAst preserves parent `s:` across child edits', () => {
    for (const c of cases) {
        it(`preserves \`s:\` on ${c.name} when a child is edited`, () => {
            let captured: any = null;
            const tree = renderChildren({
                node: c.node,
                setLocalAst: (next: any) => {
                    captured = next;
                },
            } as any);

            const nodeElements = findNodes(tree);
            expect(nodeElements.length).toBeGreaterThan(0);

            // Fire the first child's rebuilder. The rebuilt parent
            // should retain `s: 42`.
            nodeElements[0].props.setLocalAst(c.newChild);

            expect(captured).not.toBeNull();
            expect(captured.s).toBe(42);
        });
    }
});
