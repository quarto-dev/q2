import { useContext } from 'react';

import type { NodeArgs, RawBlock as RawBlockType } from '../../framework';
import { AssetManifestContext } from '../AssetManifestContext';
import { rewriteEmbedIframeSrcs } from '../embedIframe';

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
 * Embedded-example decks (bd-kjrpya2d): the Rust `.embed-example-iframe`
 * transform emits the deck as a raw `<iframe src="/examples/…">`. There
 * is no server to answer that request in preview, so we swap the `src`
 * to the deck's `text/html` blob URL minted by the asset walker
 * (`assetWalker.ts`) and carried in via `AssetManifestContext` — the
 * same parent-side VFS-to-blob path images use. Non-embed HTML and
 * decks absent from the manifest pass through untouched.
 */
export const RawBlock = ({ node }: NodeArgs<RawBlockType>) => {
    const [format, content] = node.c;
    const manifest = useContext(AssetManifestContext);
    if (format === 'html' || format === 'html5') {
        const html = rewriteEmbedIframeSrcs(content, manifest);
        return <div dangerouslySetInnerHTML={{ __html: html }} />;
    }
    return <pre>{content}</pre>;
};
