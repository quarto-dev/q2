import type { NodeArgs, RawInlineInline } from '@quarto/preview-renderer/framework';

/**
 * RawInline semantics mirror RawBlock:
 *  - format === 'html' (or 'html5'): inject raw HTML.
 *  - any other format: render as `<code>` so the source is visible.
 *
 * Note: dangerouslySetInnerHTML on a span is fine — React allows it.
 */
export const RawInline = ({ node }: NodeArgs<RawInlineInline>) => {
    const [format, content] = node.c;
    if (format === 'html' || format === 'html5') {
        return <span dangerouslySetInnerHTML={{ __html: content }} />;
    }
    return <code>{content}</code>;
};
