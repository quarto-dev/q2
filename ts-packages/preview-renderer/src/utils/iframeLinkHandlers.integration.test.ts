/**
 * Unit tests for `installLinkHandlers`. The handler installs delegated
 * click/keydown listeners on a Document that route external links to
 * `window.open`, `.qmd` clicks and same-doc anchor clicks to a
 * caller-supplied `onQmdLinkClick`, and `Cmd/Ctrl+S` keydowns to a
 * `hub-client-save` postMessage on `window.parent`.
 *
 * The implementation uses event delegation rather than per-element
 * listeners so it survives React re-renders — q2-preview's iframe
 * mutates the AST DOM on every keystroke, so attaching a listener per
 * `<a>` would either compound or require a re-walk per render.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installLinkHandlers } from './iframeLinkHandlers';

let doc: Document;
let onQmdLinkClick: ReturnType<typeof vi.fn>;
let postMessageSpy: ReturnType<typeof vi.spyOn>;
let openSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
    // `document.implementation.createHTMLDocument()` produces a fresh
    // HTMLDocument with `<html>`, `<head>`, and `<body>` — `new Document()`
    // returns an unattached XML doc with `body === null` under jsdom.
    doc = document.implementation.createHTMLDocument();

    onQmdLinkClick = vi.fn();
    postMessageSpy = vi.spyOn(window.parent, 'postMessage');
    openSpy = vi.spyOn(window, 'open').mockImplementation(() => null);
});

afterEach(() => {
    postMessageSpy.mockRestore();
    openSpy.mockRestore();
});

function appendAnchor(href: string): HTMLAnchorElement {
    const a = doc.createElement('a');
    a.setAttribute('href', href);
    doc.body.appendChild(a);
    return a;
}

function clickFromBody(el: Element): boolean {
    // Synthesize a bubbling click that reaches the delegated listener
    // installed on doc.body. jsdom's HTMLAnchorElement.click() does not
    // bubble in a way the delegated handler can intercept, so dispatch
    // explicitly. The dispatched element type carries the same MouseEvent
    // / KeyboardEvent constructors as the parent jsdom window.
    const ev = new MouseEvent('click', {
        bubbles: true,
        cancelable: true,
    });
    return el.dispatchEvent(ev);
}

describe('installLinkHandlers', () => {
    test('external http link opens in a new tab and prevents default', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const a = appendAnchor('https://example.com');
        const continued = clickFromBody(a);

        expect(openSpy).toHaveBeenCalledWith(
            'https://example.com',
            '_blank',
            'noopener,noreferrer',
        );
        expect(continued).toBe(false); // preventDefault → dispatch returns false
        expect(onQmdLinkClick).not.toHaveBeenCalled();
    });

    test('.qmd link with anchor invokes onQmdLinkClick with resolved path and anchor', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo/bar.qmd',
            onQmdLinkClick,
        });

        const a = appendAnchor('other.qmd#sec');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: '/foo/other.qmd',
            anchor: 'sec',
        });
        expect(continued).toBe(false);
        expect(openSpy).not.toHaveBeenCalled();
    });

    test('same-document anchor click invokes onQmdLinkClick with anchor only', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const a = appendAnchor('#sec');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({ anchor: 'sec' });
        expect(continued).toBe(false);
    });

    test('non-qmd, non-anchor href is left alone (default click behavior preserved)', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const a = appendAnchor('about.html');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).not.toHaveBeenCalled();
        expect(openSpy).not.toHaveBeenCalled();
        expect(continued).toBe(true);
    });

    test('Cmd+S keydown posts hub-client-save and prevents default', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const ev = new KeyboardEvent('keydown', {
            key: 's',
            metaKey: true,
            bubbles: true,
            cancelable: true,
        });
        const continued = doc.dispatchEvent(ev);

        expect(postMessageSpy).toHaveBeenCalledWith(
            { type: 'hub-client-save' },
            '*',
        );
        expect(continued).toBe(false);
    });

    test('Ctrl+S keydown also posts hub-client-save', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const ev = new KeyboardEvent('keydown', {
            key: 's',
            ctrlKey: true,
            bubbles: true,
            cancelable: true,
        });
        doc.dispatchEvent(ev);

        expect(postMessageSpy).toHaveBeenCalledWith(
            { type: 'hub-client-save' },
            '*',
        );
    });

    test('clicks bubbling from a child of an anchor are still routed', () => {
        installLinkHandlers(doc, {
            currentFilePath: '/foo.qmd',
            onQmdLinkClick,
        });

        const a = appendAnchor('other.qmd');
        const span = doc.createElement('span');
        span.textContent = 'click me';
        a.appendChild(span);

        clickFromBody(span);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: '/other.qmd',
            anchor: null,
        });
    });

    // ─── Phase F.1 (bd-kw93.14): artifact-rooted .html links ────────
    //
    // After link-rewrite is included in the q2-preview pipeline, body
    // hrefs like `[A](about.qmd)` become artifact-rooted .html URLs
    // (`/.quarto/project-artifacts/about.html`). The link handler must
    // intercept these and route them through `onQmdLinkClick` with the
    // source-side `.qmd` path so the SPA can swap activeFile.

    test('artifact-rooted .html link maps back to its .qmd via projectFilePaths', () => {
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'about.qmd', 'posts/first.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('/.quarto/project-artifacts/about.html');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: 'about.qmd',
            anchor: null,
        });
        expect(continued).toBe(false);
    });

    test('artifact-rooted .html#anchor preserves the anchor', () => {
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'about.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('/.quarto/project-artifacts/about.html#intro');
        clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: 'about.qmd',
            anchor: 'intro',
        });
    });

    test('artifact-rooted nested page maps to its .qmd', () => {
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'posts/first.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('/.quarto/project-artifacts/posts/first.html');
        clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: 'posts/first.qmd',
            anchor: null,
        });
    });

    test('external https://...html link is NOT hijacked by artifact-root logic', () => {
        // Risk 2 from Phase F plan: an external link that happens to
        // end in .html (e.g. `https://example.org/index.html`) must
        // open in a new tab via the existing external-link handler,
        // never route through onQmdLinkClick.
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'about.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('https://example.org/about.html');
        clickFromBody(a);

        expect(openSpy).toHaveBeenCalledWith(
            'https://example.org/about.html',
            '_blank',
            'noopener,noreferrer',
        );
        expect(onQmdLinkClick).not.toHaveBeenCalled();
    });

    test('artifact-rooted link to a missing page still intercepts (overlay shows on failed render)', () => {
        // Plan §F.1 acceptance: missing-page link should surface the
        // D.4 error overlay rather than blanking the iframe with a
        // 404 navigation. So the handler intercepts even when the
        // reverse-map's .qmd candidate isn't in projectFilePaths;
        // PreviewApp's render attempt fails and the overlay appears.
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'about.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('/.quarto/project-artifacts/missing.html');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).toHaveBeenCalledWith({
            path: 'missing.qmd',
            anchor: null,
        });
        expect(continued).toBe(false);
    });

    test('non-artifact-rooted absolute path is left alone', () => {
        // A user-authored absolute href that isn't artifact-rooted
        // (e.g. someone hand-wrote `<a href="/about.html">`) should
        // not be intercepted — preserves the existing pass-through
        // behaviour for non-`.qmd`, non-`.html` links to user content.
        installLinkHandlers(doc, {
            currentFilePath: 'index.qmd',
            projectFilePaths: ['index.qmd', 'about.qmd'],
            onQmdLinkClick,
        });

        const a = appendAnchor('/about.html');
        const continued = clickFromBody(a);

        expect(onQmdLinkClick).not.toHaveBeenCalled();
        expect(continued).toBe(true);
    });
});
