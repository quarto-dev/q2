import { createContext } from 'react';

/**
 * Iframe-side context that carries the asset manifest from
 * `Q2PreviewIframe`'s parent-side walker (`assetWalker.ts`'s
 * `buildAssetManifest`) to `<Image>` (and any other consumer that
 * resolves project-relative URLs).
 *
 * Manifest shape: `{ origPath → blobUrl }` where `origPath` is the
 * user-written `Image.target.0` (so `<Image>` can look up by the same
 * string the AST contains) and `blobUrl` is a `URL.createObjectURL`
 * minted in the parent.
 *
 * External URLs (`https?:`, `data:`, `//`) are not in the manifest
 * — `q2-preview/utils.ts::lookupAssetUrl` recognizes those patterns
 * and passes them through unchanged.
 *
 * Default value is the empty manifest, so a misconfigured render
 * (consumer outside the Provider) yields broken-image affordances
 * rather than a runtime crash.
 */
export const AssetManifestContext = createContext<Record<string, string>>({});
