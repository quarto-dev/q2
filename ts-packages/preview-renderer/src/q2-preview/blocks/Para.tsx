import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { useEditableBlock } from '../useEditableBlock';

export const Para = (args: NodeArgs<ParaBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    const isEditable = resolved?.reachabilityClass === 'TopLevel'
        && ctx?.commitTextEdit !== undefined
        && ctx.content != null;

    const editor = useEditableBlock({ poolId, resolved });

    return (
        <p {...(isEditable && poolId !== undefined ? { 'data-block-pool-id': poolId } : {})}>
            {editor ?? renderChildren(args)}
        </p>
    );
};
