/**
 * Behavior tests for the `quarto-hrChanged` event in
 * `resources/js/quarto-nav/quarto-nav.js` (GH #1377). As in Quarto 1,
 * every `updateDocumentOffset` except the first dispatches the event on
 * `window`, so page scripts can re-place elements after the script resets
 * each `.sidebar`'s `top` and `max-height`.
 *
 * A browser's `ResizeObserver` calls back once as soon as it observes the
 * header; that first update is the one that stays silent. The fake
 * observer below models this. Each case runs the real script in a fresh
 * `JSDOM` window, as in `quarto-nav-announcement.integration.test.ts`.
 */

import { describe, expect, it } from 'vitest';
import { JSDOM } from 'jsdom';
import { readFileSync } from 'fs';
import path from 'path';

const NAV_JS = readFileSync(
    path.resolve(__dirname, '../../../../resources/js/quarto-nav/quarto-nav.js'),
    'utf-8',
);

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function page(opts: { resizeObserver: boolean }) {
    const dom = new JSDOM(
        `<!doctype html><html><body class="nav-fixed">` +
            `<header id="quarto-header" class="headroom fixed-top"><nav class="navbar">Nav</nav></header>` +
            `<div id="quarto-content" class="quarto-container">` +
            `<nav id="quarto-sidebar" class="sidebar">Sidebar</nav><main>Body</main></div>` +
            `</body></html>`,
        { runScripts: 'outside-only', url: 'https://example.org/' },
    );
    const win = dom.window;
    const doc = win.document;
    Object.defineProperty(doc.querySelector('#quarto-header')!, 'clientHeight', { get: () => 60 });

    if (opts.resizeObserver) {
        (win as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
            constructor(private cb: ResizeObserverCallback) {}
            observe() {
                win.setTimeout(() => this.cb([], this as unknown as ResizeObserver), 0);
            }
            unobserve() {}
            disconnect() {}
        };
    }

    const events: Event[] = [];
    win.addEventListener('quarto-hrChanged', (e) => events.push(e));
    win.eval(NAV_JS);
    if (doc.readyState === 'loading') {
        await new Promise((resolve) => doc.addEventListener('DOMContentLoaded', resolve));
    }
    return { win, doc, events };
}

describe('quarto-nav.js quarto-hrChanged event (GH #1377)', () => {
    it('fires after the 250ms initial update and after a resize, not on the first update', async () => {
        const { win, doc, events } = await page({ resizeObserver: true });

        // The observer's immediate callback is the first update: silent.
        await wait(20);
        expect(doc.body.getAttribute('data-bs-offset')).toBe('60');
        expect(events).toHaveLength(0);

        // Q1's 250ms initial measurement.
        await wait(300);
        expect(events).toHaveLength(1);
        expect(events[0]).toBeInstanceOf(win.CustomEvent);
        expect(events[0].bubbles).toBe(true);

        // Window resize (throttled by 50ms).
        win.dispatchEvent(new win.Event('resize'));
        await wait(100);
        expect(events).toHaveLength(2);
    });

    it('leaves the 250ms update silent when it is the first one', async () => {
        const { win, doc, events } = await page({ resizeObserver: false });

        await wait(300);
        expect(doc.body.style.paddingTop).toBe('60px');
        expect(events).toHaveLength(0);

        win.dispatchEvent(new win.Event('resize'));
        await wait(100);
        expect(events).toHaveLength(1);
    });
});
