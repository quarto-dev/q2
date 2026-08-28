/**
 * Interaction spec for the ProjectsHome dialogs after their conversion to
 * the shared ModalDialog (Phase 1): opened through the real UI (New menu,
 * Connect/Import button), each must present the WCAG dialog contract —
 * role="dialog", aria-modal, labelled by its title, Escape closes, focus
 * returns to the trigger, and the header close button works.
 *
 * Runs in the no-server visual config (DevHarness, not screenshots).
 */

import { test, expect } from '@playwright/test';
import { bootHarness } from './helpers/harness';

test.beforeEach(async ({ page }) => {
  await bootHarness(page, 'projects-home', '.projects-home', 'light');
});

test('New project dialog opens via the New menu and closes with focus return', async ({
  page,
}) => {
  const trigger = page.getByRole('button', { name: '＋ New ▾' });
  await trigger.click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();

  // Keyboard: first item focused; Enter opens the dialog.
  await expect(menu.locator('[role="menuitem"]').first()).toBeFocused();
  await page.keyboard.press('Enter');

  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('aria-modal', 'true');
  await expect(dialog.locator('h2')).toHaveText('New default');
  // Focus is inside the dialog (the name input autofocuses).
  await expect(dialog.locator('#qh-new-name')).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(page.locator('[role="dialog"]')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test('Connect/Import dialog: header close button closes and returns focus', async ({ page }) => {
  const trigger = page.locator('button:has-text("Connect / Import")');
  await trigger.click();

  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expect(dialog.locator('h2')).toHaveText('Add an existing project');

  // Tab cycles within the dialog (focus trap): Shift+Tab from the first
  // focusable wraps to the last.
  await dialog.locator('.close-btn').focus();
  await page.keyboard.press('Shift+Tab');
  const focused = await page.evaluate(() => document.activeElement?.textContent);
  // The Import submit is disabled until a file is chosen, so the last
  // enabled focusable is Cancel.
  expect(focused).toBe('Cancel');

  await dialog.locator('.close-btn').click();
  await expect(page.locator('[role="dialog"]')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});
