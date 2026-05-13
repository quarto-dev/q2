import type { CodeInline, NodeArgs } from '@quarto/preview-renderer/framework';

export const Code = ({ node }: NodeArgs<CodeInline>) => {
    const [[id, classes, kvs], content] = node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-')) props[k] = v;
    }
    return <code {...props}>{content}</code>;
};
