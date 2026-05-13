/**
 * VFS path helpers — canonical implementations of three near-duplicate
 * private copies that previously lived in `iframePostProcessor.ts`,
 * `iframeLinkHandlers.ts`, and `ReactAstSlideRenderer.tsx`.
 *
 * Plan 2B's asset walker is the fourth consumer; consolidating here
 * avoids fanning the duplication out further.
 *
 * All three helpers operate on Pandoc-style POSIX paths (forward
 * slashes only). The VFS itself uses the `/project/` prefix; these
 * helpers do not strip it — callers that need to match VFS keys do
 * the strip themselves.
 */

/**
 * Resolve a relative path against the directory of `currentFile`.
 *
 * `currentFile` is interpreted as a path to a file (not a directory);
 * the directory is everything up to the last `/`. Absolute paths
 * (leading `/`) are returned unchanged.
 *
 *   resolveRelativePath('/project/sub/index.qmd', 'hero.png')
 *     → '/project/sub/hero.png'
 *   resolveRelativePath('/project/sub/index.qmd', '../shared/hero.png')
 *     → '/project/shared/hero.png'
 *   resolveRelativePath('anything', '/already/absolute.png')
 *     → '/already/absolute.png'
 */
export function resolveRelativePath(
    currentFile: string,
    relativePath: string,
): string {
    if (relativePath.startsWith('/')) {
        return relativePath; // Already absolute
    }
    const lastSlash = currentFile.lastIndexOf('/');
    const currentDir =
        lastSlash >= 0 ? currentFile.substring(0, lastSlash + 1) : '/';
    return normalizePath(currentDir + relativePath);
}

/**
 * Collapse `.` and `..` segments and remove empty segments, ensuring
 * the result has a single leading `/`. Trailing `..` past the root is
 * silently swallowed (matches the prior private-copy behavior).
 */
export function normalizePath(path: string): string {
    const parts = path.split('/').filter((p) => p !== '.');
    const result: string[] = [];
    for (const part of parts) {
        if (part === '..') {
            result.pop();
        } else if (part) {
            result.push(part);
        }
    }
    return '/' + result.join('/');
}

/**
 * Guess a MIME type from the file extension (case-insensitive).
 * Returns `application/octet-stream` for unknown extensions.
 *
 * Covers the union of extensions previously hand-listed in three
 * private copies — image formats plus `css`/`js` (which only the
 * iframePostProcessor copy carried).
 */
export function guessMimeType(path: string): string {
    const ext = path.split('.').pop()?.toLowerCase();
    const mimeTypes: Record<string, string> = {
        png: 'image/png',
        jpg: 'image/jpeg',
        jpeg: 'image/jpeg',
        gif: 'image/gif',
        svg: 'image/svg+xml',
        webp: 'image/webp',
        css: 'text/css',
        js: 'text/javascript',
    };
    return mimeTypes[ext || ''] || 'application/octet-stream';
}
