import { useMemo } from 'react';
import type { NodeArgs, RawInlineInline } from '../../framework';

/**
 * RawInline semantics mirror RawBlock:
 *  - format === 'html' (or 'html5'): inject raw HTML.
 *  - any other format: render as `<code>` so the source is visible.
 *
 * Note: dangerouslySetInnerHTML on a span is fine — React allows it.
 */
export const RawInline = ({ node }: NodeArgs<RawInlineInline>) => {
    const [format, content] = node.c;
    // Memoized per content string: React 19 re-sets innerHTML whenever the
    // object identity changes, which would tear down the injected DOM on
    // every re-render (see RawBlock).
    const innerHtml = useMemo(() => ({ __html: content }), [content]);
    if (format === 'html' || format === 'html5') {
        return <span dangerouslySetInnerHTML={innerHtml} />;
    }
    return <code>{content}</code>;
};
