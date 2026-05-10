/**
 * Parent-side asset walker for q2-preview.
 *
 * Walks the AST for `Image` nodes, resolves each `target.0` against
 * `currentFilePath`, reads VFS bytes, mints (or reuses cached) blob URLs,
 * and produces a manifest of `{ origPath → blobUrl }` that the iframe's
 * `<Image>` component consumes via `AssetManifestContext`.
 *
 * Cache identity is the base64 content string itself — `vfsReadBinaryFile`
 * returns base64-encoded bytes, identical bytes always produce identical
 * base64, so the string is a deterministic 1-to-1 fingerprint with no
 * hashing required (and stays synchronous, which lets the caller use a
 * `useMemo` keyed on `astJson + currentFilePath`).
 *
 * Cache eviction: any entry whose key is not seen in this run is revoked
 * via `URL.revokeObjectURL` and removed from the cache. The caller passes
 * in a long-lived `Map` (typically a `useRef` on `Q2PreviewIframe`), so
 * stable image content keeps the same blob URL across renders.
 */

import { vfsReadBinaryFile } from '../../../services/wasmRenderer';
import { resolveRelativePath, guessMimeType } from '../../../utils/vfsPaths';

export interface ManifestCacheEntry {
    url: string;
    /** The base64 content string — held in cache as the identity key
     * is `${path}\0${contentB64}`, so reads of `contentB64` are not
     * needed by callers; field is here for debugability. */
    contentB64: string;
}

export interface AssetManifestResult {
    /** origPath (the user-facing string from the Image node's `target.0`)
     * → blob URL. External URLs (`http(s):`, `data:`, `//`) are not in
     * the manifest; the iframe's lookup helper passes those through. */
    manifest: Record<string, string>;
    /** Blob URLs that fell out of the manifest this run; the caller
     * may use this for telemetry, but revocation has already happened. */
    revoked: string[];
}

/**
 * Build the asset manifest for one render cycle.
 *
 * @param astJson — JSON string of the post-pipeline AST.
 * @param currentFilePath — the doc's file path, used to resolve
 *   relative image paths. Convention: `/project/<…>`.
 * @param cache — caller-owned Map. Mutated in place; entries that fall
 *   out of the manifest this run are revoked and deleted.
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
        return { manifest: {}, revoked: [] };
    }

    const imagePaths = collectImagePaths(ast);
    const manifest: Record<string, string> = {};
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
            const blob = base64ToBlob(result.content, guessMimeType(resolved));
            entry = { url: URL.createObjectURL(blob), contentB64: result.content };
            cache.set(cacheKey, entry);
        }
        manifest[origPath] = entry.url;
    }

    // Revoke and evict cache entries no longer referenced.
    const revoked: string[] = [];
    for (const [key, entry] of cache) {
        if (!seenKeys.has(key)) {
            URL.revokeObjectURL(entry.url);
            revoked.push(entry.url);
            cache.delete(key);
        }
    }

    return { manifest, revoked };
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
