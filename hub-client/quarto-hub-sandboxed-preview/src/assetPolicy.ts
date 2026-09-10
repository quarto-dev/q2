/**
 * Shared asset-proxy policy — the single source of truth for how document
 * assets travel between the sandboxed iframe and the parent's WASM VFS.
 *
 * Consumed by three parties:
 *  - the service worker (`serviceWorker.ts`) — which fetches to intercept,
 *    what MIME type to synthesize;
 *  - the iframe page bridge (`registerServiceWorker.ts`);
 *  - the parent responder (`Q2SandboxedPreviewIframe.tsx` in hub-client,
 *    imported by relative path) — binary vs text VFS read.
 *
 * ## Routing (bd-00bgt5cy)
 *
 * **Any relative path is served from the VFS.** The service worker
 * intercepts every same-origin GET inside its scope EXCEPT the frame's
 * own app files:
 *
 *   - the page itself (`/`, `index.html`)
 *   - `serviceWorker.js`
 *   - `q2-preview-assets/*` — the hashed renderer bundle chunks and
 *     KaTeX fonts (deliberately NOT `assets/`, so a project's own
 *     `assets/` directory is proxied like any other project path;
 *     `build.assetsDir` in vite.config.ts must stay in sync)
 *
 * Everything else is treated as a document asset: the URL path relative
 * to the SW scope IS the VFS path. The parent resolves AST image targets
 * against `currentFilePath` at manifest-build time (same resolution as
 * q2-preview's asset walker) and ships bare resolved paths, so e.g. a
 * subdirectory image renders as `<img src="sub/images/pic.png">` — full
 * paths in the URL keep same-named files in different directories
 * distinct. Paths that never went through the manifest (an `<img>` in
 * raw HTML or navbar chrome) are still intercepted; the parent responder
 * retries them against the current document's directory on a VFS miss.
 *
 * Known limitation, deliberate: project files literally named
 * `index.html`, `serviceWorker.js`, or under `q2-preview-assets/` at the
 * VFS root are shadowed by the app-file exemption (they fall through to
 * the network). The app asset dir is named `q2-preview-assets` precisely
 * so no real project trips over this.
 */

/** App files the service worker must NOT proxy (relative to its scope). */
const APP_FILES = new Set(['', 'index.html', 'serviceWorker.js']);
const APP_ASSET_DIR = 'q2-preview-assets/';

/** Page-relative URL for a resolved VFS path (bare path, URI-encoded). */
export function pageRelativeUrlForVfsPath(vfsPath: string): string {
    return encodeURI(vfsPath.replace(/^\/+/, ''));
}

/**
 * Inverse of {@link pageRelativeUrlForVfsPath}: extract the VFS path from
 * a request URL, or `null` when the URL should not be proxied — outside
 * `scopeUrl`, or one of the frame's own app files.
 */
export function vfsPathForRequestUrl(url: string, scopeUrl: string): string | null {
    let pathname: string;
    let scopePath: string;
    try {
        const u = new URL(url);
        const s = new URL(scopeUrl);
        if (u.origin !== s.origin) return null;
        pathname = u.pathname;
        scopePath = s.pathname;
    } catch {
        return null;
    }
    if (!scopePath.endsWith('/')) scopePath += '/';
    if (!pathname.startsWith(scopePath)) return null;
    const rest = decodeURI(pathname.slice(scopePath.length));
    if (APP_FILES.has(rest) || rest.startsWith(APP_ASSET_DIR)) return null;
    return rest;
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
 * Rewrite relative `url(...)` references in theme CSS into **absolute**
 * page URLs, resolved against the directory the CSS artifact lives in
 * (`.quarto/project-artifacts`) and anchored at `pageBaseUrl` (the
 * iframe document's base). Absolute because the theme is applied through
 * a blob-URL stylesheet whose opaque base cannot anchor relative refs.
 * The resulting URLs land inside the SW scope, so fonts and background
 * images round-trip from the VFS like any other document asset.
 *
 * Absolute (`http(s):`, `//`), `data:`, `blob:`, root-relative (`/…`),
 * and fragment (`#…`) refs pass through untouched.
 */
export function rewriteThemeCssUrls(
    cssText: string,
    cssDirVfsPath: string,
    pageBaseUrl: string,
): string {
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
            const abs = new URL(
                pageRelativeUrlForVfsPath(segments.join('/')),
                pageBaseUrl,
            ).href;
            return `url(${quote}${abs}${quote})`;
        },
    );
}
