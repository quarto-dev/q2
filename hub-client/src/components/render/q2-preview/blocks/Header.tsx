import { renderChildren } from '@quarto/preview-renderer/framework';
import type { HeaderBlock, NodeArgs } from '@quarto/preview-renderer/framework';

const headerTags = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'] as const;

export const Header = (args: NodeArgs<HeaderBlock>) => {
    const [level, [id, classes, kvs]] = args.node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    const Tag = headerTags[Math.min(Math.max(level, 1), 6) - 1];
    return <Tag {...props}>{renderChildren(args)}</Tag>;
};
