/**
 * Forced-colors (Windows High Contrast) spec. With forced colors active
 * the platform strips backgrounds, box shadows, and author colors, so any
 * control whose boundary or state is conveyed by background/shadow alone
 * disappears. These specs assert the key surfaces keep visible boundaries
 * (system-color borders) and that the selected row stays distinguishable.
 *
 * A manual Windows High Contrast pass is documented alongside the
 * screen-reader smoke script (hub-client/screen-reader-smoke.md).
 *
 * Phase 2 deliverable of the UI/UX modernization plan.
 */

import { test, expect, type Page, type Locator } from '@playwright/test';
import { bootHarness } from './helpers/visual';

test.setTimeout(60_000);

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ forcedColors: 'active' });
});

/** Computed border summary, e.g. "solid 1px rgb(0, 0, 0)". */
function borderOf(locator: Locator): Promise<string> {
  return locator.evaluate((el) => {
    const s = getComputedStyle(el);
    return `${s.borderTopStyle} ${s.borderTopWidth} ${s.borderTopColor}`;
  });
}

async function expectVisibleBoundary(locator: Locator) {
  const border = await borderOf(locator);
  expect(border.startsWith('none') || border.includes(' 0px ')).toBe(false);
}

test('gallery: buttons and menu keep visible boundaries', async ({ page }) => {
  await bootHarness(page, 'gallery', 'text=Component gallery', 'light');

  // Background-only variants (primary) and hairline variants (outline)
  // must both keep a boundary when backgrounds are stripped.
  await expectVisibleBoundary(page.locator('button.qh-btn.primary').first());
  await expectVisibleBoundary(page.locator('button.qh-btn.outline').first());
  await expectVisibleBoundary(page.locator('button.qh-icon-btn').first());
  await expectVisibleBoundary(page.locator('input.qh-input').first());

  await page.click('button:has-text("Gallery menu")');
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  await expectVisibleBoundary(menu);
});

test('dialog keeps a visible boundary without its shadow', async ({ page }) => {
  await bootHarness(page, 'dialog-share', '.share-dialog', 'light');
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expectVisibleBoundary(dialog);
});

test('sidebar: the selected file row stays distinguishable', async ({ page }) => {
  await bootHarness(page, 'sidebar', '.sidebar-sections', 'light');
  const tree = page.locator('[role="tree"][aria-label="Files"]');
  const active = tree.locator('[role="treeitem"][aria-selected="true"]');
  const other = tree.locator('[role="treeitem"]', { hasText: 'analysis.qmd' });
  const bgOf = (l: Locator) => l.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await bgOf(active)).not.toBe(await bgOf(other));
});

test('tooltip keeps a visible boundary', async ({ page }) => {
  await bootHarness(page, 'gallery', 'text=Component gallery', 'light');
  // The gallery tooltip demo: focus shows the tooltip immediately.
  await page.getByRole('button', { name: 'Hover or focus me' }).focus();
  const tip = page.locator('[role="tooltip"]');
  await expect(tip).toBeVisible();
  await expectVisibleBoundary(tip);
});
