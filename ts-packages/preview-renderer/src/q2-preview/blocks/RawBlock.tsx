import { useContext } from 'react';
import type { NodeArgs, RawBlock as RawBlockType } from '../../framework';
import { PreviewContext } from '../PreviewContext';

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
    const ctx = useContext(PreviewContext);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable = resolved?.reachabilityClass === 'TopLevel' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId } : {};

    const [format, content] = node.c;
    if (format === 'html' || format === 'html5') {
        return <div {...affordanceAttr} dangerouslySetInnerHTML={{ __html: content }} />;
    }
    return <pre {...affordanceAttr}>{content}</pre>;
};
