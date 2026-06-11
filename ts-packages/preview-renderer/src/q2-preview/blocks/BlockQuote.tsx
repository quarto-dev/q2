import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { BlockQuoteBlock, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';

export const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    return (
        <blockquote {...(isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {})}>
            {renderChildren(args)}
        </blockquote>
    );
};
