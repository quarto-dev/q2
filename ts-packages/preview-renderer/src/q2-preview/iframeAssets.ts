/**
 * Iframe-side blob URLs for the parent's asset manifest.
 *
 * The parent posts each image's bytes as a `Blob` with a content key
 * (`assetWalker.ts`). The iframe mints the `blob:` URL itself, because a
 * `blob:` URL only loads in the origin that minted it: when the host
 * frames the preview with `sandbox="allow-scripts"` (no
 * `allow-same-origin`), the parent and this iframe have different opaque
 * origins.
 *
 * One URL per key, so an image whose content didn't change keeps its URL
 * (and isn't reloaded) across `UPDATE_AST`s. A URL is revoked once a
 * manifest no longer uses it.
 */

import type { AssetManifestEntry } from './assetWalker';

/** Minted URLs by asset key. */
const minted = new Map<string, string>();

/**
 * The manifest `<Image>` reads: `{ origPath → URL }`. A string entry is
 * already a URL and is used as-is, so a host that re-posts a payload this
 * iframe resolved (its URLs) keeps them alive.
 */
export function resolveAssetManifest(
    manifest: Record<string, AssetManifestEntry>,
): Record<string, string> {
    const resolved: Record<string, string> = {};
    const usedKeys = new Set<string>();
    const usedUrls = new Set<string>();
    for (const [path, entry] of Object.entries(manifest)) {
        if (typeof entry === 'string') {
            resolved[path] = entry;
            usedUrls.add(entry);
            continue;
        }
        let url = minted.get(entry.key);
        if (!url) {
            url = URL.createObjectURL(entry.blob);
            minted.set(entry.key, url);
        }
        usedKeys.add(entry.key);
        resolved[path] = url;
    }
    for (const [key, url] of minted) {
        if (!usedKeys.has(key) && !usedUrls.has(url)) {
            URL.revokeObjectURL(url);
            minted.delete(key);
        }
    }
    return resolved;
}

/** Test hook: forget every minted URL. */
export function resetMintedAssetsForTest(): void {
    minted.clear();
}
