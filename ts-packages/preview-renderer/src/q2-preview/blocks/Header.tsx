import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { HeaderBlock, NodeArgs } from '../../framework';
import { PreviewContext } from '..';
import { sliceBytes } from '../../utils/sliceSource';

const headerTags = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'] as const;

export const Header = (args: NodeArgs<HeaderBlock>) => {
    const [level, [id, classes, kvs]] = args.node.c;
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;

    // Resolve pool entry for this block
    const pool = ctx?.pool as Array<{ t: number; r: [number, number]; d: number }> | undefined;
    const entry = poolId !== undefined ? pool?.[Number(poolId)] : undefined;

    // Editable if: context present, commitEdit available, content present, Original block in active file
    const isEditable = ctx?.commitEdit !== undefined
        && poolId !== undefined
        && ctx.content != null
        && entry?.t === 0
        && entry?.d === 0;

    const isEditTarget = isEditable && ctx!.editTarget === poolId;

    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    const Tag = headerTags[Math.min(Math.max(level, 1), 6) - 1];

    if (!isEditable) {
        return <Tag {...props}>{renderChildren(args)}</Tag>;
    }

    if (isEditTarget) {
        // sliceBytes already includes the `##` prefix — pass directly to commitEdit
        const initialText = sliceBytes(ctx!.content!, entry!.r[0], entry!.r[1]).trimEnd();

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
                style={{ fontFamily: 'monospace', width: '100%', boxSizing: 'border-box', minHeight: '2em', resize: 'vertical' }}
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
        <Tag
            {...props}
            onClick={() => ctx!.setEditTarget!(poolId!)}
            style={{ cursor: 'pointer' }}
            title="Click to edit"
        >
            {renderChildren(args)}
        </Tag>
    );
};
