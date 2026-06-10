import { useContext } from 'react';
import { Node } from '../../framework';
import type { BlockNode, InlineNode, LineBlockBlock, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';

/**
 * LineBlock → `<div class="line-block">` with each line as a `<div>`
 * containing its inlines. Pandoc HTML writer produces this exact shape;
 * theme CSS targets `.line-block`.
 */
export const LineBlock = (args: NodeArgs<LineBlockBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;

    const { node, setLocalAst, onNavigateToDocument } = args;
    return (
        <div className="line-block" {...(isEditable ? { 'data-block-pool-id': poolId } : {})}>
            {node.c.map((line, i) => (
                <div key={i}>
                    {line.map((inl, j) => (
                        <Node
                            key={j}
                            node={inl}
                            onNavigateToDocument={onNavigateToDocument}
                            setLocalAst={(newInl: BlockNode | InlineNode) => {
                                const newLine = line.slice();
                                newLine[j] = newInl as InlineNode;
                                const newLines = node.c.slice();
                                newLines[i] = newLine;
                                setLocalAst({ t: 'LineBlock', c: newLines });
                            }}
                        />
                    ))}
                </div>
            ))}
        </div>
    );
};
