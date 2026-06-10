import { useContext } from 'react';
import { Node } from '../../framework';
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
    const [[id, classes], [shortCaption, captionBlocks], bodyBlocks] = node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    if (isEditable) props['data-block-pool-id'] = String(poolId);

    return (
        <figure {...props}>
            {bodyBlocks.map((b, i) => (
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
            ))}
            {captionBlocks.length > 0 && (
                <figcaption>
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
            )}
        </figure>
    );
};
