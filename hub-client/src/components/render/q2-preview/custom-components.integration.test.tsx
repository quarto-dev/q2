/**
 * Per-component integration tests for the q2-preview CustomNode
 * components shipped in Plan 2C. Mounts the full `<Ast>` with the
 * post-2C `previewRegistry` and asserts the rendered DOM matches
 * the spec at §"q2-preview/custom/" in the plan.
 *
 * Coverage matrix per plan §Test plan:
 *  - Per-component snapshot/structure (Callout 3-deep nesting,
 *    Theorem env class, Proof title-via-em, FloatRefTarget figure-vs-
 *    div, Equation \tag{N}, CrossrefResolvedRef text format).
 *  - Generic Fallback test (unknown type_name).
 *  - Class-compatibility (built-in classes match `quartoClasses.ts`).
 *  - Atomic CustomNode read-only test (registry-routed) — captures
 *    setLocalAst inside CrossrefResolvedRef and asserts it's the
 *    framework's NOOP.
 *  - CustomNode override integration (Pandoc tag override + CustomNode
 *    override layered by the same merge).
 */

import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import type { FormatRegistry, NodeArgs, PandocAST } from '@quarto/preview-renderer/framework';
import { previewRegistry } from './registry';
import {
    CALLOUT,
    CALLOUT_BODY,
    CALLOUT_BODY_CONTAINER,
    CALLOUT_FLEX_FILL,
    CALLOUT_HEADER,
    CALLOUT_ICON,
    CALLOUT_ICON_CONTAINER,
    CALLOUT_TITLE_CONTAINER,
    PROOF,
    QUARTO_XREF,
    THEOREM,
    THEOREM_TITLE,
} from './quartoClasses';

const noopNav = () => {};
const noopSet = () => {};

function astJson(blocks: any[]): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: blocks as any,
    };
    return JSON.stringify(ast);
}

function mount(blocks: any[], registryOverride?: FormatRegistry) {
    return render(
        <Ast
            astJson={astJson(blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={noopNav}
            setAst={noopSet}
            registry={registryOverride ?? previewRegistry}
        />,
    );
}

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (...inlines: any[]) => ({ t: 'Para', c: inlines });

// Test fixtures —————————————————————————————————————————————————————————

function calloutAst(opts: {
    type?: string;
    appearance?: string;
    collapse?: boolean;
    icon?: boolean;
    title?: any[] | undefined; // undefined = no title slot; [] = empty Inlines
    content?: any[];
    id?: string;
}) {
    const slots: Record<string, any> = {};
    if (opts.title !== undefined) {
        slots.title = { kind: 'inlines', value: opts.title };
    }
    slots.content = { kind: 'blocks', value: opts.content ?? [PARA(STR('body'))] };
    return {
        t: 'CustomBlock',
        type_name: 'Callout',
        slots,
        plain_data: {
            type: opts.type ?? 'note',
            appearance: opts.appearance ?? 'default',
            collapse: opts.collapse ?? false,
            icon: opts.icon ?? true,
        },
        attr: [opts.id ?? '', [], []],
    };
}

function theoremAst(opts: {
    refType: string;
    kind?: string;
    identifier?: string;
    order?: number;
    title?: any[];
    content?: any[];
}) {
    const slots: Record<string, any> = {
        content: { kind: 'blocks', value: opts.content ?? [PARA(STR('body'))] },
    };
    if (opts.title !== undefined) slots.title = { kind: 'inlines', value: opts.title };
    return {
        t: 'CustomBlock',
        type_name: 'Theorem',
        slots,
        plain_data: {
            ref_type: opts.refType,
            kind: opts.kind ?? 'Theorem',
            identifier: opts.identifier ?? '',
            ...(opts.order !== undefined ? { order: { section: [], order: opts.order } } : {}),
        },
        attr: [opts.identifier ?? '', [], []],
    };
}

function proofAst(opts: { title?: any[]; content?: any[]; identifier?: string }) {
    const slots: Record<string, any> = {
        content: { kind: 'blocks', value: opts.content ?? [PARA(STR('body'))] },
    };
    if (opts.title !== undefined) slots.title = { kind: 'inlines', value: opts.title };
    return {
        t: 'CustomBlock',
        type_name: 'Proof',
        slots,
        plain_data: { kind: 'Proof' },
        attr: [opts.identifier ?? '', [], []],
    };
}

function floatRefAst(opts: {
    refType: string;
    kind?: string;
    identifier?: string;
    order?: number;
    content?: any[];
    captionLong?: any[];
}) {
    const slots: Record<string, any> = {
        content: { kind: 'blocks', value: opts.content ?? [PARA(STR('body'))] },
    };
    if (opts.captionLong !== undefined) {
        slots.caption_long = { kind: 'blocks', value: opts.captionLong };
    }
    return {
        t: 'CustomBlock',
        type_name: 'FloatRefTarget',
        slots,
        plain_data: {
            ref_type: opts.refType,
            kind: opts.kind ?? 'Figure',
            identifier: opts.identifier ?? '',
            ...(opts.order !== undefined ? { order: { section: [], order: opts.order } } : {}),
        },
        attr: [opts.identifier ?? '', [], []],
    };
}

function equationAst(opts: { latex: string; order?: number; identifier?: string; type?: 'DisplayMath' | 'InlineMath' }) {
    return {
        t: 'CustomInline',
        type_name: 'Equation',
        slots: {
            content: {
                kind: 'inlines',
                value: [
                    {
                        t: 'Math',
                        c: [{ t: opts.type ?? 'DisplayMath' }, opts.latex],
                    },
                ],
            },
        },
        plain_data: {
            ref_type: 'eq',
            kind: 'Equation',
            identifier: opts.identifier ?? '',
            ...(opts.order !== undefined ? { order: { section: [], order: opts.order } } : {}),
        },
        attr: [opts.identifier ?? '', [], []],
    };
}

function crossrefRefAst(opts: {
    identifier: string;
    refType: string;
    kind: string;
    resolved: boolean;
    order?: number;
    suffix?: any[];
}) {
    const slots: Record<string, any> = {};
    if (opts.suffix !== undefined) slots.suffix = { kind: 'inlines', value: opts.suffix };
    return {
        t: 'CustomInline',
        type_name: 'CrossrefResolvedRef',
        slots,
        plain_data: {
            identifier: opts.identifier,
            ref_type: opts.refType,
            kind: opts.kind,
            resolved: opts.resolved,
            kind_source: 'builtin',
            ...(opts.order !== undefined ? { order: { section: [], order: opts.order } } : {}),
        },
        attr: ['', [], []],
    };
}

// —————————————————————————————————————————————————————————————————————————

describe('Callout', () => {
    it('renders the three-deep Bootstrap-flavored nesting', () => {
        const { container } = mount([
            calloutAst({ type: 'note', title: [STR('Heads up')], content: [PARA(STR('body text'))] }),
        ]);
        // .callout > .callout-header > .callout-title-container.flex-fill
        const titleContainer = container.querySelector(
            `div.${CALLOUT}.callout-note > div.${CALLOUT_HEADER} > div.${CALLOUT_TITLE_CONTAINER}.${CALLOUT_FLEX_FILL}`,
        );
        expect(titleContainer).not.toBeNull();
        expect(titleContainer!.textContent).toContain('Heads up');
        // .callout > .callout-body-container.callout-body
        const body = container.querySelector(
            `div.${CALLOUT} > div.${CALLOUT_BODY_CONTAINER}.${CALLOUT_BODY}`,
        );
        expect(body).not.toBeNull();
        expect(body!.textContent).toContain('body text');
    });

    it('emits the icon container <i class="callout-icon"> when icon=true', () => {
        const { container } = mount([calloutAst({ icon: true })]);
        const iconContainer = container.querySelector(`.${CALLOUT_ICON_CONTAINER}`);
        expect(iconContainer).not.toBeNull();
        const icon = iconContainer!.querySelector(`i.${CALLOUT_ICON}`);
        expect(icon).not.toBeNull();
    });

    it('omits the icon container when icon=false', () => {
        const { container } = mount([calloutAst({ icon: false })]);
        expect(container.querySelector(`.${CALLOUT_ICON_CONTAINER}`)).toBeNull();
    });

    it('falls back to the capitalized type as default title when title slot is absent', () => {
        const { container } = mount([calloutAst({ type: 'tip', title: undefined })]);
        const titleContainer = container.querySelector(`.${CALLOUT_TITLE_CONTAINER}`);
        expect(titleContainer).not.toBeNull();
        expect(titleContainer!.textContent).toContain('Tip');
    });

    it('falls back to the default when title slot is empty Inlines', () => {
        const { container } = mount([calloutAst({ type: 'warning', title: [] })]);
        const titleContainer = container.querySelector(`.${CALLOUT_TITLE_CONTAINER}`);
        expect(titleContainer!.textContent).toContain('Warning');
    });

    it('uses a whitespace-only authored title verbatim (matches Rust inlines.is_empty rule)', () => {
        const { container } = mount([calloutAst({ type: 'note', title: [STR(' ')] })]);
        const titleContainer = container.querySelector(`.${CALLOUT_TITLE_CONTAINER}`);
        // Whitespace-only authored title still wins over the default.
        // Asserts the title rendering went through the user slot path,
        // not the default branch which would output "Note".
        expect(titleContainer!.textContent).not.toContain('Note');
    });

    it('emits callout-appearance-{a} class for non-default appearance', () => {
        const { container } = mount([calloutAst({ appearance: 'simple' })]);
        const callout = container.querySelector('div.callout');
        expect(callout!.classList.contains('callout-appearance-simple')).toBe(true);
    });

    it('omits callout-appearance class for the default appearance', () => {
        const { container } = mount([calloutAst({ appearance: 'default' })]);
        const callout = container.querySelector('div.callout');
        const hasAppearance = Array.from(callout!.classList).some((c) =>
            c.startsWith('callout-appearance-'),
        );
        expect(hasAppearance).toBe(false);
    });

    it('omits id attribute when attr id is empty', () => {
        const { container } = mount([calloutAst({ id: '' })]);
        const callout = container.querySelector('div.callout');
        expect(callout!.hasAttribute('id')).toBe(false);
    });

    it('honors id attribute when attr id is non-empty', () => {
        const { container } = mount([calloutAst({ id: 'cal-1' })]);
        const callout = container.querySelector('div.callout');
        expect(callout!.id).toBe('cal-1');
    });
});

describe('Theorem', () => {
    it('renders <div class="theorem"> + label inside <p><span.theorem-title><strong>...</strong></span>', () => {
        const { container } = mount([
            theoremAst({ refType: 'thm', kind: 'Theorem', order: 1, identifier: 'thm-1', content: [PARA(STR('body'))] }),
        ]);
        const div = container.querySelector(`div.${THEOREM}`);
        expect(div).not.toBeNull();
        expect(div!.id).toBe('thm-1');
        const span = div!.querySelector(`p > span.${THEOREM_TITLE} > strong`);
        expect(span).not.toBeNull();
        expect(span!.textContent).toContain('Theorem');
        expect(span!.textContent).toContain('1');
    });

    it('joins kind and number with NBSP (\\u00a0)', () => {
        const { container } = mount([
            theoremAst({ refType: 'thm', kind: 'Theorem', order: 5, content: [PARA(STR('body'))] }),
        ]);
        const strong = container.querySelector('span.theorem-title > strong');
        expect(strong!.textContent).toBe('Theorem 5');
    });

    it('adds env class for non-thm ref_types (lemma)', () => {
        const { container } = mount([
            theoremAst({ refType: 'lem', kind: 'Lemma', order: 2, content: [PARA(STR('body'))] }),
        ]);
        const div = container.querySelector(`div.${THEOREM}`);
        expect(div!.classList.contains('lemma')).toBe(true);
        expect(div!.classList.contains('theorem')).toBe(true);
    });

    it('skips env when refType is "thm" (env === theorem)', () => {
        const { container } = mount([theoremAst({ refType: 'thm', order: 1, content: [PARA(STR('b'))] })]);
        const div = container.querySelector(`div.${THEOREM}`);
        // Only the `theorem` class — no duplicate `theorem` token.
        const tokens = Array.from(div!.classList);
        expect(tokens.filter((t) => t === 'theorem').length).toBe(1);
    });

    it('renders an authored title in parentheses inside the label', () => {
        const { container } = mount([
            theoremAst({
                refType: 'thm',
                kind: 'Theorem',
                order: 3,
                title: [STR('Pythagoras')],
                content: [PARA(STR('body'))],
            }),
        ]);
        const strong = container.querySelector('span.theorem-title > strong');
        expect(strong!.textContent).toContain('(Pythagoras)');
    });

    it('elides the number when plain_data.order is missing', () => {
        const { container } = mount([
            theoremAst({ refType: 'thm', kind: 'Theorem', content: [PARA(STR('body'))] }),
        ]);
        const strong = container.querySelector('span.theorem-title > strong');
        expect(strong!.textContent).toBe('Theorem');
    });

    it('omits id attribute when identifier is empty', () => {
        const { container } = mount([theoremAst({ refType: 'thm', identifier: '', content: [PARA(STR('b'))] })]);
        const div = container.querySelector(`div.${THEOREM}`);
        expect(div!.hasAttribute('id')).toBe(false);
    });
});

describe('Proof', () => {
    it('renders <em>Proof.</em> as the default label and DOES NOT have a proof-title class', () => {
        const { container } = mount([proofAst({ content: [PARA(STR('body'))] })]);
        const div = container.querySelector(`div.${PROOF}`);
        expect(div).not.toBeNull();
        // Label inside first <p>
        const em = div!.querySelector('p > em');
        expect(em).not.toBeNull();
        expect(em!.textContent).toBe('Proof.');
        // No proof-title class anywhere.
        expect(container.querySelector('.proof-title')).toBeNull();
    });

    it('renders an authored title with appended period inside <em>', () => {
        const { container } = mount([
            proofAst({ title: [STR('Custom title')], content: [PARA(STR('body'))] }),
        ]);
        const em = container.querySelector(`div.${PROOF} > p > em`);
        expect(em!.textContent).toBe('Custom title.');
    });
});

describe('FloatRefTarget', () => {
    it('emits <figure> for ref_type "fig"', () => {
        const { container } = mount([
            floatRefAst({
                refType: 'fig',
                kind: 'Figure',
                identifier: 'fig-1',
                order: 1,
                content: [PARA(STR('image-placeholder'))],
                captionLong: [PARA(STR('cap'))],
            }),
        ]);
        const fig = container.querySelector('figure');
        expect(fig).not.toBeNull();
        expect(fig!.id).toBe('fig-1');
        const figcap = fig!.querySelector('figcaption');
        expect(figcap).not.toBeNull();
        expect(figcap!.textContent).toContain('Figure 1: cap');
    });

    it('emits <div> for non-figure ref_type (tbl)', () => {
        const { container } = mount([
            floatRefAst({
                refType: 'tbl',
                kind: 'Table',
                identifier: 'tbl-1',
                order: 1,
                content: [PARA(STR('table-placeholder'))],
                captionLong: [PARA(STR('tcap'))],
            }),
        ]);
        // No <figure> — <div> wrapper.
        expect(container.querySelector('figure')).toBeNull();
        // The id sits on the wrapper.
        const div = container.querySelector('div#tbl-1');
        expect(div).not.toBeNull();
        // Caption is appended (not wrapped in figcaption).
        expect(div!.textContent).toContain('Table 1: tcap');
    });

    it('uses ASCII space (NOT NBSP) between kind and number in the caption prefix', () => {
        const { container } = mount([
            floatRefAst({
                refType: 'fig',
                kind: 'Figure',
                identifier: 'fig-2',
                order: 2,
                content: [],
                captionLong: [PARA(STR('caption'))],
            }),
        ]);
        const figcap = container.querySelector('figcaption');
        // Regular space, not NBSP — distinguishing from Theorem.
        expect(figcap!.textContent!.startsWith('Figure 2:')).toBe(true);
        expect(figcap!.textContent).not.toContain('Figure 2');
    });
});

describe('Equation', () => {
    it('appends \\tag{N} to the LaTeX when plain_data.order is set', () => {
        const { container } = mount([
            PARA(equationAst({ latex: 'a^2 + b^2 = c^2', order: 1, identifier: 'eq-pyth' })),
        ]);
        const span = container.querySelector('span#eq-pyth');
        expect(span).not.toBeNull();
        // KaTeX renders \tag{N} as a side-floated number; the rendered
        // tree contains a `<span class="tag">` whose textContent is the
        // parenthesized number. Splitting by individual character spans
        // means we check textContent rather than innerHTML.
        const tagEl = span!.querySelector('.tag');
        expect(tagEl).not.toBeNull();
        expect(tagEl!.textContent).toBe('(1)');
    });

    it('does NOT append \\tag when order is missing', () => {
        const { container } = mount([
            PARA(equationAst({ latex: 'a + b', identifier: 'eq-x' })),
        ]);
        const span = container.querySelector('span#eq-x');
        expect(span).not.toBeNull();
        // No KaTeX-emitted `.tag` wrapper when no \tag{} command was
        // appended to the latex.
        expect(span!.querySelector('.tag')).toBeNull();
    });

    it('renders empty <span id> for an empty content slot (defensive branch 1)', () => {
        const ast = [
            PARA({
                t: 'CustomInline',
                type_name: 'Equation',
                slots: {
                    content: { kind: 'inlines', value: [] },
                },
                plain_data: { ref_type: 'eq', kind: 'Equation', identifier: 'eq-empty' },
                attr: ['eq-empty', [], []],
            }),
        ];
        const { container } = mount(ast);
        const span = container.querySelector('span#eq-empty');
        expect(span).not.toBeNull();
        expect(span!.textContent).toBe('');
    });

    it('warns and renders verbatim when first inline is non-canonical (defensive branch 3)', () => {
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
        try {
            const ast = [
                PARA({
                    t: 'CustomInline',
                    type_name: 'Equation',
                    slots: {
                        content: {
                            kind: 'inlines',
                            value: [
                                { t: 'Math', c: [{ t: 'InlineMath' }, 'x'] },
                            ],
                        },
                    },
                    plain_data: {
                        ref_type: 'eq', kind: 'Equation', identifier: 'eq-bad',
                        order: { section: [], order: 9 },
                    },
                    attr: ['eq-bad', [], []],
                }),
            ];
            const { container } = mount(ast);
            const span = container.querySelector('span#eq-bad');
            expect(span).not.toBeNull();
            // No \tag{9} — number not appended.
            expect(span!.innerHTML).not.toContain('(9)');
            expect(warn).toHaveBeenCalledOnce();
            expect(warn.mock.calls[0][0]).toContain('Math(InlineMath)');
        } finally {
            warn.mockRestore();
        }
    });
});

describe('CrossrefResolvedRef', () => {
    it('renders <a class="quarto-xref" href="#id">{kind}\\u00a0{n}</a> for resolved refs', () => {
        const { container } = mount([
            PARA(crossrefRefAst({ identifier: 'fig-1', refType: 'fig', kind: 'Figure', resolved: true, order: 3 })),
        ]);
        const a = container.querySelector(`a.${QUARTO_XREF}`);
        expect(a).not.toBeNull();
        expect(a!.getAttribute('href')).toBe('#fig-1');
        expect(a!.textContent).toBe('Figure 3');
    });

    it('renders "?id?" link text for unresolved refs', () => {
        const { container } = mount([
            PARA(crossrefRefAst({ identifier: 'fig-missing', refType: 'fig', kind: 'Figure', resolved: false })),
        ]);
        const a = container.querySelector(`a.${QUARTO_XREF}`);
        expect(a!.textContent).toBe('?fig-missing?');
    });

    it('renders kind alone for resolved refs without an order', () => {
        const { container } = mount([
            PARA(crossrefRefAst({ identifier: 'fig-1', refType: 'fig', kind: 'Figure', resolved: true })),
        ]);
        const a = container.querySelector(`a.${QUARTO_XREF}`);
        expect(a!.textContent).toBe('Figure');
    });

    it('renders the suffix slot inlines after the link', () => {
        const { container } = mount([
            PARA(crossrefRefAst({
                identifier: 'fig-1', refType: 'fig', kind: 'Figure', resolved: true, order: 1,
                suffix: [STR(' (and onwards)')],
            })),
        ]);
        const p = container.querySelector('p');
        expect(p!.textContent).toBe('Figure 1 (and onwards)');
    });
});

describe('Fallback', () => {
    it('renders a styled box displaying the unknown type_name', () => {
        const { container } = mount([
            {
                t: 'CustomBlock',
                type_name: 'IncludeExpansion',
                slots: {
                    content: {
                        kind: 'blocks',
                        value: [PARA(STR('included content'))],
                    },
                },
                plain_data: null,
                attr: ['', [], []],
            },
        ]);
        // The fallback box contains the type name (IncludeExpansion is
        // not yet shipped — Plan 8).
        expect(container.textContent).toContain('IncludeExpansion');
        // And recurses into the slot via renderChildren.
        expect(container.textContent).toContain('included content');
    });
});

describe('Atomic CustomNode read-only (registry-routed)', () => {
    it('CrossrefResolvedRef rendered through previewRegistry receives a no-op setLocalAst', () => {
        // Capture the setLocalAst that the framework hands to the
        // CustomInline dispatcher. The atomic gate runs in framework's
        // <Node> *before* the dispatcher; we mount a capturing
        // CustomInline replacement to confirm it received NOOP.
        let capturedSetLocalAst: ((n: unknown) => void) | null = null;
        const Capturing = (args: NodeArgs<any>) => {
            capturedSetLocalAst = args.setLocalAst as (n: unknown) => void;
            return <span>captured-{args.node.type_name}</span>;
        };
        const merged: FormatRegistry = {
            ...previewRegistry,
            CustomInline: Capturing,
        } as FormatRegistry;

        let parentSetCalls = 0;
        const ast = [
            PARA(crossrefRefAst({
                identifier: 'fig-1', refType: 'fig', kind: 'Figure', resolved: true, order: 1,
            })),
        ];
        render(
            <Ast
                astJson={astJson(ast)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={() => { parentSetCalls += 1; }}
                registry={merged}
            />,
        );
        expect(capturedSetLocalAst).not.toBeNull();
        // Calling the captured setLocalAst should be a no-op.
        capturedSetLocalAst!({ t: 'CustomInline', type_name: 'CrossrefResolvedRef', slots: {}, plain_data: {}, attr: ['', [], []] });
        expect(parentSetCalls).toBe(0);
    });
});

describe('User overrides', () => {
    it('user TSX export of Pandoc tag (Para) wins over the built-in', () => {
        const MyPara = (args: NodeArgs<any>) => (
            <p className="my-para">{args.node.c.map((inl: any, i: number) => <span key={i}>{inl.c ?? ''}</span>)}</p>
        );
        const merged: FormatRegistry = {
            ...previewRegistry,
            Para: MyPara,
        } as FormatRegistry;
        const { container } = mount([PARA(STR('hi'))], merged);
        const p = container.querySelector('p.my-para');
        expect(p).not.toBeNull();
        expect(p!.textContent).toBe('hi');
    });

    it('user TSX export of CustomNode type_name (Callout) wins over the built-in', () => {
        const MyCallout = (args: NodeArgs<any>) => <div className="my-callout">overridden</div>;
        const merged: FormatRegistry = {
            ...previewRegistry,
            Callout: MyCallout,
        } as FormatRegistry;
        const { container } = mount(
            [calloutAst({ type: 'note', content: [PARA(STR('body'))] })],
            merged,
        );
        const myDiv = container.querySelector('div.my-callout');
        expect(myDiv).not.toBeNull();
        expect(myDiv!.textContent).toBe('overridden');
        // Built-in callout class should NOT be present.
        expect(container.querySelector('div.callout')).toBeNull();
    });

    it('Pandoc tag override + CustomNode override fire simultaneously', () => {
        const MyPara = (args: NodeArgs<any>) => <p className="my-para">{args.node.c.map((inl: any, i: number) => <span key={i}>{inl.c ?? ''}</span>)}</p>;
        const MyCallout = () => <div className="my-callout">cb</div>;
        const merged: FormatRegistry = {
            ...previewRegistry,
            Para: MyPara,
            Callout: MyCallout,
        } as FormatRegistry;
        const { container } = mount(
            [
                PARA(STR('hi')),
                calloutAst({ content: [PARA(STR('body'))] }),
            ],
            merged,
        );
        expect(container.querySelector('p.my-para')).not.toBeNull();
        expect(container.querySelector('div.my-callout')).not.toBeNull();
    });
});

describe('Class-compatibility (post-2C)', () => {
    it('built-in classes match the quartoClasses constants', () => {
        const { container } = mount([
            calloutAst({ type: 'note', content: [PARA(STR('b'))] }),
        ]);
        // Sanity: the Callout component emits CALLOUT_HEADER /
        // CALLOUT_TITLE_CONTAINER / CALLOUT_BODY_CONTAINER as
        // declared in quartoClasses.ts. If quartoClasses changes
        // and the components don't follow, this test fails.
        expect(container.querySelector(`.${CALLOUT_HEADER}`)).not.toBeNull();
        expect(container.querySelector(`.${CALLOUT_TITLE_CONTAINER}`)).not.toBeNull();
        expect(container.querySelector(`.${CALLOUT_BODY_CONTAINER}`)).not.toBeNull();
    });

    it('Theorem emits THEOREM and THEOREM_TITLE classes', () => {
        const { container } = mount([
            theoremAst({ refType: 'thm', order: 1, content: [PARA(STR('b'))] }),
        ]);
        expect(container.querySelector(`div.${THEOREM}`)).not.toBeNull();
        expect(container.querySelector(`span.${THEOREM_TITLE}`)).not.toBeNull();
    });

    it('CrossrefResolvedRef emits the QUARTO_XREF class', () => {
        const { container } = mount([
            PARA(crossrefRefAst({ identifier: 'fig-1', refType: 'fig', kind: 'Figure', resolved: true, order: 1 })),
        ]);
        expect(container.querySelector(`a.${QUARTO_XREF}`)).not.toBeNull();
    });
});
