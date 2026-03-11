/**
 * Test that projects created via quarto-sync-client can be loaded
 * in the browser through the full Automerge sync pipeline.
 */

import { test, expect } from '@playwright/test';
import {
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

test.describe('Project Loading', () => {
  test('should load a project created on the server', async ({ page }) => {
    const serverUrl = getServerUrl();

    // Create a simple project on the hub server
    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: 'project:\n  type: default\n',
        contentType: 'text',
      },
      {
        path: 'index.qmd',
        content: [
          '---',
          'title: E2E Test Document',
          '---',
          '',
          '## Hello from E2E',
          '',
          'This is a test paragraph.',
        ].join('\n'),
        contentType: 'text',
      },
    ]);

    // Navigate to app root first (initializes Vite modules)
    await page.goto('/');
    await expect(page.locator('body')).toBeVisible();

    // Seed the project in browser IndexedDB
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    // Navigate to the project file
    await page.goto(`/#/project/${localId}/file/index.qmd`);

    // Wait for the preview iframe to render content (up to 30s for WASM init + render)
    const previewFrame = page.frameLocator('iframe.preview-active');
    await expect(previewFrame.locator('body')).toContainText('Hello from E2E', {
      timeout: 30000,
    });
    await expect(previewFrame.locator('body')).toContainText('test paragraph');
  });
});
