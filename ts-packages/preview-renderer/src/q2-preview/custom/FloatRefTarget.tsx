import { useContext } from 'react';
import type {
    BlockNode,
    CustomBlockNode,
    InlineNode,
    NodeArgs,
    ParaBlock,
    PlainBlock,
} from '../../framework';
import { Node } from '../../framework';
import { PreviewContext } from '../PreviewContext';
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
 * `plain_data` (writer: `transforms/float_ref_target.rs:292-295`; keys
 * mirrored from `crates/quarto-pandoc-types/resources/custom-node-schema.json`):
 *   - `ref_type`, `kind`, `identifier`, optional `order: { section, order }`.
 *
 * The key array below is asserted against the schema (both directions)
 * by `schemaConformance.test.ts`'s T4.3.
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

export const FLOAT_REF_TARGET_PLAIN_DATA_KEYS = ['ref_type', 'kind', 'identifier', 'order'] as const;

type FloatRefTargetPlainData = { [K in (typeof FLOAT_REF_TARGET_PLAIN_DATA_KEYS)[number]]?: unknown };

export const FloatRefTarget = ({
    node,
    onNavigateToDocument,
    setLocalAst,
}: NodeArgs<CustomBlockNode>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    const plain = (node.plain_data ?? {}) as FloatRefTargetPlainData;
    const refType = (plain.ref_type as string | undefined) ?? '';
    const kind = (plain.kind as string | undefined) ?? '';
    const order = plain.order as { section?: number[]; order?: number } | undefined;
    const number = order?.order;

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
    // Apply the prefix into the first inline-bearing block of caption_long
    // (if any). That block is a Para for the div-form trailing paragraph
    // and a Plain for Pandoc-native Figure / Table captions
    // (`![cap](img){#fig-x}`, `: cap {#tbl-x}`); the prefix lands in
    // either, mirroring crossref_render's `prefix_caption` (bd-n3sark9b —
    // matching Para only used to drop the prefix for the Plain forms).
    // Returns the (virtually) prefixed blocks for rendering, plus a
    // pointer to the unmodified-source first inline so the per-inline
    // setLocalAst doesn't need to know about the synthetic prefix.
    const firstCaptionBlock: ParaBlock | PlainBlock | undefined =
        captionLongBlocks.length > 0 &&
        (captionLongBlocks[0].t === 'Para' || captionLongBlocks[0].t === 'Plain')
            ? (captionLongBlocks[0] as ParaBlock | PlainBlock)
            : undefined;
    const remainingCaptionBlocks: BlockNode[] = firstCaptionBlock
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
        if (!firstCaptionBlock) return;
        const nextInlines = firstCaptionBlock.c.slice();
        nextInlines[i] = newInline as InlineNode;
        // Preserve the block type: a Plain caption stays Plain.
        const nextBlock: ParaBlock | PlainBlock = { ...firstCaptionBlock, c: nextInlines };
        const nextCaption = captionLongBlocks.slice();
        nextCaption[0] = nextBlock;
        replaceCaptionBlocks(nextCaption);
    };
    const setRemainingCaptionBlock = (i: number) => (newBlock: BlockNode | InlineNode) => {
        const nextCaption = captionLongBlocks.slice();
        const targetIdx = firstCaptionBlock ? i + 1 : i;
        nextCaption[targetIdx] = newBlock as BlockNode;
        replaceCaptionBlocks(nextCaption);
    };

    // Caption JSX: <prefix>{first-block inlines via Node} then remaining
    // caption blocks via Node. A Para first block renders as <p>, a Plain
    // one as bare inlines — the same distinction the native HTML writer
    // draws inside <figcaption>. Nothing is rendered if caption_long is
    // empty.
    const firstCaptionInlinesJsx = firstCaptionBlock ? (
        <>
            {prefixText}
            {firstCaptionBlock.c.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setFirstCaptionInline(i)}
                />
            ))}
        </>
    ) : null;
    const captionJsx =
        captionLongBlocks.length === 0 ? null : (
            <>
                {firstCaptionBlock?.t === 'Para' ? (
                    <p>{firstCaptionInlinesJsx}</p>
                ) : (
                    firstCaptionInlinesJsx
                )}
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
            <figure id={id || undefined} {...affordanceAttr}>
                {bodyJsx}
                {captionJsx ? <figcaption>{captionJsx}</figcaption> : null}
            </figure>
        );
    }

    return (
        <div id={id || undefined} {...affordanceAttr}>
            {bodyJsx}
            {captionJsx}
        </div>
    );
};

function composePrefixText(kind: string, number: number | undefined): string {
    if (!kind) return '';
    return number !== undefined ? `${kind} ${number}: ` : `${kind}: `;
}
