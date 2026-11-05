/**
 * `format: dashboard` end to end (dashboard plan
 * `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 8).
 *
 * 1. The live preview (`q2-dashboard`, React): the dashboard shell and view
 *    nodes (`ts-packages/preview-renderer/src/q2-preview/custom/dashboard/`)
 *    switch pages and card tabs, collapse the sidebar and expand a card.
 * 2. The rendered page (`q2 preview --static`): the vendored bslib and
 *    dashboard JS run — the page in the URL hash is shown on load (Q1
 *    #14818, #9411), the tabs, sidebar toggle and card expand work, and
 *    the skip link targets the content area.
 */

import { test, expect, type Frame, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const DASHBOARD = `---
title: E2E Dashboard
format: dashboard
---

# Overview

## {.sidebar}

SIDEBAR-TEXT

## Column

::: {.card title="Main card"}
MAIN-CARD-TEXT
:::

## Column {.tabset}

::: {.card title="Tab A"}
TAB-A-TEXT
:::

::: {.card title="Tab B"}
TAB-B-TEXT
:::

# Details

::: {.card title="Detail card"}
DETAILS-TEXT
:::
`;

let server: PreviewServerHandle | undefined;

test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

/** The frame holding the React preview (the q2-preview iframe). */
async function previewFrame(page: Page): Promise<Frame> {
  await page.waitForFunction(
    () => {
      const doc = document.querySelector('iframe')?.contentDocument;
      return !!doc?.querySelector('#quarto-dashboard-header .navbar-title-text');
    },
    undefined,
    { timeout: 30_000 },
  );
  const frame = page.frames().find((f) => f !== page.mainFrame());
  if (!frame) throw new Error('no preview frame');
  return frame;
}

test('live preview: pages, card tabs, sidebar collapse and expand', async ({ page }) => {
  server = await startPreviewServer({ fixtureFiles: [{ path: 'index.qmd', content: DASHBOARD }] });
  await page.goto(server.url);
  const f = await previewFrame(page);

  await expect(f.locator('.navbar-title-text')).toHaveText('E2E Dashboard');
  const overview = f.locator('#overview');
  const details = f.locator('#details');
  await expect(overview).toHaveClass(/\bactive\b/);
  await expect(details).not.toHaveClass(/\bactive\b/);
  await f.locator('#tab-details').click();
  await expect(details).toHaveClass(/\bactive\b/);
  await expect(overview).not.toHaveClass(/\bactive\b/);
  await f.locator('#tab-overview').click();

  // Card tabs.
  const tabB = f.locator('.card-header-tabs a', { hasText: 'Tab B' });
  await tabB.click();
  await expect(tabB).toHaveAttribute('aria-selected', 'true');
  await expect(f.locator('.bslib-card .tab-pane.active', { hasText: 'TAB-B-TEXT' })).toHaveCount(1);

  // Sidebar collapse.
  const layout = f.locator('.bslib-sidebar-layout');
  await f.locator('button.collapse-toggle').click();
  await expect(layout).toHaveClass(/sidebar-collapsed/);
  await f.locator('button.collapse-toggle').click();
  await expect(layout).not.toHaveClass(/sidebar-collapsed/);

  // Card full screen.
  const card = f.locator('.bslib-card', { hasText: 'MAIN-CARD-TEXT' });
  // bslib's CSS shows the expand control on hover only.
  await card.hover();
  await card.locator('.bslib-full-screen-enter').click();
  await expect(card).toHaveAttribute('data-full-screen', 'true');
  await f.locator('#bslib-full-screen-overlay .bslib-full-screen-exit').click();
  await expect(card).toHaveAttribute('data-full-screen', 'false');
});

test('rendered page: hash navigation, tabs, collapse, expand, skip link', async ({ page }) => {
  // `--static` serves a project (a bare folder has no render list).
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: default\n' },
      { path: 'index.qmd', content: DASHBOARD },
    ],
    staticMode: true,
  });
  const errors: string[] = [];
  // bslib's sidebar toggle ends with a jQuery call for Shiny's input
  // binding; dashboards load no jQuery (Quarto 1 neither), so it throws
  // after the toggle has happened (bd-tolifxld).
  const knownBslibError = (m: string) => m === '$ is not defined';
  page.on('pageerror', (e) => {
    if (!knownBslibError(e.message)) errors.push(e.message);
  });

  // The page named in the hash is shown on load (Q1 #14818 / #9411).
  await page.goto(server.url + 'index.html#details');
  await expect(page.locator('#details')).toHaveClass(/\bactive\b/);
  await expect(page.locator('#overview')).not.toHaveClass(/\bactive\b/);

  await page.locator('#tab-overview').click();
  await expect(page.locator('#overview')).toHaveClass(/\bactive\b/);

  const tabB = page.locator('.card-header-tabs a', { hasText: 'Tab B' });
  await tabB.click();
  await expect(tabB).toHaveAttribute('aria-selected', 'true');

  const layout = page.locator('.bslib-sidebar-layout').first();
  await page.locator('button.collapse-toggle').first().click();
  await expect(layout).toHaveClass(/sidebar-collapsed/);

  const card = page.locator('.bslib-card', { hasText: 'MAIN-CARD-TEXT' });
  await card.hover();
  await card.locator('.bslib-full-screen-enter').click();
  await expect(card).toHaveAttribute('data-full-screen', 'true');
  await page.keyboard.press('Escape');
  await expect(card).toHaveAttribute('data-full-screen', 'false');

  await expect(page.locator('#quarto-skip-link')).toHaveAttribute('href', '#quarto-document-content');
  await expect(page.locator('#quarto-document-content')).toHaveAttribute('role', 'main');

  expect(errors, errors.join('\n')).toEqual([]);
});

test('rendered page in a website: under the site header, not beneath it', async ({ page }) => {
  server = await startPreviewServer({
    fixtureFiles: [
      {
        path: '_quarto.yml',
        content:
          'project:\n  type: website\nwebsite:\n  title: Site\n  navbar:\n    left:\n      - href: index.qmd\n        text: Home\n      - href: dash.qmd\n        text: Dash\n  page-footer: FOOTER-TEXT\nformat:\n  html:\n    theme: cosmo\n',
      },
      { path: 'index.qmd', content: '---\ntitle: Home\n---\n\nHome.\n' },
      { path: 'dash.qmd', content: DASHBOARD },
    ],
    staticMode: true,
  });
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto(server.url + 'dash.html');

  const site = page.locator('#quarto-header');
  const dashHeader = site.locator('#quarto-dashboard-header');
  await expect(dashHeader).toBeVisible();
  await expect(site.locator('nav.navbar a.nav-link', { hasText: 'Home' })).toBeVisible();

  // The content area starts below the fixed site header (body offset) and
  // the first card is fully on screen.
  const headerBox = (await site.boundingBox())!;
  const contentBox = (await page.locator('#quarto-content').boundingBox())!;
  expect(contentBox.y).toBeGreaterThanOrEqual(headerBox.y + headerBox.height - 1);
  const cardBox = (await page.locator('.bslib-card', { hasText: 'MAIN-CARD-TEXT' }).boundingBox())!;
  expect(cardBox.y).toBeGreaterThanOrEqual(headerBox.y + headerBox.height - 1);
  expect(cardBox.height).toBeGreaterThan(50);
  // The site footer stays on screen below the fill layout (quarto-nav.js
  // subtracts it from the content's min-height on dashboard pages).
  const footerBox = (await page.locator('footer.footer').boundingBox())!;
  expect(footerBox.y + footerBox.height).toBeLessThanOrEqual(800 + 1);

  // Dashboard page tabs still work inside the site chrome.
  await page.locator('#tab-details').click();
  await expect(page.locator('#details')).toHaveClass(/\bactive\b/);
});

test('live preview in a website: site header wraps the dashboard header', async ({ page }) => {
  server = await startPreviewServer({
    fixtureFiles: [
      {
        path: '_quarto.yml',
        content:
          'project:\n  type: website\nwebsite:\n  title: Site\n  navbar:\n    left:\n      - href: index.qmd\n        text: Dash\n      - href: about.qmd\n        text: About\n  page-footer: FOOTER-TEXT\n',
      },
      { path: 'index.qmd', content: DASHBOARD },
      { path: 'about.qmd', content: '---\ntitle: About\n---\n\nAbout.\n' },
    ],
  });
  await page.goto(server.url);
  const f = await previewFrame(page);
  await expect(f.locator('header#quarto-header nav.navbar a', { hasText: 'About' })).toBeVisible();
  await expect(f.locator('header#quarto-header #quarto-dashboard-header .navbar-title-text')).toHaveText(
    'E2E Dashboard',
  );
  await expect(f.locator('div#quarto-content.quarto-dashboard-content')).toHaveCount(1);
  await expect(f.locator('footer.footer')).toContainText('FOOTER-TEXT');
  await f.locator('#tab-details').click();
  await expect(f.locator('#details')).toHaveClass(/\bactive\b/);
});
