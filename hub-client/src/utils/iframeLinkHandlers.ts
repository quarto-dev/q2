/**
 * Link-handler utility for q2-preview's iframe surface. Installs three
 * delegated listeners on a Document:
 *
 *   - Click on an `<a href="https://…">` (or `http://…`) — opens in a
 *     new tab and prevents default.
 *   - Click on an `<a href="*.qmd">` or `<a href="*.qmd#frag">` — calls
 *     `onQmdLinkClick({ path: <resolved>, anchor })` and prevents
 *     default. Relative paths are resolved against `currentFilePath`.
 *   - Click on an `<a href="#sec">` (same-document anchor) — calls
 *     `onQmdLinkClick({ anchor: 'sec' })` and prevents default.
 *   - `Cmd+S` / `Ctrl+S` keydown — posts `{ type: 'hub-client-save' }`
 *     to `window.parent` and prevents default.
 *
 * **Why event delegation, not per-element listeners.** q2-preview's
 * iframe is React-rendered; the AST DOM rebuilds on every keystroke.
 * Per-element listeners would either compound or require a re-walk per
 * render. A single delegated listener on `doc.body` survives mutations
 * and runs once per click.
 *
 * The HTML iframe's per-element pattern in `iframePostProcessor.ts`
 * stays as-is — that path is one-shot post-processing of fetched HTML
 * (single DOM walk, single attach), not a continuously-rendered React
 * tree. The contrast is "one-shot HTML walk vs continuously-re-rendered
 * React DOM," not q2-debug vs q2-preview.
 *
 * The `/.quarto/...` artifact-rooted reverse-mapping branch from
 * `iframePostProcessor.ts:253-272` has no analog here — q2-preview's
 * pipeline excludes `LinkRewriteTransform`, so artifact-rooted hrefs
 * never appear in the q2-preview AST.
 */

export interface InstallLinkHandlersOptions {
    /** Current file path; used to resolve relative `.qmd` link targets. */
    currentFilePath: string;
    /**
     * Click callback for `.qmd` links and same-document anchor clicks.
     * - With a `.qmd` link → `{ path: <resolved>, anchor: <fragment | null> }`.
     * - With a `#frag` link → `{ anchor: <fragment> }`.
     * Optional — when omitted, internal links fall through to default
     * browser behavior.
     */
    onQmdLinkClick?: (
        arg: { path: string; anchor: string | null } | { anchor: string },
    ) => void;
}

export function installLinkHandlers(
    doc: Document,
    opts: InstallLinkHandlersOptions,
): void {
    const body = doc.body;
    if (!body) return;

    body.addEventListener('click', (ev) => {
        const anchor = findAnchorAncestor(ev.target as Element | null);
        if (!anchor) return;
        const href = anchor.getAttribute('href');
        if (!href) return;

        if (href.startsWith('http://') || href.startsWith('https://')) {
            ev.preventDefault();
            window.open(href, '_blank', 'noopener,noreferrer');
            return;
        }

        if (href.startsWith('#')) {
            const parsed = parseLink(href);
            if (parsed.anchor && opts.onQmdLinkClick) {
                ev.preventDefault();
                opts.onQmdLinkClick({ anchor: parsed.anchor });
            }
            return;
        }

        const parsed = parseLink(href);
        if (parsed.path && parsed.path.endsWith('.qmd') && opts.onQmdLinkClick) {
            ev.preventDefault();
            const resolved = resolveRelativePath(opts.currentFilePath, parsed.path);
            opts.onQmdLinkClick({ path: resolved, anchor: parsed.anchor });
        }
    });

    doc.addEventListener('keydown', (ev) => {
        if ((ev.ctrlKey || ev.metaKey) && ev.key === 's') {
            ev.preventDefault();
            window.parent.postMessage({ type: 'hub-client-save' }, '*');
        }
    });
}

interface ParsedLink {
    path: string | null;
    anchor: string | null;
}

function parseLink(href: string): ParsedLink {
    const hashIndex = href.indexOf('#');
    if (hashIndex === -1) {
        return { path: href, anchor: null };
    }
    const path = hashIndex === 0 ? null : href.substring(0, hashIndex);
    const anchor = href.substring(hashIndex + 1);
    return { path, anchor: anchor || null };
}

function findAnchorAncestor(start: Element | null): HTMLAnchorElement | null {
    let node: Element | null = start;
    while (node) {
        if (node.tagName === 'A') return node as HTMLAnchorElement;
        node = node.parentElement;
    }
    return null;
}

function resolveRelativePath(currentFile: string, relativePath: string): string {
    if (relativePath.startsWith('/')) return relativePath;
    const lastSlash = currentFile.lastIndexOf('/');
    const currentDir = lastSlash >= 0 ? currentFile.substring(0, lastSlash + 1) : '/';
    return normalizePath(currentDir + relativePath);
}

function normalizePath(path: string): string {
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
