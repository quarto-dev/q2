/**
 * End-to-end test for Phase 1 full-text search: a project with several files
 * is loaded through the real Automerge sync pipeline, then the FileSidebar
 * search box is exercised in the browser.
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

test.describe('Full-text search', () => {
  test('finds files by content and opens the selected result', async ({ page }) => {
    const serverUrl = getServerUrl();

    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: 'project:\n  type: default\n',
        contentType: 'text',
      },
      {
        path: 'index.qmd',
        content: ['---', 'title: Home', '---', '', 'Welcome to the project homepage.'].join('\n'),
        contentType: 'text',
      },
      {
        path: 'methods.qmd',
        content: [
          '---',
          'title: Methods',
          '---',
          '',
          'We fit a logistic regression model to the survey data.',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'notes.qmd',
        content: ['---', 'title: Notes', '---', '', 'Buy groceries and water the plants.'].join('\n'),
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // Wait for the project to load (preview renders the home page in the
    // q2-preview iframe — the default renderer for plain documents).
    const previewFrame = page.frameLocator('iframe[src*="q2-preview.html"]');
    await expect(previewFrame.locator('body')).toContainText('homepage', { timeout: 30000 });

    const searchBox = page.getByLabel('Search files');
    await expect(searchBox).toBeVisible();

    // Query a term unique to methods.qmd.
    await searchBox.fill('regression');

    // The matching file appears; non-matching files do not.
    const results = page.locator('.search-result');
    await expect(results).toHaveCount(1);
    await expect(page.locator('.search-result-name')).toHaveText('methods.qmd');
    // The snippet highlights the matched term.
    await expect(page.locator('.search-result-snippet mark')).toContainText('regression');

    // Selecting the result opens that file in the preview.
    await page.locator('.search-result').click();
    await expect(previewFrame.locator('body')).toContainText('logistic regression', {
      timeout: 30000,
    });

    // Clearing the search restores the file tree (all files listed).
    await page.getByLabel('Clear search').click();
    await expect(searchBox).toHaveValue('');
    await expect(page.locator('.file-item')).toHaveCount(4);
  });
});
