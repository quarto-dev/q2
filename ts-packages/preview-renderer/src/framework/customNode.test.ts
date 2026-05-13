/**
 * Round-trip property tests for `unwrapCustomNodes` / `rewrapCustomNodes`.
 *
 * Anchored to the Rust wire format produced by
 * `crates/pampa/src/writers/json.rs::write_custom_block:1297` and
 * `write_custom_inline:1381`. Every JS-side change here must keep the
 * wire format compatible with `read_custom_block_from_div:2220` and
 * `read_custom_inline_from_span:2358`.
 */

import { describe, it, expect } from 'vitest';
import { unwrapCustomNodes, rewrapCustomNodes } from './customNode';
import type {
    Attr,
    CustomBlockNode,
    CustomInlineNode,
    PandocAST,
} from './types';

// --- helpers -----------------------------------------------------------

function ast(blocks: any[]): PandocAST {
    return {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
    };
}

function emptyAttr(): Attr {
    return ['', [], []];
}

function str(c: string) {
    return { t: 'Str', c };
}

function para(...inlines: any[]) {
    return { t: 'Para', c: inlines };
}

// --- round-trip fixtures (the six in-tree CustomNode type_names) -------

function calloutJsNative(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'Callout',
        slots: {
            title: { kind: 'inlines', value: [str('Note title')] },
            content: { kind: 'blocks', value: [para(str('body'))] },
        },
        plain_data: { type: 'note', icon: true, appearance: 'default' },
        attr: ['my-id', ['extra-class'], [['lang', 'en']]],
    };
}

function theoremJsNative(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'Theorem',
        slots: {
            content: { kind: 'blocks', value: [para(str('statement'))] },
        },
        plain_data: { ref_type: 'thm', identifier: 'thm-1', order: { section: [1], order: 1 } },
        attr: emptyAttr(),
    };
}

function proofJsNative(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'Proof',
        slots: {
            content: { kind: 'blocks', value: [para(str('QED'))] },
        },
        plain_data: { kind: 'proof' },
        attr: emptyAttr(),
    };
}

function floatRefTargetJsNative(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'FloatRefTarget',
        slots: {
            content: {
                kind: 'block',
                value: { t: 'Para', c: [str('figure body')] } as any,
            },
            caption_long: { kind: 'inlines', value: [str('A caption')] },
            caption_short: { kind: 'inlines', value: [] },
        },
        plain_data: {
            ref_type: 'fig',
            identifier: 'fig-1',
            order: { section: [], order: 1 },
        },
        attr: emptyAttr(),
    };
}

function equationJsNative(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'Equation',
        slots: {
            content: {
                kind: 'inline',
                value: { t: 'Math', c: [{ t: 'DisplayMath' }, 'a^2 + b^2 = c^2'] } as any,
            },
        },
        plain_data: {
            identifier: 'eq-pythag',
            order: { section: [], order: 1 },
        },
        attr: ['eq-pythag', [], []],
    };
}

function crossrefResolvedRefJsNative(): CustomInlineNode {
    return {
        t: 'CustomInline',
        type_name: 'CrossrefResolvedRef',
        slots: {
            suffix: { kind: 'inlines', value: [str(' (and onwards)')] },
        },
        plain_data: {
            identifier: 'fig-1',
            kind: 'Figure',
            ref_type: 'fig',
            resolved: true,
            kind_source: 'builtin',
            order: { section: [], order: 1 },
        },
        attr: emptyAttr(),
    };
}

function includeExpansionPlaceholder(): CustomBlockNode {
    return {
        t: 'CustomBlock',
        type_name: 'IncludeExpansion',
        slots: {
            content: { kind: 'blocks', value: [para(str('included content'))] },
        },
        plain_data: { source: 'partials/intro.qmd' },
        attr: emptyAttr(),
    };
}

// --- tests -------------------------------------------------------------

describe('CustomNode round-trip', () => {
    const fixtures: Array<[string, CustomBlockNode | CustomInlineNode]> = [
        ['Callout', calloutJsNative()],
        ['Theorem', theoremJsNative()],
        ['Proof', proofJsNative()],
        ['FloatRefTarget', floatRefTargetJsNative()],
        ['Equation', equationJsNative()],
        ['CrossrefResolvedRef', crossrefResolvedRefJsNative()],
        ['IncludeExpansion', includeExpansionPlaceholder()],
    ];

    for (const [name, jsNative] of fixtures) {
        it(`unwrap(rewrap(jsNative)) ≡ jsNative for ${name}`, () => {
            const wire = rewrapCustomNodes(ast([jsNative]));
            const round = unwrapCustomNodes(wire);
            expect(round.blocks).toEqual([jsNative]);
        });

        it(`rewrap(unwrap(wire)) ≡ wire for ${name}`, () => {
            const wireV1 = rewrapCustomNodes(ast([jsNative]));
            const wireV2 = rewrapCustomNodes(unwrapCustomNodes(wireV1));
            expect(wireV2).toEqual(wireV1);
        });
    }

    it('inline CustomNode (CrossrefResolvedRef) inside a Para has no Plain wrapper on the wire', () => {
        // Critical asymmetry: block CustomNodes wrap Inline/Inlines slot
        // contents in Plain blocks; inline CustomNodes do NOT.
        const xref = crossrefResolvedRefJsNative();
        const wire = rewrapCustomNodes(ast([para(str('see '), xref)]));
        // Drill into the wire-format Para's c[1] (the rewrapped inline) →
        // wrapper Span → c[1][0] (slot wrapper Span) → c[1] (slot content).
        const paraInlines = (wire.blocks[0] as any).c;
        const wrapperSpan = paraInlines[1];
        expect(wrapperSpan.t).toBe('Span');
        const slotWrapper = wrapperSpan.c[1][0];
        expect(slotWrapper.t).toBe('Span');
        const slotContent = slotWrapper.c[1];
        // Slot content must be a flat array of inlines, not a Plain wrapper.
        expect(Array.isArray(slotContent)).toBe(true);
        expect(slotContent[0]).toEqual(str(' (and onwards)'));
        expect(slotContent[0].t).not.toBe('Plain');
    });

    it('block CustomNode wraps Inlines slot in a Plain block on the wire', () => {
        // Mirror invariant: Callout's title slot (kind: inlines) emits
        // `[{t: 'Plain', c: [...inlines]}]` on the wire.
        const callout = calloutJsNative();
        const wire = rewrapCustomNodes(ast([callout]));
        const wrapperDiv = wire.blocks[0] as any;
        expect(wrapperDiv.t).toBe('Div');
        // Find the title slot.
        const slotChildren = wrapperDiv.c[1];
        const titleWrapper = slotChildren.find(
            (s: any) => s.c[0][2].some(([k]: [string, string]) => k === 'data-slot-name')
                && s.c[0][2].find(([k]: [string, string]) => k === 'data-slot-name')[1] === 'title',
        );
        expect(titleWrapper).toBeDefined();
        expect(titleWrapper.c[1][0].t).toBe('Plain');
    });

    it('preserves user attr (id, classes, kvs) — not just the custom-node bookkeeping', () => {
        const callout = calloutJsNative();
        const wire = rewrapCustomNodes(ast([callout])) as PandocAST;
        const div = wire.blocks[0] as any;
        const [id, classes, kvs] = div.c[0];
        expect(id).toBe('my-id');
        // __quarto_custom_node first, user 'extra-class' after.
        expect(classes[0]).toBe('__quarto_custom_node');
        expect(classes).toContain('extra-class');
        // User kvs are preserved alongside the data-custom-* triple.
        expect(kvs.find(([k]: [string, string]) => k === 'lang')).toEqual(['lang', 'en']);
        expect(kvs.find(([k]: [string, string]) => k === 'data-custom-type')).toEqual([
            'data-custom-type',
            'Callout',
        ]);
    });

    it('omits data-custom-data when plain_data is null', () => {
        const node: CustomBlockNode = {
            t: 'CustomBlock',
            type_name: 'Empty',
            slots: {},
            plain_data: null,
            attr: emptyAttr(),
        };
        const wire = rewrapCustomNodes(ast([node])) as any;
        const kvs = wire.blocks[0].c[0][2] as [string, string][];
        const keys = kvs.map(([k]) => k);
        expect(keys).not.toContain('data-custom-data');
    });

    it('preserves source-info index s through the round-trip', () => {
        const node: CustomBlockNode = {
            t: 'CustomBlock',
            type_name: 'Theorem',
            slots: {},
            plain_data: null,
            attr: emptyAttr(),
            s: 42,
        };
        const wire = rewrapCustomNodes(ast([node])) as any;
        expect(wire.blocks[0].s).toBe(42);
        const round = unwrapCustomNodes(wire);
        expect((round.blocks[0] as CustomBlockNode).s).toBe(42);
    });

    it('handles nested CustomNode inside a slot (Plan 8 shape)', () => {
        const inner = theoremJsNative();
        const outer: CustomBlockNode = {
            t: 'CustomBlock',
            type_name: 'Callout',
            slots: {
                content: { kind: 'blocks', value: [inner] },
            },
            plain_data: null,
            attr: emptyAttr(),
        };
        const wire = rewrapCustomNodes(ast([outer]));
        const round = unwrapCustomNodes(wire);
        expect(round.blocks).toEqual([outer]);
    });
});

describe('Walker purity (structural sharing)', () => {
    it('subtrees with no wrappers are returned by reference', () => {
        // Capture references to leaves in unrelated branches; make sure
        // they survive unwrap by identity. Load-bearing for the Note
        // WeakMap lookup in PreviewRoot.
        const leafA = str('untouched A');
        const leafB = str('untouched B');
        const xref = rewrapCustomNodes(
            ast([para(crossrefResolvedRefJsNative())]),
        ).blocks[0]; // wire-format Para containing the wrapper Span
        const tree = ast([
            para(leafA),
            xref as any,
            para(leafB),
        ]);
        const out = unwrapCustomNodes(tree);
        // Unrelated branches: same Para (and same Str) by reference.
        expect(out.blocks[0]).toBe(tree.blocks[0]);
        expect((out.blocks[0] as any).c[0]).toBe(leafA);
        expect(out.blocks[2]).toBe(tree.blocks[2]);
        expect((out.blocks[2] as any).c[0]).toBe(leafB);
        // The branch that contains the wrapper IS rebuilt at the path,
        // but the para reference should differ.
        expect(out.blocks[1]).not.toBe(tree.blocks[1]);
    });

    it('returns the input AST by reference when no wrappers exist anywhere', () => {
        const tree = ast([para(str('a')), para(str('b'))]);
        const out = unwrapCustomNodes(tree);
        expect(out).toBe(tree);
        expect(out.blocks).toBe(tree.blocks);
    });

    it('rewrap returns the input AST by reference when no JS-native CustomNodes exist', () => {
        const tree = ast([para(str('a'))]);
        const out = rewrapCustomNodes(tree);
        expect(out).toBe(tree);
    });
});

describe('Defensive decoding', () => {
    it('decodes a Div with __quarto_custom_node missing data-custom-type to "Unknown"', () => {
        const wire: any = ast([
            {
                t: 'Div',
                c: [
                    ['', ['__quarto_custom_node'], []],
                    [],
                ],
            },
        ]);
        const out = unwrapCustomNodes(wire) as any;
        expect(out.blocks[0].t).toBe('CustomBlock');
        expect(out.blocks[0].type_name).toBe('Unknown');
    });

    it('treats a non-wrapper Div with the same structural shape as a regular Div', () => {
        const wire: any = ast([
            {
                t: 'Div',
                c: [
                    ['my-id', ['some-class'], [['k', 'v']]],
                    [para(str('hello'))],
                ],
            },
        ]);
        const out = unwrapCustomNodes(wire);
        // Non-wrapper Div is preserved by reference (no transform path).
        expect(out).toBe(wire);
        expect(out.blocks[0]).toBe(wire.blocks[0]);
    });
});
