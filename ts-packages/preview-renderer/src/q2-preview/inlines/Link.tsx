import { renderChildren } from '../../framework';
import type { LinkInline, NodeArgs } from '../../framework';

export const Link = (args: NodeArgs<LinkInline>) => {
    const [[id, classes, kvs], , [url, title]] = args.node.c;
    const props: Record<string, string> = { href: url };
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    if (title) props.title = title;
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'rel' || k === 'target') props[k] = v;
    }
    return <a {...props}>{renderChildren(args)}</a>;
};
