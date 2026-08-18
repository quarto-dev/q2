/**
 * Regression spec for quarto-dev/q2#128 (bd-sxx1az83): clicking a
 * cross-document link in the hub-client HTML preview failed in Safari —
 * the preview iframe navigated away and went blank.
 *
 * Root cause: WebKit bug 218086 blocks parent-attached event listeners
 * on a sandboxed frame that lacks `allow-scripts`, so the post-processor's
 * click interception never ran in Safari and the iframe followed the raw
 * href. The fix adds `allow-scripts` to the MorphIframe sandbox (plus a
 * CSP meta neutralizing in-document scripts — see
 * preview-script-blocking.spec.ts).
 *
 * This spec is the one place the fix is verified against real WebKit:
 * the `webkit` project in playwright.config.ts is scoped to it via
 * testMatch. It also runs under chromium as a no-regression check.
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

test.describe('Preview link navigation (q2#128)', () => {
  test('clicking a .qmd link in the preview switches the editor to that file', async ({
    page,
  }) => {
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
          'title: Link Nav Index',
          '---',
          '',
          '## Index page',
          '',
          '[Go to other](other.qmd)',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'other.qmd',
        content: [
          '---',
          'title: Link Nav Other',
          '---',
          '',
          '## Other page',
          '',
          'Other document body text.',
        ].join('\n'),
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // Wait for the preview iframe to render index.qmd (up to 30s for
    // WASM init + render).
    const previewFrame = page.frameLocator('iframe.preview-active');
    await expect(previewFrame.locator('body')).toContainText('Index page', {
      timeout: 30000,
    });

    // Click the rendered cross-doc link inside the preview iframe.
    await previewFrame.getByRole('link', { name: 'Go to other' }).click();

    // The editor switches to other.qmd (SPA navigation, not an iframe
    // navigation)…
    await expect(page).toHaveURL(new RegExp(`/#/p/${localId}/file/other\\.qmd`));

    // …and the preview shows the other document — not a blank frame.
    await expect(previewFrame.locator('body')).toContainText('Other document body text', {
      timeout: 30000,
    });
  });
});
