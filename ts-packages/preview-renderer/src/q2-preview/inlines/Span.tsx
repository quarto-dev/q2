import { renderChildren } from '../../framework';
import type { NodeArgs, SpanInline } from '../../framework';
import { cssStringToObject } from '../utils';
import { useCommentAnchor } from '../commentAnchor';

export const Span = (args: NodeArgs<SpanInline>) => {
    // Span comments (custom/CommentSpan.tsx) anchor their bubble here and
    // ask for the hover/open highlight through the same context.
    const { ref: anchorRef, highlighted } = useCommentAnchor(args.node);
    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, unknown> = {};
    if (id) props.id = id;
    const classList = highlighted ? [...classes, 'q2-comment-span-hover'] : classes;
    if (classList.length) props.className = classList.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-')) props[k] = v;
    }
    // `[text]{style="color: red"}` — the native writer emits the style
    // attribute verbatim; React needs a style object (same as Div).
    const styleStr = kvs.find(([k]) => k === 'style')?.[1];
    if (styleStr) props.style = cssStringToObject(styleStr);
    return <span ref={anchorRef} {...props}>{renderChildren(args)}</span>;
};
