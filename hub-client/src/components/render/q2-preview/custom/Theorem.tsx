import type {
    BlockNode,
    CustomBlockNode,
    InlineNode,
    NodeArgs,
    ParaBlock,
} from '@quarto/preview-renderer/framework';
import { Node } from '@quarto/preview-renderer/framework';
import { THEOREM, THEOREM_TITLE } from '../quartoClasses';
import { theoremEnvFor } from '../theoremEnvs';
import { makeSlotSetter } from '../utils';

/**
 * Theorem — q2-preview port of `render_theorem` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:321-378`.
 *
 * Output structure:
 * ```
 * <div [id] class="theorem [<env>]">
 *   <p>
 *     <span class="theorem-title">
 *       <strong>{kind}\u{a0}{n}[ ({title inlines})]</strong>
 *     </span>{ }{first paragraph's inlines}
 *   </p>
 *   {remaining content blocks}
 * </div>
 * ```
 *
 * Mirrors `theorem_label_inlines` (`crossref_render.rs:432-497`):
 *  - kind + NBSP + number, then optional " (" + title-inlines + ")"
 *  - whole label sits inside one Strong; label is then wrapped in
 *    Span(class="theorem-title") with a trailing Str(" ") outside.
 *
 * `plain_data` (writer: `transforms/theorem.rs:282`):
 *  - `ref_type`, `kind`, `identifier`, optional `order: { section, order }`.
 *
 * Env-class rule (`crossref_render.rs:346-352`):
 *  - always include `theorem`,
 *  - additionally include `theoremEnvFor(refType)` when it is non-empty,
 *    not "theorem", and not already on the attr.
 *
 * The label is rendered as JSX. Title slot inlines are dispatched
 * through `<Node>` so the atomic gate + edit chain survive; the
 * surrounding `"("` / `")"` separators are static JSX (they are
 * synthesized labels, not user-authored content). Body inlines and
 * remaining blocks are dispatched through `<Node>` with `setLocalAst`
 * callbacks that thread back through the content slot.
 */

interface TheoremPlainData {
    ref_type?: string;
    kind?: string;
    identifier?: string;
    order?: { section?: number[]; order?: number };
}

export const Theorem = ({ node, onNavigateToDocument, setLocalAst }: NodeArgs<CustomBlockNode>) => {
    const plain = (node.plain_data ?? {}) as TheoremPlainData;
    const refType = plain.ref_type ?? '';
    const kind = plain.kind ?? '';
    const number = plain.order?.order;

    const titleSlot = node.slots.title;
    const titleInlines: InlineNode[] | undefined =
        titleSlot && titleSlot.kind === 'inlines' && titleSlot.value.length > 0
            ? titleSlot.value
            : undefined;

    // Class list: user attr classes + 'theorem' + (env unless skip)
    const userClasses = node.attr[1];
    const classList = [...userClasses];
    if (!classList.includes(THEOREM)) classList.push(THEOREM);
    const env = theoremEnvFor(refType);
    if (env && env !== THEOREM && !classList.includes(env)) classList.push(env);

    const id = node.attr[0];
    const setSlot = makeSlotSetter(node, setLocalAst);

    // Decompose the content slot into "first Paragraph" and "rest".
    // If the first block isn't a Paragraph (or content is empty), the
    // Rust pipeline synthesizes a `Paragraph(Str("\u{a0}"))`; we mirror
    // that purely for layout so the label has somewhere to live.
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

    // setLocalAst for the i-th inline of the first paragraph: rebuilds
    // the Para and the surrounding content slot. When the first block
    // wasn't actually a Paragraph in the source AST, this is a no-op
    // (firstParaInlines is empty so this map never iterates).
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

    // Compose the head text (kind + NBSP + number). All-Strong, all bold.
    let headText = '';
    if (kind) headText = kind;
    if (number !== undefined) {
        if (headText) headText += ' ';
        headText += String(number);
    }

    return (
        <div className={classList.join(' ')} id={id || undefined}>
            <p>
                <span className={THEOREM_TITLE}>
                    <strong>
                        {headText}
                        {titleInlines && (
                            <>
                                {' ('}
                                {titleInlines.map((inl, i) => (
                                    <Node
                                        key={i}
                                        node={inl}
                                        onNavigateToDocument={onNavigateToDocument}
                                        setLocalAst={setTitleInline(i)}
                                    />
                                ))}
                                {')'}
                            </>
                        )}
                    </strong>
                </span>
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
