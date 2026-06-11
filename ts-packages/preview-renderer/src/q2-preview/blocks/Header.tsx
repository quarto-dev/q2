import { useContext } from 'react';
import { renderChildren } from '../../framework';
import type { HeaderBlock, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';

const headerTags = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'] as const;

export const Header = (args: NodeArgs<HeaderBlock>) => {
    const [level, [id, classes, kvs]] = args.node.c;
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    const isEditable = resolved != null
        && resolved.reachabilityClass !== 'Opaque'
        && poolId !== undefined
        && !ctx?.editingDisabled;

    const domProps: Record<string, string | number> = {};
    if (id) domProps.id = id;
    if (classes.length) domProps.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') domProps[k] = v;
    }
    if (isEditable) {
        domProps['data-block-pool-id'] = String(poolId);
        domProps.tabIndex = -1;
    }
    const Tag = headerTags[Math.min(Math.max(level, 1), 6) - 1];

    return <Tag {...domProps}>{renderChildren(args)}</Tag>;
};
