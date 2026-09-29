import { useContext, useMemo } from 'react';
import { dataLocProps } from '../../framework';
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
 *
 * Reveal stretch: React can only inject raw HTML through a wrapper element
 * (`dangerouslySetInnerHTML`), so a stretched reveal video ends up at
 * `section > div > iframe.r-stretch`. reveal's stretch selector
 * (`section > .r-stretch`, direct children only) would miss it. When the raw
 * HTML's root element carries `r-stretch`, we mirror that class onto the
 * wrapper div so reveal stretches the wrapper; quarto-reveal.css then sizes the
 * inner iframe to fill it. Inert outside reveal (the CSS rule is reveal-scoped).
 * bd-xfw2omlt.
 */

/** Does the first/root element of an HTML fragment carry the given class? */
function rootHasClass(html: string, cls: string): boolean {
    const m = html.match(/^\s*<[a-zA-Z][^>]*?\sclass\s*=\s*"([^"]*)"/);
    return m != null && m[1].split(/\s+/).includes(cls);
}

export const RawBlock = ({ node }: NodeArgs<RawBlockType>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    const [format, content] = node.c;
    const locProps = dataLocProps(node);
    // React 19 re-assigns `innerHTML` whenever the `dangerouslySetInnerHTML`
    // object identity changes (it no longer compares `__html` strings), so an
    // inline `{ __html: content }` would recreate the injected DOM — and
    // reload any embedded <iframe> — on every preview re-render. Memoize the
    // object per content string so unchanged raw HTML keeps its DOM.
    const innerHtml = useMemo(() => ({ __html: content }), [content]);
    if (format === 'html' || format === 'html5') {
        const className = rootHasClass(content, 'r-stretch') ? 'r-stretch' : undefined;
        return <div className={className} {...affordanceAttr} {...locProps} dangerouslySetInnerHTML={innerHtml} />;
    }
    return <pre {...affordanceAttr} {...locProps}>{content}</pre>;
};
