/**
 * Monaco must be bundled, not CDN-loaded (bd-yvz2xqrm).
 *
 * Without loader.config({ monaco }), @monaco-editor/loader fetches Monaco
 * from cdn.jsdelivr.net at runtime, so the source editor sticks at its
 * "Loading..." placeholder whenever the CDN is slow, blocked, or offline.
 * This spec blocks the CDN outright: the editor must still become
 * functional, and no jsDelivr request may be attempted.
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

test.describe('Monaco without CDN', () => {
  test('source editor loads with cdn.jsdelivr.net blocked', async ({ page }) => {
    let cdnRequests = 0;
    await page.route('https://cdn.jsdelivr.net/**', (route) => {
      cdnRequests++;
      return route.abort();
    });

    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: 'index.qmd',
        content: [
          '---',
          'title: Bundled Monaco',
          '---',
          '',
          '## Editor works offline',
        ].join('\n'),
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // The editor itself (not just the preview) must render the document.
    await expect(page.locator('.monaco-editor .view-lines')).toContainText(
      'Editor works offline',
      { timeout: 30000 },
    );
    expect(cdnRequests).toBe(0);
  });
});
