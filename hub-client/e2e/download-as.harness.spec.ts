/**
 * "Download as" control (pandoc-host H5): keyboard contract of the menu button, the described
 * disabled state, and axe scans of every state (strict: the new surfaces carry no baselined
 * debt). Runs against the harness routes `#/dev/download-as*`; no pandoc or hub needed.
 */
import { test, expect, type Page } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';
import { THEMES, bootHarness } from './helpers/harness';

test.setTimeout(60_000);

// The new surfaces only: the surrounding top bar carries baselined contrast debt (`.doc-kicker`,
// see baseline-a11y 'top-bars') that is not this phase's to fix.
const scan = (page: Page) => new AxeBuilder({ page }).include('.download-btn-box').include('.download-only-pane').analyze();

const BUTTON = 'button[aria-label="Download as"]';

test('keyboard: Enter opens, first item focused, Enter downloads and reports in the live region', async ({ page }) => {
  await bootHarness(page, 'download-as', '.top-bars', 'light');
  const button = page.locator(BUTTON);
  await button.focus();
  await page.keyboard.press('Enter');
  const item = page.getByRole('menuitem', { name: 'Word' });
  await expect(item).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(page.getByRole('status', { name: 'Download status' })).toHaveText('Downloaded report.docx.');
  await page.getByRole('button', { name: 'Dismiss' }).click();
  await expect(page.getByTestId('download-status')).toHaveCount(0);
});

test('Escape closes the menu and returns focus to the button', async ({ page }) => {
  await bootHarness(page, 'download-as', '.top-bars', 'light');
  await page.locator(BUTTON).click();
  await expect(page.getByRole('menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(page.locator(BUTTON)).toBeFocused();
});

test('disabled: aria-disabled, described by text, and the explanation is visible in the pane', async ({ page }) => {
  await bootHarness(page, 'download-as-disabled', '.top-bars', 'light');
  const button = page.locator(BUTTON);
  await expect(button).toHaveAttribute('aria-disabled', 'true');
  await expect(button).toHaveAccessibleDescription(/documents with format latex/);
  await button.click({ force: true });
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(page.getByTestId('neither-pane')).toContainText('latex');
});

test('a book chapter\'s menu: "Download book as" and "This chapter only" per format, docx keeps one entry', async ({ page }) => {
  await bootHarness(page, 'download-as-book', '.top-bars', 'light');
  await page.locator(BUTTON).click();
  const items = page.getByRole('menuitem');
  await expect(items).toHaveCount(3);
  await expect(items.nth(0)).toHaveText('Word');
  await expect(items.nth(1)).toHaveText('Download book as EPUB (.epub)');
  await expect(items.nth(2)).toContainText('This chapter only');
  await items.nth(1).click();
  await expect(page.getByRole('status', { name: 'Download status' })).toHaveText('Book downloaded (3 chapters)');
});

test('a book download in progress reads "Rendering chapter i of N: file" in the panel and the live region', async ({ page }) => {
  await bootHarness(page, 'download-as-book-progress', '.top-bars', 'light');
  await expect(page.getByTestId('download-status')).toContainText('Rendering chapter 2 of 3: report.qmd');
  await expect(page.getByRole('status', { name: 'Download status' })).toHaveText('Rendering chapter 2 of 3: report.qmd');
});

const STATES = [
  'download-as',
  'download-as-progress',
  'download-as-done',
  'download-as-failed',
  'download-as-disabled',
  'download-as-only',
  'download-as-book',
  'download-as-book-progress',
];
for (const route of STATES) {
  for (const theme of THEMES) {
    test(`axe: ${route} — ${theme} theme`, async ({ page }) => {
      await bootHarness(page, route, '.top-bars', theme);
      const results = await scan(page);
      const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
      expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    });
  }
}

test('axe: book menu open', async ({ page }) => {
  await bootHarness(page, 'download-as-book', '.top-bars', 'light');
  await page.locator(BUTTON).click();
  await expect(page.getByRole('menu')).toBeVisible();
  const results = await scan(page);
  const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
  expect(blocking.map((v) => v.id)).toEqual([]);
});

test('axe: menu open', async ({ page }) => {
  await bootHarness(page, 'download-as', '.top-bars', 'light');
  await page.locator(BUTTON).click();
  await expect(page.getByRole('menu')).toBeVisible();
  const results = await scan(page);
  const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
  expect(blocking.map((v) => v.id)).toEqual([]);
});
