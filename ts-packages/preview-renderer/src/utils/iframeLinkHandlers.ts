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
 *   - Click on an `<a href="/.quarto/project-artifacts/foo.html">`
 *     (Phase F.1, bd-kw93.14) — reverse-maps the artifact-rooted
 *     `.html` URL to its source `.qmd`, calls
 *     `onQmdLinkClick({ path: 'foo.qmd', anchor })`. We always
 *     intercept artifact-rooted hrefs (even when the candidate `.qmd`
 *     is not in `projectFilePaths`) so a missing-page click surfaces
 *     the D.4 render-error overlay rather than navigating the iframe
 *     to a 404. External `https://example.com/foo.html` links fall
 *     into the new-tab branch above and are never hijacked.
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
 */

import { resolveRelativePath } from './vfsPaths';

/**
 * VFS root the q2-preview WASM renderer uses for artifact paths. Body
 * `.qmd` hrefs go through `LinkRewriteTransform` and come out rooted
 * here as `.html` URLs (`/.quarto/project-artifacts/about.html`).
 *
 * Duplicated from `iframePostProcessor.ts`. bd-msp0 tracks hoisting
 * this constant once the service-worker resource resolution lands.
 */
const ARTIFACT_ROOT = '/.quarto/project-artifacts/';

export interface InstallLinkHandlersOptions {
    /** Current file path; used to resolve relative `.qmd` link targets. */
    currentFilePath: string;
    /**
     * Phase F.1 (bd-kw93.14): project file paths used to reverse-map
     * artifact-rooted `.html` clicks back to their source `.qmd`.
     * Optional and currently unused except for type-shape consistency
     * with `iframePostProcessor.ts` — the q2-preview link handler
     * intercepts every artifact-rooted href regardless of whether the
     * mapped `.qmd` is in this list, so the field is a documentation
     * hook for the future (e.g. a "did you mean ..." overlay).
     */
    projectFilePaths?: readonly string[];
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

        // Phase F.1: artifact-rooted .html hrefs land here after
        // LinkRewriteTransform runs. Always intercept; route the
        // .qmd candidate through onQmdLinkClick (PreviewApp's render
        // attempt is what surfaces a missing-page error).
        const artifact = parseArtifactHref(href);
        if (artifact && opts.onQmdLinkClick) {
            ev.preventDefault();
            opts.onQmdLinkClick({ path: artifact.qmdCandidate, anchor: artifact.anchor });
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

/**
 * Parse an artifact-rooted `.html` href into its `.qmd` source-path
 * candidate + optional anchor. Returns null for hrefs that don't
 * start with the artifact root or don't carry a `.html` stem.
 *
 * Examples:
 *   /.quarto/project-artifacts/about.html       → { qmdCandidate: 'about.qmd', anchor: null }
 *   /.quarto/project-artifacts/about.html#sec   → { qmdCandidate: 'about.qmd', anchor: 'sec' }
 *   /.quarto/project-artifacts/posts/x.html     → { qmdCandidate: 'posts/x.qmd', anchor: null }
 *   /.quarto/project-artifacts/styles.css       → null  (not .html)
 *   ./about.qmd                                 → null  (not artifact-rooted)
 *
 * Unlike `iframePostProcessor.ts::reverseMapArtifactHref`, this helper
 * does not consult `projectFilePaths` — see the docstring on
 * `installLinkHandlers` for the rationale (missing-page UX).
 */
function parseArtifactHref(href: string): { qmdCandidate: string; anchor: string | null } | null {
    if (!href.startsWith(ARTIFACT_ROOT)) return null;
    const stripped = href.slice(ARTIFACT_ROOT.length);
    const hashIdx = stripped.indexOf('#');
    const stem = hashIdx === -1 ? stripped : stripped.slice(0, hashIdx);
    const anchor = hashIdx === -1 ? null : stripped.slice(hashIdx + 1) || null;
    if (!stem.endsWith('.html')) return null;
    return {
        qmdCandidate: stem.slice(0, -'.html'.length) + '.qmd',
        anchor,
    };
}

function findAnchorAncestor(start: Element | null): HTMLAnchorElement | null {
    let node: Element | null = start;
    while (node) {
        if (node.tagName === 'A') return node as HTMLAnchorElement;
        node = node.parentElement;
    }
    return null;
}

