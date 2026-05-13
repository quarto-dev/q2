import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, SpanInline } from '@quarto/preview-renderer/framework';

export const Span = (args: NodeArgs<SpanInline>) => {
    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-')) props[k] = v;
    }
    return <span {...props}>{renderChildren(args)}</span>;
};
