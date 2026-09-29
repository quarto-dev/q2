/**
 * E2E: Files header action row (bd-qhn2raky).
 *
 * The New/Upload buttons (plus New folder and Search) are icon-only
 * buttons that share one compact, left-aligned header row. Guards the
 * regression where text buttons wrapped Upload onto a second row at the
 * default 220px sidebar width. The conditional Print button used to
 * live here too; it now sits in the document top bar beside the
 * fullscreen button, so the header row no longer changes with format.
 */

import { test, expect, type Page } from '@playwright/test';
import {
  bootstrapProjectsHome,
  createProjectOnServer,
  getServerUrl,
  seedProjectInBrowser,
} from './helpers/projectFactory';

async function openEditor(page: Page, title: string, content: string) {
  const syncServer = getServerUrl();
  await bootstrapProjectsHome(page);
  const indexDocId = await createProjectOnServer(syncServer, [
    { path: 'index.qmd', content, contentType: 'text' },
  ]);
  await seedProjectInBrowser(page, indexDocId, syncServer, title);
  const row = page.locator('.qh-row', { hasText: title });
  await expect(row).toBeVisible({ timeout: 15000 });
  await row.locator('.qh-row-name').click();
  await expect(page.locator('.new-file-btn')).toBeVisible({ timeout: 30000 });
}

async function expectSingleCompactRow(page: Page, selectors: string[]) {
  const boxes = [];
  for (const sel of selectors) {
    const btn = page.locator(sel);
    await expect(btn).toBeVisible();
    boxes.push((await btn.boundingBox())!);
  }
  // All on one row (no wrapping), equal width and height, in the given
  // left-to-right order.
  for (let i = 1; i < boxes.length; i++) {
    expect(Math.abs(boxes[i].y - boxes[0].y)).toBeLessThan(1);
    expect(Math.abs(boxes[i].width - boxes[0].width)).toBeLessThan(1);
    expect(Math.abs(boxes[i].height - boxes[0].height)).toBeLessThan(1);
    expect(boxes[i].x).toBeGreaterThan(boxes[i - 1].x);
  }
  // Compact and left-aligned: the first button starts at the header's
  // 12px padding edge.
  const headerBox = (await page.locator('.sidebar-header').boundingBox())!;
  expect(Math.abs(boxes[0].x - headerBox.x - 12)).toBeLessThan(1);
}

test.describe('Files header action row', () => {
  test.setTimeout(90_000);

  test('New and Upload share one compact row', async ({ page }) => {
    // `format: q2-html-render` opts out of the q2-preview default
    // (bd-kltzdhle) into the full-DOM iframe, which has no printable
    // version.
    await openEditor(
      page,
      'Two Button Row',
      '---\ntitle: Two Button Row\nformat: q2-html-render\n---\n\nHello.\n',
    );

    await expect(page.locator('.print-btn')).toHaveCount(0);
    await expectSingleCompactRow(page, ['.new-file-btn', '.upload-asset-btn']);
  });

  test('Print lives in the document top bar and leaves the header row unchanged', async ({ page }) => {
    // A plain document (no `format:` key) renders through q2-preview by
    // default (bd-kltzdhle), which is printable.
    await openEditor(page, 'Three Button Row', '---\ntitle: Three Button Row\n---\n\nHello.\n');

    // Print appears in the top bar once the format is detected as
    // printable, beside the fullscreen button — never in the header row.
    const print = page.locator('.document-top-bar .print-btn');
    await expect(print).toBeVisible({ timeout: 30000 });
    await expect(page.locator('.sidebar-header .print-btn')).toHaveCount(0);
    const printBox = (await print.boundingBox())!;
    const fullscreenBox = (await page.locator('.document-top-bar .preview-btn').boundingBox())!;
    expect(Math.abs(printBox.y - fullscreenBox.y)).toBeLessThan(1);
    expect(printBox.x).toBeLessThan(fullscreenBox.x);

    await expectSingleCompactRow(page, ['.new-file-btn', '.upload-asset-btn']);
  });
});
