/**
 * Shared asset-proxy policy — the single source of truth for how document
 * assets travel between the sandboxed iframe and the parent's WASM VFS.
 *
 * Consumed by three parties (this replaces the skewed extension lists the
 * old TODO pair in serviceWorker.ts / Q2SandboxedPreviewIframe.tsx warned
 * about):
 *  - the service worker (`serviceWorker.ts`) — which fetches to intercept,
 *    what MIME type to synthesize;
 *  - the iframe page bridge (`registerServiceWorker.ts`);
 *  - the parent responder (`Q2SandboxedPreviewIframe.tsx` in hub-client,
 *    imported by relative path) — binary vs text VFS read.
 *
 * ## The proxy namespace
 *
 * Proxied assets live under a dedicated **page-relative** URL segment:
 *
 *     __q2_vfs__/<resolved VFS path, no leading slash>
 *
 * The parent resolves each AST image target against `currentFilePath`
 * (same resolution as q2-preview's asset walker) and ships the mapping in
 * the `UPDATE_AST` asset manifest, so the browser requests e.g.
 * `https://quarto-dev.github.io/q2/__q2_vfs__/project/sub/pic.png`.
 *
 * Page-relative matters twice: the URL stays inside the service worker's
 * scope under a project-path deployment (`/q2/…`), and the full resolved
 * path rides in the URL — two same-named images in different directories
 * stay distinct (the old bridge stripped URLs to their basename and
 * collided them).
 */

export const VFS_PROXY_SEGMENT = '__q2_vfs__';

/** Page-relative proxy URL for a resolved VFS path. */
export function proxyUrlForVfsPath(vfsPath: string): string {
    const clean = vfsPath.replace(/^\/+/, '');
    return `${VFS_PROXY_SEGMENT}/${encodeURI(clean)}`;
}

/**
 * Inverse of {@link proxyUrlForVfsPath}: extract the VFS path from a
 * request URL, or `null` when the URL is outside the proxy namespace
 * (the page itself, `assets/*` bundle files, `serviceWorker.js`, …).
 */
export function vfsPathForRequestUrl(url: string): string | null {
    let pathname: string;
    try {
        pathname = new URL(url).pathname;
    } catch {
        return null;
    }
    const marker = `/${VFS_PROXY_SEGMENT}/`;
    const idx = pathname.indexOf(marker);
    if (idx === -1) return null;
    const rest = pathname.slice(idx + marker.length);
    if (!rest) return null;
    return decodeURI(rest);
}

/**
 * Whether the parent should read this path with `vfs_read_binary_file`
 * (content travels base64) rather than `vfs_read_file` (plain text).
 */
export function isBinaryPath(path: string): boolean {
    return /\.(png|jpg|jpeg|gif|webp|ico|pdf|ttf|otf|woff|woff2|eot|zip|wasm)$/i.test(path);
}

const MIME_TYPES: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    svg: 'image/svg+xml',
    webp: 'image/webp',
    ico: 'image/x-icon',
    pdf: 'application/pdf',
    html: 'text/html',
    css: 'text/css',
    js: 'application/javascript',
    json: 'application/json',
    txt: 'text/plain',
    wasm: 'application/wasm',
    ttf: 'font/ttf',
    otf: 'font/otf',
    woff: 'font/woff',
    woff2: 'font/woff2',
};

export function mimeTypeFor(filename: string): string {
    const ext = filename.split('.').pop()?.toLowerCase();
    return MIME_TYPES[ext || ''] || 'application/octet-stream';
}

/**
 * Rewrite relative `url(...)` references in theme CSS into the proxy
 * namespace, resolved against the directory the CSS artifact lives in
 * (`.quarto/project-artifacts`). The theme is applied through a blob URL
 * whose base is opaque, so relative refs would otherwise resolve nowhere
 * — q2-preview simply loses them; here the service worker can serve them
 * from the VFS.
 *
 * Absolute (`http(s):`, `//`), `data:`, `blob:`, and fragment (`#…`) refs
 * pass through untouched.
 */
export function rewriteThemeCssUrls(cssText: string, cssDirVfsPath: string): string {
    const baseSegments = cssDirVfsPath.replace(/^\/+|\/+$/g, '').split('/');
    return cssText.replace(
        /url\(\s*(['"]?)([^'")]+)\1\s*\)/g,
        (whole, quote: string, ref: string) => {
            const trimmed = ref.trim();
            if (/^(https?:|data:|blob:|\/\/|#|\/)/i.test(trimmed)) return whole;
            // Resolve ./ and ../ segments against the CSS directory.
            const segments = [...baseSegments];
            for (const part of trimmed.split('/')) {
                if (part === '' || part === '.') continue;
                if (part === '..') {
                    segments.pop();
                } else {
                    segments.push(part);
                }
            }
            return `url(${quote}${proxyUrlForVfsPath(segments.join('/'))}${quote})`;
        },
    );
}
