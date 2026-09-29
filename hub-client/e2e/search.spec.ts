/**
 * End-to-end test for full-text search: a project with several files is
 * loaded through the real Automerge sync pipeline, then the search dialog
 * (opened from the FileSidebar's magnifying-glass button) is exercised in
 * the browser.
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

    await bootstrapProjectSet(page);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // Wait for the project to load (preview renders the home page in the
    // q2-preview iframe — the default renderer for plain documents).
    const previewFrame = page.frameLocator('iframe[src*="q2-preview.html"]');
    await expect(previewFrame.locator('body')).toContainText('homepage', { timeout: 30000 });

    // Open the search dialog from the sidebar header.
    await page.getByRole('button', { name: 'Search files' }).click();
    const dialog = page.getByRole('dialog', { name: 'Search files' });
    await expect(dialog).toBeVisible();
    const searchBox = dialog.getByRole('searchbox', { name: 'Search files' });
    await expect(searchBox).toBeFocused();

    // Query a term unique to methods.qmd.
    await searchBox.fill('regression');

    // The matching file appears; non-matching files do not.
    const results = dialog.locator('.search-result');
    await expect(results).toHaveCount(1);
    await expect(dialog.locator('.search-result-name')).toHaveText('methods.qmd');
    // The snippet highlights the matched term.
    await expect(dialog.locator('.search-result-snippet mark')).toContainText('regression');

    // Selecting the result closes the dialog and opens that file in the
    // preview, with the match selected in the editor.
    await results.click();
    await expect(dialog).not.toBeVisible();
    await expect(previewFrame.locator('body')).toContainText('logistic regression', {
      timeout: 30000,
    });
    await expect(page.locator('.file-path')).toHaveText('methods.qmd');

    // The file tree is untouched by searching (all files still listed).
    await expect(page.locator('.file-item')).toHaveCount(4);
  });
});
