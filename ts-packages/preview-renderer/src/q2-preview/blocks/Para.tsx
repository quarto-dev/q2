import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';
import { PreviewContext } from '..';
import { sliceBytes } from '../../utils/sliceSource';

export const Para = (args: NodeArgs<ParaBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;

    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    const isEditable = resolved?.reachabilityClass === 'TopLevel'
        && ctx?.commitEdit !== undefined
        && ctx.content != null;

    const isEditTarget = isEditable && ctx!.editTarget === poolId;

    if (!isEditable) {
        return <p>{renderChildren(args)}</p>;
    }

    if (isEditTarget) {
        const initialText = sliceBytes(ctx!.content!, resolved!.sourceEntry.r[0], resolved!.sourceEntry.r[1]).trimEnd();

        const commit = (el: HTMLTextAreaElement) => {
            const text = el.value;
            if (!text.trim()) {
                ctx!.setEditTarget!(null);
                return;
            }
            ctx!.commitEdit!(poolId!, text);
            ctx!.setEditTarget!(null);
        };

        return (
            <textarea
                autoFocus
                defaultValue={initialText}
                style={{ fontFamily: 'monospace', width: '100%', boxSizing: 'border-box', minHeight: '4em', resize: 'vertical' }}
                onBlur={(e) => commit(e.currentTarget)}
                onKeyDown={(e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                        e.preventDefault();
                        commit(e.currentTarget);
                    } else if (e.key === 'Escape') {
                        e.preventDefault();
                        ctx!.setEditTarget!(null);
                    }
                }}
            />
        );
    }

    return (
        <p
            onClick={() => ctx!.setEditTarget!(poolId!)}
            style={{ cursor: 'pointer' }}
            title="Click to edit"
        >
            {renderChildren(args)}
        </p>
    );
};
