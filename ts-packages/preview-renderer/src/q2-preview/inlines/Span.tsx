import { renderChildren } from '../../framework';
import type { NodeArgs, SpanInline } from '../../framework';
import { cssStringToObject } from '../utils';

export const Span = (args: NodeArgs<SpanInline>) => {
    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, unknown> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-')) props[k] = v;
    }
    // `[text]{style="color: red"}` — the native writer emits the style
    // attribute verbatim; React needs a style object (same as Div).
    const styleStr = kvs.find(([k]) => k === 'style')?.[1];
    if (styleStr) props.style = cssStringToObject(styleStr);
    return <span {...props}>{renderChildren(args)}</span>;
};
