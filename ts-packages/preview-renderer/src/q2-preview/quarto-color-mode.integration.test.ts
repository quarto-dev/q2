/**
 * Behavior tests for the color-scheme toggle in
 * `crates/quarto-core/resources/js/quarto-color-mode.js` (bd-kwgtllvn,
 * Quarto 1 #14739): every toggle is a `role="switch"` whose
 * `aria-checked` tracks the active mode, and the floating fallback (a page
 * with no navbar toggle) is a localized `<button>` inside a landmark of
 * its own — a `<header>` when the page has no banner yet, a named
 * `region` otherwise.
 *
 * The runtime reads its config from `document.currentScript.dataset`, so
 * it must run as a real inline `<script>` during parsing, exactly where
 * `template.rs` puts it (first thing in `<body>`). Hence `runScripts:
 * 'dangerously'` with the page HTML in the constructor, and
 * `beforeParse` to seed a stored preference before the script reads it.
 * The source is read with `fs`, as in the quarto-nav tests: Vite returns
 * empty for `?raw` files outside the package root.
 */

import { describe, expect, it } from 'vitest';
import { JSDOM, type DOMWindow } from 'jsdom';
import { readFileSync } from 'fs';
import path from 'path';

const COLOR_MODE_JS = readFileSync(
    path.resolve(
        __dirname,
        '../../../../crates/quarto-core/resources/js/quarto-color-mode.js',
    ),
    'utf-8',
);

interface PageOpts {
    /** HTML placed in `<body>` after the runtime script. */
    body?: string;
    authorPrefersDark?: boolean;
    /** `data-toggle-label`; omitted entirely when `null`. */
    label?: string | null;
    /** Value of `localStorage["quarto-color-scheme"]` before load. */
    stored?: 'alternate' | 'default';
}

async function page(opts: PageOpts = {}) {
    const label =
        opts.label === null
            ? ''
            : ` data-toggle-label="${opts.label ?? 'Basculer le mode sombre'}"`;
    const html =
        `<!doctype html><html><head>` +
        `<link rel="stylesheet" href="styles.css" class="quarto-color-scheme" id="quarto-bootstrap" data-mode="light">` +
        `<link rel="stylesheet" href="styles-dark.css" class="quarto-color-scheme quarto-color-alternate" id="quarto-bootstrap" data-mode="dark">` +
        `</head><body>` +
        `<script id="quarto-color-mode" data-author-prefers-dark="${opts.authorPrefersDark ? 'true' : 'false'}" ` +
        `data-respect-user-color-scheme="false"${label}>${COLOR_MODE_JS}</script>` +
        `${opts.body ?? '<main id="quarto-document-content">Body</main>'}` +
        `</body></html>`;
    const dom = new JSDOM(html, {
        runScripts: 'dangerously',
        url: 'https://example.org/doc.html',
        beforeParse(window) {
            if (opts.stored) {
                window.localStorage.setItem('quarto-color-scheme', opts.stored);
            }
        },
    });
    const doc = dom.window.document;
    if (doc.readyState === 'loading') {
        await new Promise((resolve) => doc.addEventListener('DOMContentLoaded', resolve));
    }
    return dom.window;
}

/** The navbar's toggle, as `navbar_to_html` emits it. */
const NAVBAR_TOGGLE =
    '<header id="quarto-header"><nav class="navbar"><div class="quarto-navbar-tools">' +
    '<button type="button" role="switch" aria-checked="false" ' +
    'class="quarto-color-scheme-toggle quarto-navigation-tool px-1" ' +
    'onclick="window.quartoToggleColorScheme(); return false;" ' +
    'title="Toggle dark mode" aria-label="Toggle dark mode"><i class="bi"></i></button>' +
    '</div></nav></header><main>Body</main>';

const toggles = (win: DOMWindow) =>
    Array.from(win.document.querySelectorAll('.quarto-color-scheme-toggle'));

describe('quarto-color-mode.js: aria-checked tracks the mode', () => {
    it('is false on a light-default page', async () => {
        const win = await page({ body: NAVBAR_TOGGLE });
        expect(toggles(win).map((t) => t.getAttribute('aria-checked'))).toEqual(['false']);
    });

    it('is true when the author prefers dark', async () => {
        const win = await page({ body: NAVBAR_TOGGLE, authorPrefersDark: true });
        expect(toggles(win)[0].getAttribute('aria-checked')).toBe('true');
    });

    it('follows a stored preference over the author default', async () => {
        const win = await page({ body: NAVBAR_TOGGLE, stored: 'alternate' });
        expect(toggles(win)[0].getAttribute('aria-checked')).toBe('true');
    });

    it('flips on every toggle, for every toggle on the page', async () => {
        // Two toggles stand in for any mix (navbar, dashboard, fallback):
        // the runtime syncs them all through one code path.
        const second =
            '<button type="button" role="switch" aria-checked="false" ' +
            'class="quarto-color-scheme-toggle"><i class="bi"></i></button>';
        const win = await page({ body: NAVBAR_TOGGLE + second });
        const all = toggles(win);
        expect(all).toHaveLength(2);

        (win as unknown as { quartoToggleColorScheme: () => void }).quartoToggleColorScheme();
        expect(all.map((t) => t.getAttribute('aria-checked'))).toEqual(['true', 'true']);
        expect(all.every((t) => t.classList.contains('alternate'))).toBe(true);

        (win as unknown as { quartoToggleColorScheme: () => void }).quartoToggleColorScheme();
        expect(all.map((t) => t.getAttribute('aria-checked'))).toEqual(['false', 'false']);
    });
});

describe('quarto-color-mode.js: floating fallback toggle', () => {
    it('is a localized switch button', async () => {
        const win = await page();
        const all = toggles(win);
        expect(all).toHaveLength(1);
        const t = all[0];
        expect(t.tagName).toBe('BUTTON');
        expect(t.getAttribute('type')).toBe('button');
        expect(t.getAttribute('role')).toBe('switch');
        expect(t.getAttribute('aria-checked')).toBe('false');
        expect(t.getAttribute('aria-label')).toBe('Basculer le mode sombre');
        expect(t.classList.contains('top-right')).toBe(true);
        expect(t.querySelector('i.bi')).not.toBeNull();
        expect(t.hasAttribute('href')).toBe(false);
    });

    it('falls back to the English label without data-toggle-label', async () => {
        const win = await page({ label: null });
        expect(toggles(win)[0].getAttribute('aria-label')).toBe('Toggle dark mode');
    });

    it('reports dark state from the start on a dark page', async () => {
        const win = await page({ authorPrefersDark: true });
        expect(toggles(win)[0].getAttribute('aria-checked')).toBe('true');
    });

    it('is not added when the page already has a toggle', async () => {
        const win = await page({ body: NAVBAR_TOGGLE });
        expect(toggles(win)).toHaveLength(1);
        expect(win.document.querySelector('.quarto-color-scheme-toggle-container')).toBeNull();
    });
});

describe('quarto-color-mode.js: fallback landmark', () => {
    const container = (win: DOMWindow) => {
        const c = win.document.querySelector('.quarto-color-scheme-toggle-container');
        expect(c).not.toBeNull();
        expect(c!.parentElement).toBe(win.document.body);
        expect(c!.querySelector('button.quarto-color-scheme-toggle')).not.toBeNull();
        return c!;
    };

    it('is a header when the page has no banner', async () => {
        const c = container(await page());
        expect(c.tagName).toBe('HEADER');
        expect(c.hasAttribute('role')).toBe(false);
    });

    for (const [name, body] of [
        [
            'a body-level header',
            '<header id="title-block-header"><h1>T</h1></header><main>Body</main>',
        ],
        [
            'a header nested in a plain wrapper (custom layout)',
            '<div class="page-layout"><header id="title-block-header"><h1>T</h1></header></div><main>Body</main>',
        ],
        ['an explicit role="banner"', '<div role="banner">Site</div><main>Body</main>'],
    ] as const) {
        it(`is a named region when the page already has ${name}`, async () => {
            const c = container(await page({ body }));
            expect(c.tagName).toBe('DIV');
            expect(c.getAttribute('role')).toBe('region');
            expect(c.getAttribute('aria-label')).toBe('Basculer le mode sombre');
        });
    }

    it('is a header when the only header sits inside sectioning content', async () => {
        // A <header> inside <main>/<article> is not a banner, so the page
        // still has none and the fallback may provide it.
        const c = container(
            await page({
                body: '<main><article><header><h1>T</h1></header>Body</article></main>',
            }),
        );
        expect(c.tagName).toBe('HEADER');
    });
});
