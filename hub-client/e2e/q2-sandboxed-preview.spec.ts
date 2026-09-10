/**
 * E2E test for the q2-sandboxed-preview format: the full q2-preview
 * renderer running inside the sandboxed iframe against a real hub +
 * WASM render pipeline.
 *
 * Covers the port end-to-end (2026-09-01 sandboxed-preview port plan):
 *  - Phase 0: the format runs the preview pipeline (theme fingerprint →
 *    `<link data-q2-theme>` inside the frame; KaTeX math from the real
 *    renderer).
 *  - Phase 1: the real PreviewRoot renders the document (heading text).
 *  - Phase 2: images resolve through the `__q2_vfs__` service-worker
 *    proxy (manifest-resolved path in the src; decoded natural size).
 *
 * The iframe is the same-origin copy at public/q2-sandboxed-preview/
 * (`VITE_Q2_SANDBOXED_PREVIEW_URL` is set by the test:e2e script and the
 * CI workflow) — same bundle the GitHub Pages origin serves in
 * production, without depending on the last deployed version.
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

// 1x1 red PNG.
const DOT_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

const qmdContent = `---
title: Sandboxed Doc
format: q2-sandboxed-preview
---

# Hello Sandbox

Inline math: $e = mc^2$

![A tiny dot](images/dot.png)
`;

test.describe('q2-sandboxed-preview format', () => {
  test('renders a themed document with math and proxied images in the sandboxed iframe', async ({
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
        path: 'images/dot.png',
        content: DOT_PNG_B64,
        contentType: 'binary',
        mimeType: 'image/png',
      },
      {
        path: 'sandboxed.qmd',
        content: qmdContent,
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(
      `/#/p/${localId}/file/${encodeURIComponent('sandboxed.qmd')}`,
    );

    const iframe = page.frameLocator('iframe[title="q2-sandboxed-preview Renderer"]');

    // Phase 1: the real renderer commits the document — both the title
    // block and the content heading (which carries a data-loc stamp).
    await expect(
      iframe.getByRole('heading', { name: 'Hello Sandbox' }),
    ).toBeVisible({ timeout: 45000 });
    await expect(
      iframe.getByRole('heading', { name: 'Sandboxed Doc' }),
    ).toBeVisible();

    // Phase 0: preview pipeline ran — KaTeX markup (not raw $…$) and the
    // compiled theme applied as <link data-q2-theme> inside the frame.
    await expect(iframe.locator('.katex').first()).toBeVisible();
    await expect(iframe.locator('link[data-q2-theme]')).toHaveCount(1, {
      timeout: 20000,
    });

    // Phase 2: the image goes through the __q2_vfs__ service-worker proxy
    // with the manifest-resolved path, and actually decodes.
    const img = iframe.locator('img[src*="__q2_vfs__"]');
    await expect(img).toBeVisible();
    const src = await img.getAttribute('src');
    expect(src).toContain('images/dot.png');
    await expect
      .poll(async () => img.evaluate((el: HTMLImageElement) => el.naturalWidth), {
        timeout: 20000,
      })
      .toBe(1);
  });
});
