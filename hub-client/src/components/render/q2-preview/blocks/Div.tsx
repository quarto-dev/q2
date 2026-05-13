import { renderChildren } from '@quarto/preview-renderer/framework';
import type { DivBlock, NodeArgs } from '@quarto/preview-renderer/framework';

export const Div = (args: NodeArgs<DivBlock>) => {
    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    return <div {...props}>{renderChildren(args)}</div>;
};
