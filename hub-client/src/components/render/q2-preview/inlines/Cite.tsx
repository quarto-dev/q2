import { Node } from '@quarto/preview-renderer/framework';
import type { BlockNode, CiteInline, InlineNode, NodeArgs } from '@quarto/preview-renderer/framework';

/**
 * Cite renders `c[1]` (the visible inlines Pandoc fills in for the
 * link text) via `<Node>`. `c[0]` (the citations metadata array) is
 * ignored in v1 — bibliography rendering is deferred. No wrapper
 * element: the visible inlines flow into the surrounding context.
 */
export const Cite = (args: NodeArgs<CiteInline>) => {
    const { node, setLocalAst, onNavigateToDocument } = args;
    const [citations, inlines] = node.c;
    return (
        <>
            {inlines.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newInl: BlockNode | InlineNode) => {
                        const next = inlines.slice();
                        next[i] = newInl as InlineNode;
                        setLocalAst({ t: 'Cite', c: [citations, next] });
                    }}
                />
            ))}
        </>
    );
};
