/**
 * The demand-filled preview VFS (preview VFS contract D2, D8, D11).
 *
 * Files the render needs but sync did not push (a theme's Sass partials,
 * a bibliography) are pulled from the server on the render's miss, in
 * single-file and project mode alike. A file created mid-session where the
 * render looked arrives and re-renders the page. The pulls and their sync
 * echoes cost exactly one render, and the render's read set decides which
 * edits re-render.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const ticks = (page: Page) =>
  page.evaluate(() => (window as unknown as { __renderTicks?: number }).__renderTicks ?? 0);

const missStats = (page: Page) =>
  page.evaluate(() =>
    (window as unknown as { __q2MissStats: () => Promise<Record<string, number>> }).__q2MissStats(),
  );

const bodyBackground = (page: Page) =>
  page.evaluate(() => {
    const doc = document.querySelector('iframe')?.contentDocument;
    return doc?.body ? getComputedStyle(doc.body).backgroundColor : null;
  });

const iframeText = (page: Page) =>
  page.evaluate(() => document.querySelector('iframe')?.contentDocument?.body?.innerText ?? '');

const THEMED_DECK = `---
title: Deck
format:
  html:
    theme: styles/deck.scss
---

# Themed

Body text.
`;

const DECK_SCSS = `/*-- scss:defaults --*/
@import "partials/vars";
$body-bg: $deck-bg !default;

/*-- scss:rules --*/
`;

let server: PreviewServerHandle | undefined;

test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

test('single-file: a theme pulls its partials, and a partial created mid-session re-themes', async ({ page }) => {
  server = await startPreviewServer({
    targetFile: 'deck.qmd',
    fixtureFiles: [
      { path: 'deck.qmd', content: THEMED_DECK },
      { path: 'styles/deck.scss', content: DECK_SCSS },
      { path: 'styles/partials/_vars.scss', content: '$deck-bg: #abcdef;\n' },
    ],
  });
  await page.goto(server.url);
  await expect.poll(() => bodyBackground(page), { timeout: 30_000 }).toBe('rgb(171, 205, 239)');

  const dir = path.join(server.projectDir, 'styles', 'partials');
  await writeFile(path.join(dir, '_extra.scss'), '$deck-bg: #123456;\n');
  await writeFile(path.join(dir, '_vars.scss'), '$deck-bg: #abcdef;\n@import "extra";\n');
  await expect.poll(() => bodyBackground(page), { timeout: 15_000 }).toBe('rgb(18, 52, 86)');
});

for (const mode of ['single-file', 'project'] as const) {
  test(`${mode}: a .bib beside the document resolves citations on the first render`, async ({ page }) => {
    const files = [
      { path: 'index.qmd', content: '---\ntitle: Cites\nbibliography: refs.bib\n---\n\nAs shown by @knuth84.\n' },
      {
        path: 'refs.bib',
        content: '@book{knuth84, author = {Donald Knuth}, title = {The TeXbook}, year = {1984}}\n',
      },
    ];
    if (mode === 'project') files.push({ path: '_quarto.yml', content: 'project:\n  type: default\n' });
    server = await startPreviewServer({
      fixtureFiles: files,
      ...(mode === 'single-file' ? { targetFile: 'index.qmd' } : {}),
    });
    await page.goto(server.url);
    await expect.poll(() => iframeText(page), { timeout: 30_000 }).toContain('Knuth');
    expect(await ticks(page)).toBe(1);
  });
}

test('one cold render: pulls and their echoes cost one render, and a re-render pulls nothing', async ({ page }) => {
  server = await startPreviewServer({
    targetFile: 'deck.qmd',
    fixtureFiles: [
      { path: 'deck.qmd', content: THEMED_DECK.replace('# Themed', '# Themed\n\nSee @knuth84.').replace('title: Deck', 'title: Deck\nbibliography: refs.bib') },
      { path: 'refs.bib', content: '@book{knuth84, author = {Donald Knuth}, title = {T}, year = {1984}}\n' },
      { path: 'styles/deck.scss', content: DECK_SCSS },
      { path: 'styles/partials/_vars.scss', content: '$deck-bg: #abcdef;\n' },
    ],
  });
  await page.goto(server.url);
  await expect.poll(() => bodyBackground(page), { timeout: 30_000 }).toBe('rgb(171, 205, 239)');
  // Give the sync echoes of the pulled files time to arrive.
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(1);
  const before = await missStats(page);
  expect(before['read.200']).toBeGreaterThanOrEqual(3);

  await page.getByRole('button', { name: /refresh/i }).click();
  await expect.poll(() => ticks(page)).toBe(2);
  expect(await missStats(page)).toEqual(before);
});

test('project: the read set decides which edits re-render', async ({ page }) => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: default\n' },
      { path: 'index.qmd', content: '---\ntitle: Home\nbibliography: refs.bib\n---\n\nSee @a.\n' },
      { path: 'refs.bib', content: '@book{a, author = {Ann A}, title = {A}, year = {2000}}\n' },
      { path: 'other/_metadata.yml', content: 'author: Nobody\n' },
    ],
  });
  await page.goto(server.url);
  await expect.poll(() => iframeText(page), { timeout: 30_000 }).toContain('Ann');
  await page.waitForTimeout(2000);
  let base = await ticks(page);

  // A file in a directory the render never touched: no re-render.
  await writeFile(path.join(server.projectDir, 'other', '_metadata.yml'), 'author: Somebody\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(base);

  // A file the render read: re-renders.
  await writeFile(
    path.join(server.projectDir, 'refs.bib'),
    '@book{a, author = {Bea B}, title = {A}, year = {2000}}\n',
  );
  await expect.poll(() => iframeText(page), { timeout: 15_000 }).toContain('Bea');
  base = await ticks(page);

  // A file created where the render looked and found nothing: re-renders.
  await mkdir(server.projectDir, { recursive: true });
  await writeFile(path.join(server.projectDir, '_metadata.yml'), 'subtitle: Created later\n');
  await expect.poll(() => ticks(page), { timeout: 15_000 }).toBeGreaterThan(base);
  await expect.poll(() => iframeText(page), { timeout: 15_000 }).toContain('Created later');
});
