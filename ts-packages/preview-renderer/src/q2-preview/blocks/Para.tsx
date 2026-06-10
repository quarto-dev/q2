import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';
import { PreviewContext } from '../PreviewContext';

export const Para = (args: NodeArgs<ParaBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    const isEditable = resolved != null
        && resolved.reachabilityClass !== 'Opaque'
        && poolId !== undefined;

    return (
        <p {...(isEditable ? { 'data-block-pool-id': poolId } : {})}>
            {renderChildren(args)}
        </p>
    );
};
