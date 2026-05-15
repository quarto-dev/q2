import type {
    BlockNode,
    CustomInlineNode,
    InlineNode,
    NodeArgs,
} from '../../framework';
import { Node } from '../../framework';
import { QUARTO_XREF } from '../quartoClasses';

/**
 * CrossrefResolvedRef — q2-preview port of `render_resolved_ref` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:657-715`.
 *
 * Output: `<a class="quarto-xref" href="#{identifier}">{kind} {n}</a>{slot.suffix}`.
 *
 * Link-text rule:
 *   - `resolved && order` → `"{kind}\u{a0}{n}"` (NBSP between kind and
 *     number — same as Theorem; matches `crossref_render.rs:691`).
 *   - `resolved && !order` → `kind` alone (rare; numbered targets
 *     always have order).
 *   - `!resolved` → `"?{identifier}?"` (broken-ref affordance; matches
 *     `:695`).
 *
 * **Atomic.** `isAtomicCustomNode("CrossrefResolvedRef") === true`
 * (`hub-client/src/utils/atomicCustomNodes.ts`); the framework's
 * atomic gate at `framework/dispatch.tsx:411` no-ops `setLocalAst`
 * before this component runs, so child setters here are effectively
 * pass-through. The component is read-only and renders the resolved
 * link as static JSX; the suffix slot still mounts via `<Node>` so its
 * inlines render through the registry, but writes from the suffix
 * cannot mutate the AST (atomic gate).
 *
 * `plain_data` (writer: `transforms/crossref_resolve.rs:316`):
 *  - `identifier`, `ref_type`, `kind`, `resolved` (bool),
 *    `kind_source` (unused in render),
 *    optional `order: { section, order }`.
 */

interface CrossrefResolvedRefPlainData {
    identifier?: string;
    ref_type?: string;
    kind?: string;
    resolved?: boolean;
    kind_source?: string;
    order?: { section?: number[]; order?: number };
}

export const CrossrefResolvedRef = ({
    node,
    onNavigateToDocument,
    setLocalAst,
}: NodeArgs<CustomInlineNode>) => {
    const plain = (node.plain_data ?? {}) as CrossrefResolvedRefPlainData;
    const identifier = plain.identifier ?? '';
    const kind = plain.kind ?? '';
    const resolved = plain.resolved === true;
    const number = plain.order?.order;

    let linkText: string;
    if (!resolved) {
        linkText = `?${identifier}?`;
    } else if (number !== undefined) {
        linkText = `${kind} ${number}`;
    } else {
        linkText = kind;
    }

    const suffixSlot = node.slots.suffix;
    const suffixInlines: InlineNode[] =
        suffixSlot && suffixSlot.kind === 'inlines' ? suffixSlot.value : [];

    // Atomic — setLocalAst is no-op'd by the framework gate, but for
    // structural symmetry with the framework's renderCustomNodeChildren
    // walk we still build per-child setters. They never propagate.
    const setSuffixInline = (i: number) => (newInline: BlockNode | InlineNode) => {
        const next = suffixInlines.slice();
        next[i] = newInline as InlineNode;
        setLocalAst({
            ...node,
            slots: {
                ...node.slots,
                suffix: { kind: 'inlines', value: next },
            },
        });
    };

    return (
        <>
            <a className={QUARTO_XREF} href={`#${identifier}`}>
                {linkText}
            </a>
            {suffixInlines.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setSuffixInline(i)}
                />
            ))}
        </>
    );
};
