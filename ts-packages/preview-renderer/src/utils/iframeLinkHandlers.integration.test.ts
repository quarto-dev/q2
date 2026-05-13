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
});
