/**
 * Parent-side asset walker for q2-preview.
 *
 * Walks the AST for `Image` nodes, resolves each `target.0` against
 * `currentFilePath`, reads VFS bytes, and produces a manifest of
 * `{ origPath → { key, blob } }`. The iframe mints its own blob URL for
 * each entry (`iframeAssets.ts`) and hands `<Image>` the result via
 * `AssetManifestContext`.
 *
 * The parent must not mint the URLs: a `blob:` URL only loads in the
 * origin that minted it, and when the host frames the preview with
 * `sandbox="allow-scripts"` (no `allow-same-origin`) the iframe has a
 * different opaque origin from the parent. A `Blob` crosses
 * `postMessage` by value, as the theme's CSS text does.
 *
 * Cache identity is the base64 content string itself — `vfsReadBinaryFile`
 * returns base64-encoded bytes, identical bytes always produce identical
 * base64, so the string is a deterministic 1-to-1 fingerprint with no
 * hashing required (and stays synchronous, which lets the caller use a
 * `useMemo` keyed on `astJson + currentFilePath`). Each cached entry gets a
 * short `key`, which the iframe uses to keep one URL per content.
 *
 * Cache eviction: any entry whose identity is not seen in this run is
 * removed. The caller passes in a long-lived `Map` (typically a `useRef`
 * on `Q2PreviewIframe`), so stable image content keeps the same key (and
 * so the same iframe URL) across renders.
 */

import { vfsReadBinaryFile } from '@quarto/preview-runtime';
import { resolveRelativePath, guessMimeType } from '../utils/vfsPaths';

/**
 * One image as the parent posts it: its bytes, and a key that is the same
 * for the same path and content. A plain string is a URL that is used
 * as-is (a host that re-posts a payload the iframe already resolved).
 */
export type AssetManifestEntry = string | { readonly key: string; readonly blob: Blob };

export interface ManifestCacheEntry {
    key: string;
    blob: Blob;
    /** The base64 content string — held in cache as the identity key
     * is `${path}\0${contentB64}`, so reads of `contentB64` are not
     * needed by callers; field is here for debugability. */
    contentB64: string;
}

export interface AssetManifestResult {
    /** origPath (the user-facing string from the Image node's `target.0`)
     * → the image's key and bytes. External URLs (`http(s):`, `data:`,
     * `//`) are not in the manifest; the iframe's lookup helper passes
     * those through. */
    manifest: Record<string, AssetManifestEntry>;
}

let nextAssetKey = 0;

/**
 * Build the asset manifest for one render cycle.
 *
 * @param astJson — JSON string of the post-pipeline AST.
 * @param currentFilePath — the doc's file path, used to resolve
 *   relative image paths. Convention: `/project/<…>`.
 * @param cache — caller-owned Map. Mutated in place; entries that fall
 *   out of the manifest this run are deleted.
 */
export function buildAssetManifest(
    astJson: string,
    currentFilePath: string,
    cache: Map<string, ManifestCacheEntry>,
): AssetManifestResult {
    let ast: unknown;
    try {
        ast = JSON.parse(astJson);
    } catch {
        return { manifest: {} };
    }

    const imagePaths = collectImagePaths(ast);
    const manifest: Record<string, AssetManifestEntry> = {};
    const seenKeys = new Set<string>();

    for (const origPath of imagePaths) {
        // VFS keys don't carry the leading slash; `resolveRelativePath`
        // returns paths starting with `/`. Strip locally rather than
        // baking it into vfsPaths.ts (other consumers want the slash).
        const resolved = stripLeadingSlash(
            resolveRelativePath(currentFilePath, origPath),
        );
        const result = vfsReadBinaryFile(resolved);
        if (!result.success || !result.content) continue;

        const cacheKey = `${resolved}\0${result.content}`;
        seenKeys.add(cacheKey);

        let entry = cache.get(cacheKey);
        if (!entry) {
            entry = {
                key: `asset-${nextAssetKey++}`,
                blob: base64ToBlob(result.content, guessMimeType(resolved)),
                contentB64: result.content,
            };
            cache.set(cacheKey, entry);
        }
        manifest[origPath] = { key: entry.key, blob: entry.blob };
    }

    for (const key of cache.keys()) {
        if (!seenKeys.has(key)) cache.delete(key);
    }

    return { manifest };
}

/**
 * Collect Image-target URL strings from the AST. External URLs
 * (`http(s):`, `data:`, `//`) are filtered out — they don't need
 * VFS bytes, and the iframe's `lookupAssetUrl` passes them through.
 *
 * Uses the same structural-JSON walk as `customNode.ts` (descend
 * `c` fields, recurse element-wise into arrays).
 */
function collectImagePaths(ast: unknown): string[] {
    const paths = new Set<string>();
    visit(ast, paths);
    return Array.from(paths);
}

function visit(value: unknown, out: Set<string>): void {
    if (!value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
        for (const item of value) visit(item, out);
        return;
    }
    const obj = value as { t?: unknown; c?: unknown; blocks?: unknown };
    if (obj.t === 'Image' && Array.isArray(obj.c) && obj.c.length >= 3) {
        const target = obj.c[2];
        if (Array.isArray(target) && typeof target[0] === 'string') {
            const url = target[0];
            if (!isExternal(url)) out.add(url);
        }
    }
    if ('c' in obj) visit(obj.c, out);
    if ('blocks' in obj) visit(obj.blocks, out);
}

function isExternal(url: string): boolean {
    return (
        url.startsWith('http://') ||
        url.startsWith('https://') ||
        url.startsWith('data:') ||
        url.startsWith('//')
    );
}

function stripLeadingSlash(path: string): string {
    return path.startsWith('/') ? path.slice(1) : path;
}

/**
 * Decode a base64 string to a Blob with the given MIME type.
 * Atob handles the base64 → binary string step; we then materialize
 * a `Uint8Array` for the Blob constructor.
 */
function base64ToBlob(b64: string, mime: string): Blob {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return new Blob([bytes], { type: mime });
}
