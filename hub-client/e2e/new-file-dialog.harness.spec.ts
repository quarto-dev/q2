/**
 * Enter-key interaction spec for the New File dialog (GH #635,
 * bd-zcv0iea4), against the stateful `#/dev/dialog-new-file-stateful`
 * harness route (trigger button + created-files record, mirroring the
 * Editor wiring).
 *
 * This is the only tier where the reopen bug reproduces: the browser's
 * default action for an un-prevented Enter keydown is a synthesized
 * click on whatever holds focus once listeners and microtasks have run —
 * by which time ModalDialog's focus restore has moved focus to the
 * trigger button. jsdom performs no keyboard activation, so the unit
 * tests in NewFileDialog.integration.test.tsx can only pin
 * defaultPrevented; these specs pin the observable behavior.
 */

import { test, expect, type Page } from '@playwright/test';
import { bootHarness } from './helpers/harness';

// bootHarness does two page loads (identity pinning) against a shared dev
// server; under full parallelism the default 30s budget is too tight.
test.setTimeout(60_000);

const TRIGGER = 'button.new-file-btn';
const DIALOG = '.qh-dialog.new-file-dialog';

async function openDialog(page: Page) {
  await page.locator(TRIGGER).click();
  await expect(page.locator(DIALOG)).toBeVisible();
  // The dialog focuses the filename input on a 100ms timer; type through
  // the locator (which waits) rather than racing the focus shift.
  await page.locator('#filename').fill('notes.qmd');
}

test.beforeEach(async ({ page }) => {
  await bootHarness(page, 'dialog-new-file-stateful', TRIGGER, 'light');
});

test('Enter in the filename input creates the file once and the dialog stays closed', async ({
  page,
}) => {
  await openDialog(page);
  await page.keyboard.press('Enter');

  const created = page.getByTestId('created-files').locator('li');
  await expect(created).toHaveText(['notes.qmd']);
  // The bug: the keydown's default-action click lands on the focus-restored
  // trigger and reopens the dialog within the same task — so by the time
  // press() resolves, a buggy build already shows the dialog again.
  await expect(page.locator(DIALOG)).toHaveCount(0);
  await expect(page.locator(TRIGGER)).toBeFocused();
});

test('Enter on the Cancel button cancels without creating a file', async ({ page }) => {
  await openDialog(page);
  await page.getByRole('button', { name: 'Cancel' }).focus();
  await page.keyboard.press('Enter');

  await expect(page.locator(DIALOG)).toHaveCount(0);
  await expect(page.getByTestId('created-files').locator('li')).toHaveCount(0);
  await expect(page.locator(TRIGGER)).toBeFocused();
});

test('Enter on the Create button creates the file exactly once and closes', async ({ page }) => {
  await openDialog(page);
  await page.getByRole('button', { name: 'Create' }).focus();
  await page.keyboard.press('Enter');

  await expect(page.getByTestId('created-files').locator('li')).toHaveText(['notes.qmd']);
  await expect(page.locator(DIALOG)).toHaveCount(0);
  await expect(page.locator(TRIGGER)).toBeFocused();
});
