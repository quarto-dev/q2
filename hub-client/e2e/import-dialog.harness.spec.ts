/**
 * Import button and dialog (document import P5 T7): keyboard contract, focus management, the
 * live region, the described states, and strict axe scans of every state. Runs against the
 * harness routes `#/dev/import-*` (a scripted import service); no pandoc or hub needed.
 */
import { test, expect, type Page } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';
import { THEMES, bootHarness } from './helpers/harness';

test.setTimeout(60_000);

const BUTTON = 'button[aria-label="Import document"]';
const DIALOG = '.import-dialog';

// The new surfaces only: the surrounding top bar carries baselined contrast debt (`.doc-kicker`).
const scan = (page: Page) => new AxeBuilder({ page }).include('.sidebar-header').include(DIALOG).analyze();
const importButton = (page: Page) => page.locator(`${DIALOG} .qh-btn.primary`);

test('the button is keyboard-reachable and opens the picker; the hidden input takes the formats from the table', async ({ page }) => {
  await bootHarness(page, 'import-button', '.file-sidebar', 'light');
  const button = page.locator(BUTTON);
  await expect(button).not.toHaveAttribute('aria-disabled', 'true');
  const accept = await page.getByTestId('import-file-input').getAttribute('accept');
  expect(accept?.split(',')).toEqual(expect.arrayContaining(['.docx', '.pptx', 'application/rtf']));
  await button.focus();
  const chooser = page.waitForEvent('filechooser');
  await page.keyboard.press('Enter');
  const fc = await chooser;
  await fc.setFiles({ name: 'chosen.docx', mimeType: 'application/octet-stream', buffer: Buffer.from('x') });
  await expect(page.getByTestId('import-picked')).toHaveText('chosen.docx');
});

test('the button stays disabled (and says so) until the format table has loaded', async ({ page }) => {
  await bootHarness(page, 'import-button-loading', '.file-sidebar', 'light');
  await expect(page.locator(BUTTON)).toHaveAttribute('aria-disabled', 'true');
});

test('the form: focus starts in the name field, Escape closes, Tab stays inside the dialog', async ({ page }) => {
  await bootHarness(page, 'import-dialog', DIALOG, 'light');
  await expect(importButton(page)).toBeEnabled();
  await expect(page.getByLabel('Name:')).toBeFocused();
  await expect(page.getByLabel('Name:')).toHaveValue('report.qmd');
  await expect(page.getByText('Images will be stored in docs/report_media/')).toBeVisible();
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press('Tab');
    expect(await page.evaluate(() => !!document.activeElement?.closest('.import-dialog'))).toBe(true);
  }
});

test('collision: the proposal advances to "report 2" and a typed colliding name is an error that disables Import', async ({ page }) => {
  await bootHarness(page, 'import-dialog-collision', DIALOG, 'light');
  await expect(page.getByLabel('Name:')).toHaveValue('report 2.qmd');
  await expect(page.getByText('Images will be stored in docs/report 2_media/')).toBeVisible();
  await page.getByLabel('Name:').fill('index.qmd');
  await expect(page.getByText('A file with this name already exists in that folder')).toBeVisible();
  await expect(importButton(page)).toBeDisabled();
});

test('too large and unsupported: Rust\'s text is shown and Import is disabled', async ({ page }) => {
  await bootHarness(page, 'import-dialog-too-large', DIALOG, 'light');
  await expect(page.getByText(/\[Q-24-2\]/)).toBeVisible();
  await expect(importButton(page)).toBeDisabled();
  await bootHarness(page, 'import-dialog-unsupported', DIALOG, 'light');
  await expect(page.getByText(/\[Q-24-1\]/)).toBeVisible();
  await expect(importButton(page)).toBeDisabled();
});

test('importing: progress in the panel and live region, and Cancel is offered', async ({ page }) => {
  await bootHarness(page, 'import-dialog-importing', DIALOG, 'light');
  await expect(page.getByTestId('import-progress')).toContainText('Converting the document…');
  await expect(page.getByRole('status', { name: 'Import progress' })).toHaveText('Converting the document…');
  await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
});

test('first use: the converter download shows its byte counts in the panel but not in the live region', async ({ page }) => {
  await bootHarness(page, 'import-dialog-loading-pandoc', DIALOG, 'light');
  await expect(page.getByTestId('import-progress')).toContainText('6.2 MB of 15.9 MB');
  await expect(page.getByRole('status', { name: 'Import progress' })).toHaveText('Downloading the converter…');
});

test('writing: Cancel is gone and Escape does nothing', async ({ page }) => {
  await bootHarness(page, 'import-dialog-writing', DIALOG, 'light');
  await expect(page.getByTestId('import-progress')).toContainText('Adding the document to the project…');
  await expect(page.getByRole('button', { name: 'Cancel' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(page.locator(DIALOG)).toBeVisible();
});

test('the report: grouped, the Close button takes focus, and the live region announces it', async ({ page }) => {
  await bootHarness(page, 'import-dialog-report', DIALOG, 'light');
  const report = page.getByTestId('import-report');
  await expect(report).toBeVisible();
  await expect(report.getByRole('heading', { name: '2 warnings' })).toBeVisible();
  await expect(report.getByRole('heading', { name: '1 note' })).toBeVisible();
  await expect(page.locator(`${DIALOG} .qh-btn.primary`)).toBeFocused();
  await expect(page.getByRole('status', { name: 'Import progress' })).toContainText('The document was imported.');
});

test('a failed import names the failure first; offline reads like Download as', async ({ page }) => {
  await bootHarness(page, 'import-dialog-failure', DIALOG, 'light');
  await expect(page.getByTestId('import-report')).toContainText('could not be imported');
  await expect(page.getByTestId('import-report')).toContainText('[Q-24-3]');
  await bootHarness(page, 'import-dialog-offline', DIALOG, 'light');
  await expect(page.getByTestId('import-report')).toContainText('You are offline and the converter is not saved yet');
  await bootHarness(page, 'import-dialog-write-failure', DIALOG, 'light');
  const items = page.getByTestId('import-report').getByRole('listitem');
  await expect(items.nth(0)).toContainText('[import-write-failed]');
  await expect(items.nth(1)).toContainText('[import-cleanup-failed]');
});

const STATES = [
  'import-button',
  'import-button-loading',
  'import-dialog',
  'import-dialog-collision',
  'import-dialog-too-large',
  'import-dialog-unsupported',
  'import-dialog-importing',
  'import-dialog-loading-pandoc',
  'import-dialog-writing',
  'import-dialog-report',
  'import-dialog-failure',
  'import-dialog-offline',
  'import-dialog-write-failure',
];
for (const route of STATES) {
  for (const theme of THEMES) {
    test(`axe: ${route} — ${theme} theme`, async ({ page }) => {
      await bootHarness(page, route, route.startsWith('import-button') ? '.file-sidebar' : DIALOG, theme);
      if (route === 'import-dialog-report' || route === 'import-dialog-failure' || route === 'import-dialog-offline' || route === 'import-dialog-write-failure') {
        await expect(page.getByTestId('import-report')).toBeVisible();
      }
      const results = await scan(page);
      const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
      expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    });
  }
}
