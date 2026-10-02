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

const STATES = ['download-as', 'download-as-progress', 'download-as-done', 'download-as-failed', 'download-as-disabled', 'download-as-only'];
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

test('axe: menu open', async ({ page }) => {
  await bootHarness(page, 'download-as', '.top-bars', 'light');
  await page.locator(BUTTON).click();
  await expect(page.getByRole('menu')).toBeVisible();
  const results = await scan(page);
  const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
  expect(blocking.map((v) => v.id)).toEqual([]);
});
