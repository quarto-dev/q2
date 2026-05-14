/**
 * Chrome injection (bd-kw93.15, Phase F.2).
 *
 * Each spec mounts the SPA against a real `examples/websites/`
 * fixture and asserts the chrome HTML pieces show up + behave.
 * The chrome strings come from the q2-preview pipeline (the
 * `*-render` transforms now included in F.2's pipeline change),
 * are passed to React via `meta.rendered.navigation.*`, and land
 * in the iframe DOM via `dangerouslySetInnerHTML` slots.
 *
 * Coverage:
 *   1. Navbar renders + clickable + cross-page nav (proves
 *      F.1 ↔ F.2 integration: the navbar is chrome-rendered HTML
 *      that uses link-rewritten artifact-rooted hrefs which the
 *      iframe link handler intercepts).
 *   2. Sidebar renders; active page highlighted; clicks switch
 *      and re-highlight.
 *   3. Page-nav (prev/next) renders on pages with sidebar
 *      ordering.
 *   4. TOC renders on a doc with sections.
 *   5. Footer renders on projects configuring page-footer.
 *   6. Bootstrap dropdown inside the navbar opens on click
 *      (chrome interactivity contract — the F.1 Bootstrap-JS
 *      injection has to work for the chrome too).
 *   7. Favicon `<link rel="icon">` lands in the iframe's
 *      `document.head`.
 *
 * Per CLAUDE.md "End-to-end verification before declaring
 * success": these specs spawn the real `q2 preview` binary
 * against the canonical fixture set, so they exercise the full
 * pipeline (Rust pass-1/pass-2, WASM bridge, samod sync, SPA
 * render) — not a unit-tested seam.
 */

import { test, expect, type Page } from '@playwright/test';
import path from 'node:path';
import {
    startPreviewServer,
    type PreviewServerHandle,
} from './helpers/previewServer';

const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');

async function waitForInnerHeading(page: Page, text: string) {
    await page.waitForFunction(
        (expected) => {
            const outer = document.querySelector('iframe');
            const innerDoc = outer?.contentDocument;
            const h1 = innerDoc?.querySelector('h1');
            return h1 != null && h1.textContent === expected;
        },
        text,
        { timeout: 30_000 },
    );
}

async function waitForInnerSelector(page: Page, selector: string) {
    await page.waitForFunction(
        (sel) => {
            const outer = document.querySelector('iframe');
            return outer?.contentDocument?.querySelector(sel) != null;
        },
        selector,
        { timeout: 30_000 },
    );
}

let server: PreviewServerHandle;

test.afterEach(async () => {
    await server?.stop();
});

// ──────────────────────────────────────────────────────────────
// Fixture: 04-navbar-footer/ — has navbar + footer (no sidebar)
// ──────────────────────────────────────────────────────────────

test.describe('navbar-footer fixture (chrome F.2)', () => {
    test.beforeEach(async () => {
        server = await startPreviewServer({
            copyFromDir: path.join(
                REPO_ROOT,
                'examples',
                'websites',
                '04-navbar-footer',
            ),
        });
    });

    test('navbar renders with brand + nav-links from _quarto.yml', async ({ page }) => {
        await page.goto(server.url);
        await waitForInnerHeading(page, 'Home');
        await waitForInnerSelector(page, 'nav.navbar');

        const navInfo = await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const nav = innerDoc.querySelector('nav.navbar')!;
            return {
                brand: nav.querySelector('.navbar-brand')?.textContent?.trim(),
                navLinks: Array.from(
                    nav.querySelectorAll('.nav-link'),
                ).map((a) => a.textContent?.trim()),
            };
        });
        expect(navInfo.brand).toBe('Demo Site');
        // Left items: Home, About, Tools (the dropdown trigger). Right item:
        // GitHub icon (textContent is empty for icon-only).
        expect(navInfo.navLinks).toEqual(
            expect.arrayContaining(['Home', 'About', 'Tools']),
        );
    });

    test('clicking a navbar link switches the active page (F.1↔F.2 integration)', async ({ page }) => {
        await page.goto(server.url);
        await waitForInnerHeading(page, 'Home');

        // The navbar's `About` link gets link-rewritten to an
        // artifact-rooted .html href. The iframe link handler
        // (Phase F.1) reverse-maps and routes through onNavigate.
        await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const link = Array.from(
                innerDoc.querySelectorAll('nav.navbar .nav-link'),
            ).find((a) => a.textContent?.trim() === 'About') as
                | HTMLAnchorElement
                | undefined;
            if (!link) throw new Error('No About link in navbar');
            link.click();
        });

        await waitForInnerHeading(page, 'About');
        // URL was updated by F.1's pushState (carries the new ?page=).
        expect(page.url()).toContain('page=about.qmd');
    });

    test('navbar dropdown opens on click (Bootstrap chrome interactivity)', async ({ page }) => {
        await page.goto(server.url);
        await waitForInnerHeading(page, 'Home');
        await waitForInnerSelector(page, 'nav.navbar .dropdown-toggle');

        // Bootstrap JS attaches a click delegate that toggles
        // `.show` on the sibling .dropdown-menu. The chrome has
        // a `Tools` dropdown configured in 04-navbar-footer/_quarto.yml.
        await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const trigger = innerDoc.querySelector(
                'nav.navbar .dropdown-toggle',
            ) as HTMLElement;
            trigger.click();
        });

        await page.waitForFunction(
            () => {
                const outer = document.querySelector('iframe') as HTMLIFrameElement;
                const innerDoc = outer.contentDocument!;
                const menu = innerDoc.querySelector(
                    'nav.navbar .dropdown-menu',
                );
                return menu?.classList.contains('show');
            },
            null,
            { timeout: 5_000 },
        );
    });

    test('footer renders the configured page-footer regions', async ({ page }) => {
        await page.goto(server.url);
        await waitForInnerHeading(page, 'Home');
        await waitForInnerSelector(page, 'footer.footer');

        const footer = await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const f = innerDoc.querySelector('footer.footer')!;
            return {
                left: f.querySelector('.nav-footer-left')?.textContent?.trim(),
                right: f.querySelector('.nav-footer-right')?.textContent?.trim(),
            };
        });
        expect(footer.left).toBe('Built with Quarto 2');
        expect(footer.right).toBe('© 2026 Example');
    });
});

// ──────────────────────────────────────────────────────────────
// Fixture: 02-auto-sidebar/ — has sidebar + body class change
// ──────────────────────────────────────────────────────────────

test.describe('auto-sidebar fixture (chrome F.2)', () => {
    test.beforeEach(async () => {
        server = await startPreviewServer({
            copyFromDir: path.join(
                REPO_ROOT,
                'examples',
                'websites',
                '02-auto-sidebar',
            ),
        });
    });

    test('sidebar renders inside #quarto-content with the discovered posts', async ({ page }) => {
        await page.goto(server.url);
        // index.qmd's frontmatter title is "Home"; "Auto Sidebar" is
        // the website-title used by the sidebar header.
        await waitForInnerHeading(page, 'Home');
        await waitForInnerSelector(page, 'nav#quarto-sidebar');

        const sidebar = await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const sb = innerDoc.querySelector('nav#quarto-sidebar')!;
            return {
                title:
                    sb.querySelector('.sidebar-title')?.textContent?.trim() ??
                    null,
                items: Array.from(
                    sb.querySelectorAll('.sidebar-item .menu-text'),
                ).map((s) => s.textContent?.trim()),
                inQuartoContent: !!innerDoc
                    .querySelector('#quarto-content')
                    ?.contains(sb),
            };
        });
        expect(sidebar.inQuartoContent).toBe(true);
        expect(sidebar.title).toBe('Auto Sidebar');
        // 02-auto-sidebar has 4 posts auto-discovered from posts/.
        expect(sidebar.items?.length).toBeGreaterThanOrEqual(4);
    });

    test('clicking a sidebar entry switches active page and re-highlights', async ({ page }) => {
        await page.goto(server.url);
        // index.qmd's frontmatter title is "Home"; "Auto Sidebar" is
        // the website-title used by the sidebar header.
        await waitForInnerHeading(page, 'Home');
        await waitForInnerSelector(page, 'nav#quarto-sidebar .sidebar-link');

        // Click the first post (Getting Started — alphabetic order via
        // the auto-sidebar's directory walk + frontmatter `order:`).
        await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const link = Array.from(
                innerDoc.querySelectorAll(
                    'nav#quarto-sidebar .sidebar-item .sidebar-link',
                ),
            ).find((a) =>
                a.textContent?.includes('Getting Started'),
            ) as HTMLAnchorElement | undefined;
            if (!link) throw new Error('Getting Started link not in sidebar');
            link.click();
        });

        // Wait for the new page's H1.
        await waitForInnerHeading(page, 'Getting Started');

        // After navigation, the sidebar re-renders with `Getting Started`
        // marked active (.sidebar-link.active). We don't assert byte-
        // perfect markup; just that the active highlight follows the page.
        const activeText = await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            return innerDoc
                .querySelector('nav#quarto-sidebar .sidebar-link.active')
                ?.textContent?.trim();
        });
        expect(activeText).toContain('Getting Started');
    });

    test('body class is set to nav-sidebar (sidebar layout)', async ({ page }) => {
        await page.goto(server.url);
        // index.qmd's frontmatter title is "Home"; "Auto Sidebar" is
        // the website-title used by the sidebar header.
        await waitForInnerHeading(page, 'Home');
        // Allow body-class commit after first render.
        await page.waitForFunction(
            () => {
                const outer = document.querySelector('iframe') as HTMLIFrameElement;
                return outer.contentDocument?.body.className.includes(
                    'nav-sidebar',
                );
            },
            null,
            { timeout: 5_000 },
        );
    });
});

// ──────────────────────────────────────────────────────────────
// Fixture: 03-nested-sidebar/ — sub-page has sidebar + page-nav
// ──────────────────────────────────────────────────────────────

test.describe('nested-sidebar fixture (chrome F.2)', () => {
    test.beforeEach(async () => {
        server = await startPreviewServer({
            copyFromDir: path.join(
                REPO_ROOT,
                'examples',
                'websites',
                '03-nested-sidebar',
            ),
        });
    });

    test('page-navigation prev/next renders on a mid-sequence page', async ({ page }) => {
        // Land on a guide sub-page that has sidebar ordering.
        // `server.url` already carries `?page=index.qmd` (the CLI's
        // root-index pick), so we re-parse and overwrite the query
        // rather than concatenate strings.
        const url = new URL(server.url);
        url.searchParams.set('page', 'guide/installation.qmd');
        await page.goto(url.toString());
        await waitForInnerHeading(page, 'Installation');
        await waitForInnerSelector(page, 'nav.page-navigation');

        const navPage = await page.evaluate(() => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            const innerDoc = outer.contentDocument!;
            const navEl = innerDoc.querySelector('nav.page-navigation')!;
            return {
                hasPrev: !!navEl.querySelector('.nav-page-previous'),
                hasNext: !!navEl.querySelector('.nav-page-next'),
                prevText:
                    navEl
                        .querySelector('.nav-page-previous .nav-page-text')
                        ?.textContent?.trim() ?? null,
                nextText:
                    navEl
                        .querySelector('.nav-page-next .nav-page-text')
                        ?.textContent?.trim() ?? null,
            };
        });
        expect(navPage.hasPrev).toBe(true);
        expect(navPage.hasNext).toBe(true);
        // Per the fixture: prev → User Guide, next → First Steps.
        expect(navPage.prevText).toBe('User Guide');
        expect(navPage.nextText).toBe('First Steps');
    });
});

// ──────────────────────────────────────────────────────────────
// TOC + favicon — inline fixtures (no canonical example exists
// for these in `examples/websites/`).
// ──────────────────────────────────────────────────────────────

test('TOC renders on a doc with sections', async ({ page }) => {
    server = await startPreviewServer({
        fixtureFiles: [
            { path: '_quarto.yml', content: 'project:\n  type: website\n' },
            {
                path: 'index.qmd',
                content: `---\ntitle: TOC Page\ntoc: true\n---\n\n# Section A\n\nA content.\n\n## Subsection A1\n\nA1 content.\n\n# Section B\n\nB content.\n`,
            },
        ],
    });

    await page.goto(server.url);
    await waitForInnerHeading(page, 'TOC Page');
    await waitForInnerSelector(page, '#quarto-margin-sidebar nav#TOC');

    const toc = await page.evaluate(() => {
        const outer = document.querySelector('iframe') as HTMLIFrameElement;
        const innerDoc = outer.contentDocument!;
        const margin = innerDoc.querySelector('#quarto-margin-sidebar')!;
        const navEl = margin.querySelector('nav#TOC')!;
        return {
            tocTitle: navEl.querySelector('h2#toc-title')?.textContent?.trim(),
            entries: Array.from(navEl.querySelectorAll('a')).map(
                (a) => a.textContent?.trim(),
            ),
        };
    });
    expect(toc.tocTitle).toBe('Table of Contents');
    expect(toc.entries).toEqual(
        expect.arrayContaining(['Section A', 'Subsection A1', 'Section B']),
    );
});

test('favicon link lands in iframe document.head', async ({ page }) => {
    // Inline the favicon as a tiny PNG fixture so the project actually
    // has a file at that path. We only assert the `<link rel="icon">`
    // shows up in the iframe head (iframe rendering — not the browser
    // tab). The href is artifact-rooted (`/.quarto/...`) because
    // `WebsiteFaviconTransform` resolves through the `vfs_root`
    // resolver in q2-preview's pipeline.
    server = await startPreviewServer({
        fixtureFiles: [
            {
                path: '_quarto.yml',
                content:
                    'project:\n  type: website\n\nwebsite:\n  favicon: favicon.png\n',
            },
            { path: 'index.qmd', content: '# Index\n\nHello.\n' },
            // 1x1 transparent PNG — minimum viable file at the
            // configured path so the resolver doesn't drop it.
            {
                path: 'favicon.png',
                content:
                    String.fromCharCode(
                        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
                        0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
                        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
                        0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
                        0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41,
                        0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
                        0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
                        0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
                        0x42, 0x60, 0x82,
                    ),
            },
        ],
    });

    await page.goto(server.url);
    await waitForInnerHeading(page, 'Index');

    await page.waitForFunction(
        () => {
            const outer = document.querySelector('iframe') as HTMLIFrameElement;
            return (
                outer.contentDocument?.head.querySelector(
                    'link[rel="icon"][data-q2-header-include]',
                ) != null
            );
        },
        null,
        { timeout: 5_000 },
    );

    const href = await page.evaluate(() => {
        const outer = document.querySelector('iframe') as HTMLIFrameElement;
        return outer.contentDocument!.head
            .querySelector('link[rel="icon"]')
            ?.getAttribute('href');
    });
    expect(href).toContain('favicon.png');
});
