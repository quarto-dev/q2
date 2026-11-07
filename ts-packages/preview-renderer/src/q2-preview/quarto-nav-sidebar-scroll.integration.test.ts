/**
 * Behavior tests for the sidebar half of
 * `resources/js/quarto-nav/quarto-nav.js` (GH #1065, bd-b4q1xni0): on
 * page load, scroll `nav#quarto-sidebar` so the active item is visible,
 * as Quarto 1 does.
 *
 * JSDOM has no layout, so each case stubs the four geometry properties
 * the script reads (`offsetTop` on the active link; `scrollTop`,
 * `clientHeight`, `scrollHeight` on the sidebar) and installs a fake
 * `ResizeObserver` whose callback the test fires by hand, standing in
 * for "the sidebar's size resolved". Each case runs the real script in
 * a fresh `JSDOM` window, as in `quarto-nav-announcement.integration.test.ts`.
 */

import { describe, expect, it } from 'vitest';
import { JSDOM } from 'jsdom';
import { readFileSync } from 'fs';
import path from 'path';

const NAV_JS = readFileSync(
    path.resolve(__dirname, '../../../../resources/js/quarto-nav/quarto-nav.js'),
    'utf-8',
);

interface Geometry {
    /** `offsetTop` of the active link. */
    activeTop: number;
    clientHeight: number;
    scrollHeight: number;
}

function sidebarHtml(active: boolean) {
    const items = Array.from({ length: 40 }, (_, i) => {
        const cls = active && i === 35 ? 'sidebar-item-text sidebar-link active' : 'sidebar-item-text sidebar-link';
        return (
            `<li class="sidebar-item"><div class="sidebar-item-container">` +
            `<a href="./page-${i}.html" class="${cls}">Page ${i}</a></div></li>`
        );
    }).join('');
    return (
        `<nav id="quarto-sidebar" class="sidebar collapse collapse-horizontal sidebar-navigation floating overflow-auto">` +
        `<div class="sidebar-menu-container"><ul class="list-unstyled mt-1">${items}</ul></div></nav>`
    );
}

async function page(geom: Geometry, opts: { active?: boolean; url?: string } = {}) {
    const dom = new JSDOM(
        `<!doctype html><html><body><div id="quarto-content">` +
            `${sidebarHtml(opts.active !== false)}<main>Body</main></div></body></html>`,
        { runScripts: 'outside-only', url: opts.url ?? 'https://example.org/page-35.html' },
    );
    const win = dom.window;
    const doc = win.document;

    // Mutable so a case can model the sidebar's height resolving late.
    const g = { ...geom, scrollTop: 0 };
    const nav = doc.querySelector('nav#quarto-sidebar')!;
    Object.defineProperty(nav, 'clientHeight', { get: () => g.clientHeight });
    Object.defineProperty(nav, 'scrollHeight', { get: () => g.scrollHeight });
    Object.defineProperty(nav, 'scrollTop', {
        get: () => g.scrollTop,
        set: (v: number) => {
            g.scrollTop = v;
        },
    });
    const activeLink = doc.querySelector('li.sidebar-item a.active');
    if (activeLink) {
        Object.defineProperty(activeLink, 'offsetTop', { get: () => g.activeTop });
    }

    // Fake ResizeObserver: record what the script observes, fire by hand.
    const observers: { cb: ResizeObserverCallback; targets: Set<Element> }[] = [];
    (win as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
        targets = new Set<Element>();
        constructor(cb: ResizeObserverCallback) {
            observers.push({ cb, targets: this.targets });
        }
        observe(el: Element) {
            this.targets.add(el);
        }
        unobserve(el: Element) {
            this.targets.delete(el);
        }
        disconnect() {
            this.targets.clear();
        }
    };
    const fireSidebarResize = () => {
        for (const o of observers) {
            if (o.targets.has(nav)) {
                o.cb([], o as unknown as ResizeObserver);
            }
        }
    };
    const observingSidebar = () => observers.some((o) => o.targets.has(nav));

    win.eval(NAV_JS);
    if (doc.readyState === 'loading') {
        await new Promise((resolve) => doc.addEventListener('DOMContentLoaded', resolve));
    }
    return { g, fireSidebarResize, observingSidebar };
}

describe('quarto-nav.js sidebar active-item scroll (GH #1065)', () => {
    it('scrolls an active item below the visible area to the top', async () => {
        const { g, fireSidebarResize, observingSidebar } = await page({
            activeTop: 979,
            clientHeight: 800,
            scrollHeight: 1132,
        });
        expect(observingSidebar()).toBe(true);
        fireSidebarResize();
        expect(g.scrollTop).toBe(979);
        // One-shot: the observer stops watching once the scroll is done.
        expect(observingSidebar()).toBe(false);
    });

    it('leaves the sidebar alone when the active item is already visible', async () => {
        const { g, fireSidebarResize, observingSidebar } = await page({
            activeTop: 300,
            clientHeight: 800,
            scrollHeight: 1132,
        });
        fireSidebarResize();
        expect(g.scrollTop).toBe(0);
        expect(observingSidebar()).toBe(false);
    });

    it('waits until the sidebar height has resolved before scrolling', async () => {
        const { g, fireSidebarResize, observingSidebar } = await page({
            activeTop: 979,
            clientHeight: 800,
            // Still laying out: content height equals the viewport.
            scrollHeight: 800,
        });
        fireSidebarResize();
        expect(g.scrollTop).toBe(0);
        expect(observingSidebar()).toBe(true);

        g.scrollHeight = 1132;
        fireSidebarResize();
        expect(g.scrollTop).toBe(979);
        expect(observingSidebar()).toBe(false);
    });

    it('does nothing without an active item', async () => {
        const { g, observingSidebar } = await page(
            { activeTop: 979, clientHeight: 800, scrollHeight: 1132 },
            { active: false },
        );
        expect(observingSidebar()).toBe(false);
        expect(g.scrollTop).toBe(0);
    });

    it('also scrolls on file:// pages (q2 does not inherit Q1\'s http-only gate)', async () => {
        const { g, fireSidebarResize } = await page(
            { activeTop: 979, clientHeight: 800, scrollHeight: 1132 },
            { url: 'file:///tmp/site/page-35.html' },
        );
        fireSidebarResize();
        expect(g.scrollTop).toBe(979);
    });
});
