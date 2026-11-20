import { useContext } from 'react';
import type { CSSProperties } from 'react';
import type { ImageInline, NodeArgs } from '../../framework';
import { inlinesToPlainText } from '../../framework';
import { AssetManifestContext } from '../AssetManifestContext';
import { cssStringToObject, lookupAssetUrl } from '../utils';

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
 * `style`: the author's `style` key-value is applied, parsed into a React
 * style object. The pipeline's `image-dimensions` transform runs in the
 * preview too (bd-html-image-dims-t5ogxjxd), and it moves every
 * non-pixel `width` / `height` (`50%`, `3in`, `50vw`) into `style`, so
 * this is where those sizes arrive.
 *
 * `width` / `height` key-values are still handled for ASTs that did not
 * go through that transform (or a post filter that wrote them back): a
 * bare number stays an HTML attribute (pixels); a value with a unit goes
 * in `style`, since the HTML attributes accept only unitless pixel
 * counts. These declarations win over the same property in `style`, as
 * they would in Pandoc's output, where they are appended after it.
 *
 * `fig-alt` needs no handling here: the pipeline's `image-alt-normalize`
 * transform folds it (and a bare `alt` key-value) into the alt inlines
 * before the AST reaches the preview (bd-ykjwiqcc).
 *
 * v1 passes `width` / `height` / `style` / `id` / `classes` / `title` only;
 * other Quarto-specific Image extensions (`fig-align`, `fig-link`,
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
    const style: CSSProperties = kvMap.style ? { ...cssStringToObject(kvMap.style) } : {};
    for (const dim of ['width', 'height'] as const) {
        const v = kvMap[dim]?.trim();
        if (!v) continue;
        if (/^\d+(\.\d+)?$/.test(v)) props[dim] = v;
        else style[dim] = v;
    }

    return <img {...props} style={Object.keys(style).length ? style : undefined} />;
};
