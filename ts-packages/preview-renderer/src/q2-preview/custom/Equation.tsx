import type {
    BlockNode,
    CustomInlineNode,
    InlineNode,
    MathInline,
    NodeArgs,
} from '../../framework';
import { Node } from '../../framework';
import { makeSlotSetter } from '../utils';

/**
 * Equation — q2-preview port of `render_equation` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:601-650`.
 *
 * `CrossrefRenderTransform` is excluded from q2-preview's pipeline
 * (see `Q2_PREVIEW_TRANSFORM_EXCLUDED` at `pipeline.rs:1071`), so the
 * `Equation` CustomNode wrapper survives into the iframe. q2-preview
 * ports the `\tag{N}` append from Rust into JS so KaTeX can render
 * the equation number natively.
 *
 * Output: `<span id="{identifier}">{Math (with \tag{N} appended)}</span>`.
 *
 * Defensive-fallback branches for non-canonical slot contents
 * (per plan §"`Equation.tsx` — `type_name: \"Equation\"`"):
 *   1. Empty/missing content slot → render `<span id={id}/>` empty,
 *      no warn.
 *   2. First inline is `Math(DisplayMath, ...)` (canonical) → append
 *      `\tag{N}` to its LaTeX (when number is set), wrap. Trailing
 *      siblings, if any, render through `<Node>` after the math.
 *   3. First inline is anything else (`Math(InlineMath)`, `Str`,
 *      `Span`, ...) → warn once, render every inline through `<Node>`
 *      unchanged. No `\tag{N}` append (the tag is meaningless inside
 *      flowing inline text and absurd inside `Math(InlineMath)`).
 */

interface EquationPlainData {
    ref_type?: string;
    kind?: string;
    identifier?: string;
    order?: { section?: number[]; order?: number };
}

function isCanonicalDisplayMath(inl: InlineNode): inl is MathInline {
    if (inl.t !== 'Math') return false;
    const math = inl as MathInline;
    return math.c?.[0]?.t === 'DisplayMath';
}

function tagInline(math: MathInline, number: number): MathInline {
    return {
        t: 'Math',
        c: [{ t: 'DisplayMath' }, `${math.c[1]}\\tag{${number}}`],
    };
}

export const Equation = ({
    node,
    onNavigateToDocument,
    setLocalAst,
}: NodeArgs<CustomInlineNode>) => {
    const plain = (node.plain_data ?? {}) as EquationPlainData;
    const number = plain.order?.order;

    const id = node.attr[0];
    const setSlot = makeSlotSetter(node, setLocalAst);

    const contentSlot = node.slots.content;
    const inlines: InlineNode[] =
        contentSlot && contentSlot.kind === 'inlines' ? contentSlot.value : [];

    const replaceInlines = (newInlines: InlineNode[]) =>
        setSlot('content')({ kind: 'inlines', value: newInlines });

    // Branch 1: empty content slot.
    if (inlines.length === 0) {
        return <span id={id || undefined} />;
    }

    const first = inlines[0];

    // Branch 2: canonical DisplayMath as the first inline.
    if (isCanonicalDisplayMath(first)) {
        const taggedFirst: InlineNode =
            number !== undefined ? tagInline(first, number) : first;

        // setLocalAst per inline: original inline at i (untagged) is
        // replaced. The synthetic-tagged form is render-only; edits
        // flow back through the source inline.
        const setInlineAt = (i: number) => (newInline: InlineNode) => {
            const next = inlines.slice();
            next[i] = newInline;
            replaceInlines(next);
        };

        return (
            <span id={id || undefined}>
                <Node
                    node={taggedFirst}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setInlineAt(0) as (n: InlineNode | BlockNode) => void}
                />
                {inlines.slice(1).map((inl, i) => (
                    <Node
                        key={i + 1}
                        node={inl}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={setInlineAt(i + 1) as (n: InlineNode | BlockNode) => void}
                    />
                ))}
            </span>
        );
    }

    // Branch 3: defensive fallback — non-canonical first inline.
    // Warn once per render so a future transform regression surfaces
    // in the dev console without breaking the document.
    let firstTag: string;
    if (first.t === 'Math') {
        const mathType = (first as MathInline).c?.[0]?.t ?? '?';
        firstTag = `Math(${mathType})`;
    } else {
        firstTag = first.t;
    }
    console.warn(
        `[q2-preview Equation] expected Math(DisplayMath) as first inline, got ${firstTag}; rendering inlines verbatim, no \\tag append.`,
    );

    const setInlineAt = (i: number) => (newInline: InlineNode) => {
        const next = inlines.slice();
        next[i] = newInline;
        replaceInlines(next);
    };

    return (
        <span id={id || undefined}>
            {inlines.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setInlineAt(i) as (n: InlineNode | BlockNode) => void}
                />
            ))}
        </span>
    );
};
