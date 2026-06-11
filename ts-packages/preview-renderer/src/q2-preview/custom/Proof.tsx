import { useContext } from 'react';
import type {
    BlockNode,
    CustomBlockNode,
    InlineNode,
    NodeArgs,
    ParaBlock,
} from '../../framework';
import { Node } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { PROOF } from '../quartoClasses';
import { makeSlotSetter } from '../utils';

/**
 * Proof — q2-preview port of `render_proof` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:534-585`.
 *
 * Output structure:
 * ```
 * <div [id] class="proof">
 *   <p><em>Proof.</em> {first paragraph's inlines}</p>
 *   {remaining content blocks}
 * </div>
 * ```
 *
 * No `proof-title` class — the label is an inline italic
 * `<em>Proof.</em>` (or the user's title with a literal `.`
 * appended) prepended to the body's first paragraph. Default label
 * text is the literal `"Proof."` (period included).
 *
 * `plain_data` (writer: `transforms/proof.rs:145`):
 *   - `kind` (string, hardcoded `"Proof"`): not used by render —
 *     the displayed label is `"Proof."` regardless.
 *
 * Identifier rule mirrors Theorem / FloatRefTarget — empty
 * `node.attr[0]` produces no `id` attribute on the wrapper.
 */

export const Proof = ({ node, onNavigateToDocument, setLocalAst }: NodeArgs<CustomBlockNode>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    const titleSlot = node.slots.title;
    const titleInlines: InlineNode[] | undefined =
        titleSlot && titleSlot.kind === 'inlines' && titleSlot.value.length > 0
            ? titleSlot.value
            : undefined;

    const userClasses = node.attr[1];
    const classList = [...userClasses];
    if (!classList.includes(PROOF)) classList.push(PROOF);

    const id = node.attr[0];

    const setSlot = makeSlotSetter(node, setLocalAst);

    const contentSlot = node.slots.content;
    const blocks: BlockNode[] =
        contentSlot && contentSlot.kind === 'blocks' ? contentSlot.value : [];
    const firstIsPara = blocks.length > 0 && blocks[0].t === 'Para';
    const firstParaInlines: InlineNode[] = firstIsPara
        ? (blocks[0] as ParaBlock).c
        : [];
    const remaining: BlockNode[] = firstIsPara ? blocks.slice(1) : blocks;

    const replaceContentBlocks = (newBlocks: BlockNode[]) =>
        setSlot('content')({ kind: 'blocks', value: newBlocks });

    const setFirstParaInline = (i: number) => (newInline: BlockNode | InlineNode) => {
        const nextInlines = firstParaInlines.slice();
        nextInlines[i] = newInline as InlineNode;
        const para: ParaBlock = { t: 'Para', c: nextInlines };
        const nextBlocks = blocks.slice();
        nextBlocks[0] = para;
        replaceContentBlocks(nextBlocks);
    };

    const setRemainingBlock = (i: number) => (newBlock: BlockNode | InlineNode) => {
        const nextBlocks = blocks.slice();
        const targetIdx = firstIsPara ? i + 1 : i;
        nextBlocks[targetIdx] = newBlock as BlockNode;
        replaceContentBlocks(nextBlocks);
    };

    const setTitleInline = (i: number) => (newInline: BlockNode | InlineNode) => {
        if (!titleInlines) return;
        const next = titleInlines.slice();
        next[i] = newInline as InlineNode;
        setSlot('title')({ kind: 'inlines', value: next });
    };

    return (
        <div className={classList.join(' ')} id={id || undefined} {...affordanceAttr}>
            <p>
                <em>
                    {titleInlines ? (
                        <>
                            {titleInlines.map((inl, i) => (
                                <Node
                                    key={i}
                                    node={inl}
                                    onNavigateToDocument={onNavigateToDocument}
                                    setLocalAst={setTitleInline(i)}
                                />
                            ))}
                            {'.'}
                        </>
                    ) : (
                        'Proof.'
                    )}
                </em>
                {' '}
                {firstParaInlines.map((inl, i) => (
                    <Node
                        key={i}
                        node={inl}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={setFirstParaInline(i)}
                    />
                ))}
            </p>
            {remaining.map((b, i) => (
                <Node
                    key={i}
                    node={b}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setRemainingBlock(i)}
                />
            ))}
        </div>
    );
};
