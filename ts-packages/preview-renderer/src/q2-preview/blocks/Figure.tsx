import { useContext } from 'react';
import { Node, dataLocProps } from '../../framework';
import type { BlockNode, FigureBlock, InlineNode, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';

/**
 * Figure → <figure> + optional <figcaption>. Reads `c[1][1]`
 * (captionBlocks) and `c[2]` (bodyBlocks) directly rather than going
 * through `renderChildren`, since renderChildren only emits c[2].
 *
 * Crossref-numbered captions ("Figure 1: ...") are already baked into
 * the caption blocks by `CrossrefResolveTransform` upstream — q2-preview
 * gets that for free.
 *
 * `setLocalAst` for body blocks updates `c[2]` immutably; for caption
 * blocks updates `c[1][1]`. Both go through the framework's atomic gate
 * via `<Node>`.
 */
export const Figure = (args: NodeArgs<FigureBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;

    const { node, setLocalAst, onNavigateToDocument } = args;
    const [[id, classes, kvs], [shortCaption, captionBlocks], bodyBlocks] = node.c;
    const props: Record<string, string | number> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    if (isEditable) {
        props['data-block-pool-id'] = String(poolId);
        props.tabIndex = -1;
    }

    // Float figcaption synthesis (bd-hcp8m3ve): the crossref renderer carries
    // figcaption metadata as `data-qf-*` kvs on the Figure attr (Pandoc's
    // Caption has no attr). Mirror pampa's HTML writer exactly: consume the
    // kvs (never emit them), give the figcaption its id + Q1-verbatim class
    // list, and honor top/bottom placement. Contract:
    // claude-notes/designs/float-layout-class-taxonomy.md.
    const kvMap = new Map(kvs ?? []);
    const qfCaptionId = kvMap.get('data-qf-caption-id');
    const qfLocation = kvMap.get('data-qf-caption-location') ?? 'bottom';
    const figcaptionProps: Record<string, string> = {};
    if (qfCaptionId !== undefined) {
        const refType = kvMap.get('data-qf-ref-type') ?? 'fig';
        let captionClasses =
            `quarto-float-caption-${qfLocation} quarto-float-caption quarto-float-${refType}`;
        if (kvMap.has('data-qf-uncaptioned')) captionClasses += ' quarto-uncaptioned';
        figcaptionProps.id = qfCaptionId;
        figcaptionProps.className = captionClasses;
    }

    const body = bodyBlocks.map((b, i) => (
        <Node
            key={i}
            node={b}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={(newBlock: BlockNode | InlineNode) => {
                const next = bodyBlocks.slice();
                next[i] = newBlock as BlockNode;
                setLocalAst({ ...node, c: [node.c[0], node.c[1], next] });
            }}
        />
    ));
    const figcaption = captionBlocks.length > 0 && (
        <figcaption {...figcaptionProps}>
            {captionBlocks.map((b, i) => (
                <Node
                    key={i}
                    node={b}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const next = captionBlocks.slice();
                        next[i] = newBlock as BlockNode;
                        setLocalAst({
                            ...node,
                            c: [node.c[0], [shortCaption, next], node.c[2]],
                        });
                    }}
                />
            ))}
        </figcaption>
    );

    return (
        <figure {...props} {...dataLocProps(node)}>
            {qfCaptionId !== undefined && qfLocation === 'top' ? (
                <>
                    {figcaption}
                    {body}
                </>
            ) : (
                <>
                    {body}
                    {figcaption}
                </>
            )}
        </figure>
    );
};
