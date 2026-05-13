import type { NodeArgs, RawBlock as RawBlockType } from '@quarto/preview-renderer/framework';

/**
 * RawBlock semantics:
 *  - format === 'html' (or 'html5'): inject raw HTML via
 *    `dangerouslySetInnerHTML` so users can embed exact markup.
 *  - any other format: render as a `<pre>` block so the source is
 *    visible (a Pandoc Markdown writer's text isn't meaningful HTML).
 *
 * Sanitization is the user's responsibility — RawBlock means "trust
 * the author." The iframe sandbox limits the blast radius.
 */
export const RawBlock = ({ node }: NodeArgs<RawBlockType>) => {
    const [format, content] = node.c;
    if (format === 'html' || format === 'html5') {
        return <div dangerouslySetInnerHTML={{ __html: content }} />;
    }
    return <pre>{content}</pre>;
};
