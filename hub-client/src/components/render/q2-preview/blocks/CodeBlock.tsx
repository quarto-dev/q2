import type { CodeBlock as CodeBlockType, NodeArgs } from '@quarto/preview-renderer/framework';

export const CodeBlock = ({ node }: NodeArgs<CodeBlockType>) => {
    const [[id, classes, kvs], code] = node.c;
    const codeProps: Record<string, string> = {};
    const preProps: Record<string, string> = {};
    if (id) preProps.id = id;
    if (classes.length) {
        // Pandoc writer puts language classes on <code>; Bootstrap
        // and pampa's HTML writer follow the same convention.
        codeProps.className = classes.join(' ');
    }
    for (const [k, v] of kvs) {
        if (k.startsWith('data-')) preProps[k] = v;
    }
    return (
        <pre {...preProps}>
            <code {...codeProps}>{code}</code>
        </pre>
    );
};
