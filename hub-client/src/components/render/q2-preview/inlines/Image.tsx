import { useContext } from 'react';
import type { ImageInline, NodeArgs } from '../../framework';
import { AssetManifestContext } from '../AssetManifestContext';
import { lookupAssetUrl, inlinesToPlainText } from '../utils';

/**
 * Image → `<img>`. Reads `target.0` (the user-written URL) and looks
 * it up in the asset manifest distributed by `AssetManifestContext`.
 *
 * External URLs (`https?:`, `data:`, `//`) pass through unchanged.
 * Project-relative paths get a blob URL from the manifest. Manifest
 * miss falls back to the original URL — the resulting broken `<img>`
 * is a deliberate signal that resolution failed.
 *
 * Alt text uses `inlinesToPlainText` to handle `Emph` / `Code` /
 * `SoftBreak` etc. inside alt content, not just `Str` filtering.
 *
 * v1 passes `width` / `height` / `id` / `classes` / `title` only;
 * Quarto-specific Image extensions (`fig-align`, `fig-link`, `fig-alt`,
 * `lightbox`, subfigures, `fig-cap-location`) are silently ignored —
 * deferred to a follow-up plan parallel to "q2-preview layout chrome."
 */
export const Image = ({ node }: NodeArgs<ImageInline>) => {
    const [[id, classes, kvs], altInlines, [url, title]] = node.c;
    const manifest = useContext(AssetManifestContext);

    const src = lookupAssetUrl(manifest, url);
    const alt = inlinesToPlainText(altInlines);
    const kvMap: Record<string, string> = {};
    for (const [k, v] of kvs) kvMap[k] = v;

    const props: Record<string, string> = { src, alt };
    if (title) props.title = title;
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    if (kvMap.width) props.width = kvMap.width;
    if (kvMap.height) props.height = kvMap.height;

    return <img {...props} />;
};
