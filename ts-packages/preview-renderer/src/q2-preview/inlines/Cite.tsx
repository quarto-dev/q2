import { Node } from '../../framework';
import type { BlockNode, CiteInline, InlineNode, NodeArgs } from '../../framework';

/**
 * Cite renders `c[1]` (the visible inlines: the resolved citation text,
 * or the `@key` source when citeproc did not run) via `<Node>`, inside
 * `<span class="citation" data-cites="...">` as the HTML writer and
 * Pandoc render it (GH #1026). `data-cites` lists the `citationId`s from
 * `c[0]`, space-separated.
 */
export const Cite = (args: NodeArgs<CiteInline>) => {
    const { node, setLocalAst, onNavigateToDocument } = args;
    const [citations, inlines] = node.c;
    const dataCites = citations
        .map((c) => (c as { citationId?: unknown }).citationId)
        .filter((id): id is string => typeof id === 'string')
        .join(' ');
    return (
        <span className="citation" data-cites={dataCites}>
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
        </span>
    );
};
