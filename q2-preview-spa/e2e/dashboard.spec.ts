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
