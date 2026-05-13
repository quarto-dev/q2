import type {
    BlockNode,
    CustomBlockNode,
    InlineNode,
    NodeArgs,
    ParaBlock,
} from '../../framework';
import { Node } from '../../framework';
import { makeSlotSetter } from '../utils';

/**
 * FloatRefTarget — q2-preview port of `render_floatreftarget` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:225-291`.
 *
 * Output discriminator on `plain_data.ref_type`:
 *  - `"fig"` → native `<figure>` with the body content followed by a
 *    `<figcaption>` carrying the prefixed caption.
 *  - any other ref_type (`"tbl"`, `"lst"`, user-defined) → `<div>` with
 *    the body content followed by the prefixed-caption blocks (no
 *    `<figcaption>` wrap).
 *
 * No classes added by the wrapper — the user's authored attr passes
 * through unchanged.
 *
 * `plain_data` (writer: `transforms/float_ref_target.rs:292-295`):
 *   - `ref_type`, `kind`, `identifier`, optional `order: { section, order }`.
 *
 * **Caption prefix format** (mirrors `prefix_caption` at
 * `crossref_render.rs:721-742`):
 *   - With number: `"{kind} {n}: "` — ASCII space, **not** NBSP
 *     (Theorem uses NBSP; FloatRefTarget does not).
 *   - Without number: `"{kind}: "`.
 *   - No-op cases: `kind` empty OR caption is empty Blocks → caption
 *     unchanged.
 *   - Prepend lands as a single `Str` (with trailing space inside the
 *     same Str) at the head of the caption's first Paragraph; if the
 *     first caption block isn't a Paragraph, the prefix is dropped
 *     silently. Match Rust behavior.
 */

interface FloatRefTargetPlainData {
    ref_type?: string;
    kind?: string;
    identifier?: string;
    order?: { section?: number[]; order?: number };
}

export const FloatRefTarget = ({
    node,
    onNavigateToDocument,
    setLocalAst,
}: NodeArgs<CustomBlockNode>) => {
    const plain = (node.plain_data ?? {}) as FloatRefTargetPlainData;
    const refType = plain.ref_type ?? '';
    const kind = plain.kind ?? '';
    const number = plain.order?.order;

    const id = node.attr[0];
    const setSlot = makeSlotSetter(node, setLocalAst);

    const contentSlot = node.slots.content;
    const contentBlocks: BlockNode[] =
        contentSlot && contentSlot.kind === 'blocks' ? contentSlot.value : [];

    const captionLongSlot = node.slots.caption_long;
    const captionLongBlocks: BlockNode[] =
        captionLongSlot && captionLongSlot.kind === 'blocks'
            ? captionLongSlot.value
            : [];

    // Compose the caption-prefix Str.
    const prefixText = composePrefixText(kind, number);
    // Apply the prefix into the first Paragraph of caption_long (if any).
    // Returns the (virtually) prefixed blocks for rendering, plus a
    // pointer to the unmodified-source first inline so the per-inline
    // setLocalAst doesn't need to know about the synthetic prefix.
    const firstCaptionPara =
        captionLongBlocks.length > 0 && captionLongBlocks[0].t === 'Para'
            ? (captionLongBlocks[0] as ParaBlock)
            : undefined;
    const remainingCaptionBlocks: BlockNode[] = firstCaptionPara
        ? captionLongBlocks.slice(1)
        : captionLongBlocks;

    const replaceContentBlocks = (newBlocks: BlockNode[]) =>
        setSlot('content')({ kind: 'blocks', value: newBlocks });
    const setContentBlock = (i: number) => (newBlock: BlockNode | InlineNode) => {
        const next = contentBlocks.slice();
        next[i] = newBlock as BlockNode;
        replaceContentBlocks(next);
    };

    const replaceCaptionBlocks = (newBlocks: BlockNode[]) =>
        setSlot('caption_long')({ kind: 'blocks', value: newBlocks });
    const setFirstCaptionInline = (i: number) => (newInline: BlockNode | InlineNode) => {
        if (!firstCaptionPara) return;
        const nextInlines = firstCaptionPara.c.slice();
        nextInlines[i] = newInline as InlineNode;
        const nextPara: ParaBlock = { t: 'Para', c: nextInlines };
        const nextCaption = captionLongBlocks.slice();
        nextCaption[0] = nextPara;
        replaceCaptionBlocks(nextCaption);
    };
    const setRemainingCaptionBlock = (i: number) => (newBlock: BlockNode | InlineNode) => {
        const nextCaption = captionLongBlocks.slice();
        const targetIdx = firstCaptionPara ? i + 1 : i;
        nextCaption[targetIdx] = newBlock as BlockNode;
        replaceCaptionBlocks(nextCaption);
    };

    // Caption JSX: <prefix>{first-Para inlines via Node} then remaining
    // caption blocks via Node. Nothing is rendered if caption_long is
    // empty.
    const captionJsx =
        captionLongBlocks.length === 0 ? null : (
            <>
                {firstCaptionPara ? (
                    <p>
                        {prefixText}
                        {firstCaptionPara.c.map((inl, i) => (
                            <Node
                                key={i}
                                node={inl}
                                onNavigateToDocument={onNavigateToDocument}
                                setLocalAst={setFirstCaptionInline(i)}
                            />
                        ))}
                    </p>
                ) : null}
                {remainingCaptionBlocks.map((b, i) => (
                    <Node
                        key={i}
                        node={b}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={setRemainingCaptionBlock(i)}
                    />
                ))}
            </>
        );

    const bodyJsx = contentBlocks.map((b, i) => (
        <Node
            key={i}
            node={b}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={setContentBlock(i)}
        />
    ));

    if (refType === 'fig') {
        return (
            <figure id={id || undefined}>
                {bodyJsx}
                {captionJsx ? <figcaption>{captionJsx}</figcaption> : null}
            </figure>
        );
    }

    return (
        <div id={id || undefined}>
            {bodyJsx}
            {captionJsx}
        </div>
    );
};

function composePrefixText(kind: string, number: number | undefined): string {
    if (!kind) return '';
    return number !== undefined ? `${kind} ${number}: ` : `${kind}: `;
}
