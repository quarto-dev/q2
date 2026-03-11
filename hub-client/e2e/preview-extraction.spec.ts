/**
 * Test preview content extraction helpers.
 *
 * Verifies that we can reliably extract HTML, CSS, and diagnostics
 * from the preview iframe after a project renders.
 */

import { test, expect } from '@playwright/test';
import {
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';
import {
  waitForPreviewRender,
  getPreviewHtml,
  getRenderDiagnostics,
} from './helpers/previewExtraction';

test.describe('Preview Extraction', () => {
  test('should extract HTML and diagnostics from preview', async ({ page }) => {
    const serverUrl = getServerUrl();

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
          'title: Extraction Test',
          '---',
          '',
          '## Section One',
          '',
          'A paragraph with **bold** text.',
        ].join('\n'),
        contentType: 'text',
      },
    ]);

    await page.goto('/');
    await expect(page.locator('body')).toBeVisible();
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/project/${localId}/file/index.qmd`);

    // Wait for render
    await waitForPreviewRender(page);

    // Extract and verify HTML (content has data-sid/data-loc spans)
    const html = await getPreviewHtml(page, 'index.qmd');
    expect(html).toMatch(/Section/);
    expect(html).toMatch(/One/);
    expect(html).toMatch(/<strong[^>]*>.*bold.*<\/strong>/s);

    // Extract and verify diagnostics (should have no errors)
    const diag = await getRenderDiagnostics(page, 'index.qmd');
    expect(diag.success).toBe(true);
    const errors = diag.diagnostics.filter(
      (d) => d.kind.toLowerCase() === 'error',
    );
    expect(errors).toHaveLength(0);
  });
});
