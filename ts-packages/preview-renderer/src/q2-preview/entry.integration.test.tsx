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
 * The parent never posts `cssUrl: undefined` by contract; only
 * `string` and `null` reach the iframe. Sequence:
 *   - dispatch `cssUrl: 'blob:abc'` → one `<link>` with that href
 *   - dispatch same `cssUrl` again → no duplication
 *   - dispatch `cssUrl: 'blob:def'` → href updated, still one `<link>`
 *   - dispatch `cssUrl: null` → `<link>` removed
 *   - dispatch `cssUrl: 'blob:ghi'` → fresh `<link>` re-created
 */

import { describe, test, expect, beforeAll, beforeEach, vi } from 'vitest';

// Stub modules that the entry imports but the test doesn't need to
// exercise. Loading the real `react-dom/client` createRoot would also
// pull in the framework runtime; we don't drive AST renders here.
vi.mock('react-dom/client', () => ({
    createRoot: vi.fn(() => ({ render: vi.fn() })),
}));
vi.mock('katex/dist/katex.min.css', () => ({}));

// The entry imports `installLinkHandlers` and the framework. Keep the
// real ones — they're side-effect-free at import time.

beforeAll(async () => {
    // Side-effect import: registers the module-top message listener
    // and the module-top global. Subsequent dispatches drive it.
    await import('./entry');
});

beforeEach(() => {
    // Reset DOM between cases.
    document.head
        .querySelectorAll('link[data-q2-theme]')
        .forEach((el) => el.remove());
});

function dispatchTheme(cssUrl: string | null) {
    window.dispatchEvent(
        new MessageEvent('message', {
            data: { type: 'UPDATE_THEME', cssUrl },
        }),
    );
}

function themeLinks(): NodeListOf<HTMLLinkElement> {
    return document.head.querySelectorAll('link[data-q2-theme]');
}

describe('q2-preview/entry UPDATE_THEME handler', () => {
    test('cssUrl=string creates a single <link> with the URL', () => {
        dispatchTheme('blob:mock:abc');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:mock:abc');
        expect(links[0].getAttribute('rel')).toBe('stylesheet');
    });

    test('repeated cssUrl with the same value does not duplicate', () => {
        dispatchTheme('blob:mock:abc');
        dispatchTheme('blob:mock:abc');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:mock:abc');
    });

    test('different cssUrl updates href on the same element', () => {
        dispatchTheme('blob:mock:abc');
        dispatchTheme('blob:mock:def');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:mock:def');
    });

    test('cssUrl=null removes the <link>', () => {
        dispatchTheme('blob:mock:abc');
        expect(themeLinks()).toHaveLength(1);
        dispatchTheme(null);
        expect(themeLinks()).toHaveLength(0);
    });

    test('cssUrl=string after null re-creates the <link>', () => {
        dispatchTheme('blob:mock:abc');
        dispatchTheme(null);
        dispatchTheme('blob:mock:ghi');
        const links = themeLinks();
        expect(links).toHaveLength(1);
        expect(links[0].getAttribute('href')).toBe('blob:mock:ghi');
    });

    test('repeated null is safely a no-op', () => {
        dispatchTheme(null);
        dispatchTheme(null);
        expect(themeLinks()).toHaveLength(0);
    });
});

describe('q2-preview/entry global', () => {
    test('__Q2_PREVIEW_RENDERER__ is set at module top', () => {
        expect((window as any).__Q2_PREVIEW_RENDERER__).toBeDefined();
    });
});
