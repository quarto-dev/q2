/**
 * E2E: Files header action row (bd-qhn2raky).
 *
 * The New/Upload buttons — and the conditional Print button — are
 * icon-only buttons that share the header row at equal width and fill it,
 * whether two or three are present. Guards the regression where three
 * text buttons wrapped Upload onto a second row at the default 220px
 * sidebar width.
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
  await bootstrapProjectsHome(page, syncServer);
  const indexDocId = await createProjectOnServer(syncServer, [
    { path: 'index.qmd', content, contentType: 'text' },
  ]);
  await seedProjectInBrowser(page, indexDocId, syncServer, title);
  const row = page.locator('.ph-row', { hasText: title });
  await expect(row).toBeVisible({ timeout: 15000 });
  await row.locator('.ph-row-name').click();
  await expect(page.locator('.new-file-btn')).toBeVisible({ timeout: 30000 });
}

async function expectSingleFullRow(page: Page, selectors: string[]) {
  const boxes = [];
  for (const sel of selectors) {
    const btn = page.locator(sel);
    await expect(btn).toBeVisible();
    boxes.push((await btn.boundingBox())!);
  }
  // All on one row, equal width and height, in the given left-to-right order.
  for (let i = 1; i < boxes.length; i++) {
    expect(Math.abs(boxes[i].y - boxes[0].y)).toBeLessThan(1);
    expect(Math.abs(boxes[i].width - boxes[0].width)).toBeLessThan(1);
    expect(Math.abs(boxes[i].height - boxes[0].height)).toBeLessThan(1);
    expect(boxes[i].x).toBeGreaterThan(boxes[i - 1].x);
  }
  // Together they fill the row: 24px horizontal padding + 6px gaps.
  const headerBox = (await page.locator('.sidebar-header').boundingBox())!;
  const totalWidth = boxes.reduce((sum, b) => sum + b.width, 0);
  expect(totalWidth).toBeCloseTo(headerBox.width - 24 - 6 * (boxes.length - 1), 0);
}

test.describe('Files header action row', () => {
  test.setTimeout(90_000);

  test('New and Upload fill the row at equal width', async ({ page }) => {
    await openEditor(page, 'Two Button Row', '---\ntitle: Two Button Row\n---\n\nHello.\n');

    await expect(page.locator('.print-file-btn')).toHaveCount(0);
    await expectSingleFullRow(page, ['.new-file-btn', '.upload-asset-btn']);
  });

  test('Print, New and Upload share one row at equal width', async ({ page }) => {
    await openEditor(
      page,
      'Three Button Row',
      '---\ntitle: Three Button Row\nformat: q2-preview\n---\n\nHello.\n',
    );

    // Print appears once the format is detected as printable, and is
    // last in the row so the stable New/Upload pair doesn't shift.
    await expect(page.locator('.print-file-btn')).toBeVisible({ timeout: 30000 });
    await expectSingleFullRow(page, [
      '.new-file-btn',
      '.upload-asset-btn',
      '.print-file-btn',
    ]);
  });
});
