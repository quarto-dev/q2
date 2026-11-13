/**
 * Iframe-side `UPDATE_THEME` handling test (Plan 2A §"Test plan" —
 * "Theme injection test (iframe side)").
 *
 * Imports `q2-preview/entry` for its side effect (registers the
 * module-top message listener), then dispatches synthetic
 * `UPDATE_THEME` MessageEvents on `window` and asserts `document.head`
 * mutations directly. No `<PreviewRoot>` mount — theme is module-top
 * DOM mutation, not React state.
 *
 * The parent posts the compiled CSS as **text** (`cssText`), and the
 * iframe mints the `blob:` URL itself. A parent-minted URL would only
 * load in the parent's origin, which differs from the iframe's when the
 * host sandboxes the preview without `allow-same-origin`.
 *
 * The parent never posts `cssText: undefined` by contract; only
 * `string` and `null` reach the iframe. Sequence:
 *   - dispatch `cssText: 'a'` → one `<link>` with a minted href
 *   - dispatch new text → href updated, still one `<link>`, old URL revoked
 *   - dispatch `cssText: null` → `<link>` removed, URL revoked
 *   - dispatch text again → fresh `<link>` re-created
 */

import { describe, test, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest';

// Stub modules that the entry imports but the test doesn't need to
// exercise. Loading the real `react-dom/client` createRoot would also
// pull in the framework runtime; we don't drive AST renders here.
vi.mock('react-dom/client', () => ({
    createRoot: vi.fn(() => ({ render: vi.fn() })),
}));
vi.mock('katex/dist/katex.min.css', () => ({}));

// The entry imports `installLinkHandlers` and the framework. Keep the
// real ones — they're side-effect-free at import time.

let createUrlSpy: ReturnType<typeof vi.fn>;
let revokeUrlSpy: ReturnType<typeof vi.fn>;
let mintedBlobs: Blob[];

beforeAll(async () => {
    // Side-effect import: registers the module-top message listener
    // and the module-top global. Subsequent dispatches drive it.
    await import('./entry');
});

beforeEach(() => {
    // Clear any theme a previous case left behind (this also revokes
    // its URL through the real handler), then reset the DOM and spies.
    // jsdom does not implement `URL.createObjectURL` / `revokeObjectURL`,
    // so stub them; each mint returns a unique synthetic URL.
    let counter = 0;
    mintedBlobs = [];
    createUrlSpy = vi.fn((blob: Blob) => {
        mintedBlobs.push(blob);
        return `blob:iframe:${++counter}`;
    });
    revokeUrlSpy = vi.fn();
    URL.createObjectURL = createUrlSpy as unknown as typeof URL.createObjectURL;
    URL.revokeObjectURL = revokeUrlSpy as unknown as typeof URL.revokeObjectURL;
    dispatchTheme(null);
    revokeUrlSpy.mockClear();
});

afterEach(() => {
    dispatchTheme(null);
});

function dispatchTheme(cssText: string | null) {
    window.dispatchEvent(
        new MessageEvent('message', {
            data: { type: 'UPDATE_THEME', cssText },
        }),
    );
}

// jsdom's Blob has no `text()`; FileReader reads it.
function readBlobText(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = () => reject(reader.error);
        reader.readAsText(blob);
    });
}

function themeLinks(): NodeListOf<HTMLLinkElement> {
    return document.head.querySelectorAll('link[data-q2-theme]');
}

describe('q2-preview/entry UPDATE_THEME handler', () => {
    test('cssText=string creates a single <link> with an iframe-minted URL', async () => {
        dispatchTheme('body { color: red; }');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('rel')).toBe('stylesheet');
        expect(links[0].getAttribute('href')).toBe('blob:iframe:1');
        expect(createUrlSpy).toHaveBeenCalledTimes(1);
        expect(mintedBlobs[0].type).toBe('text/css');
        expect(await readBlobText(mintedBlobs[0])).toBe('body { color: red; }');
    });

    test('new cssText updates href on the same element and revokes the old URL', () => {
        dispatchTheme('a');
        dispatchTheme('b');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:iframe:2');
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:iframe:1');
        expect(revokeUrlSpy).not.toHaveBeenCalledWith('blob:iframe:2');
    });

    test('cssText=null removes the <link> and revokes its URL', () => {
        dispatchTheme('a');
        expect(themeLinks()).toHaveLength(1);
        dispatchTheme(null);
        expect(themeLinks()).toHaveLength(0);
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:iframe:1');
    });

    test('cssText=string after null re-creates the <link>', () => {
        dispatchTheme('a');
        dispatchTheme(null);
        dispatchTheme('c');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:iframe:2');
    });

    test('repeated null is safely a no-op', () => {
        dispatchTheme(null);
        dispatchTheme(null);
        expect(themeLinks()).toHaveLength(0);
        expect(createUrlSpy).not.toHaveBeenCalled();
        expect(revokeUrlSpy).not.toHaveBeenCalled();
    });
});

describe('q2-preview/entry global', () => {
    test('__Q2_PREVIEW_RENDERER__ is set at module top', () => {
        expect((window as any).__Q2_PREVIEW_RENDERER__).toBeDefined();
    });
});
