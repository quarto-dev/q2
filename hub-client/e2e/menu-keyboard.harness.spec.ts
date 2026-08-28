/**
 * Keyboard-interaction spec for the shared Menu component (APG menu-button
 * pattern), driven against the `#/dev/gallery` menu demo. Runs in the
 * no-server visual config because it needs the DevHarness, not screenshots.
 *
 * Covers: open focuses first item, ArrowUp/Down (wrapping, skipping
 * disabled), Home/End, type-ahead, submenu open/close with ArrowRight/
 * ArrowLeft, Enter activates + closes + returns focus, Escape closes +
 * returns focus.
 *
 * Phase 1 deliverable of the UI/UX modernization plan (bd-iguk0hpd).
 */

import { test, expect, type Page } from '@playwright/test';
import { bootHarness } from './helpers/harness';

const TRIGGER = 'button:has-text("Gallery menu")';

async function openMenu(page: Page) {
  await page.click(TRIGGER);
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  return menu;
}

/** The top-level gallery menu (not a submenu). */
function topMenu(page: Page) {
  return page.locator('[role="menu"]', { hasNot: page.locator('[role="menu"]') });
}

test.beforeEach(async ({ page }) => {
  await bootHarness(page, 'gallery', 'text=Component gallery', 'light');
});

test('opens with first item focused; arrows move and wrap, skipping disabled', async ({ page }) => {
  const menu = await openMenu(page);
  // First item focused on open.
  await expect(page.locator('[role="menuitem"]').first()).toBeFocused();

  const items = menu.locator('[role="menuitem"]');
  // Locator order: Open(0), Move to(1), Copy link(2), Duplicate(3),
  // Unavailable(4, disabled), Delete(5). Arrow nav skips the disabled item.
  await page.keyboard.press('ArrowDown');
  await expect(items.nth(1)).toBeFocused(); // Move to
  await page.keyboard.press('ArrowUp');
  await expect(items.first()).toBeFocused(); // wraps back up
  await page.keyboard.press('ArrowUp');
  await expect(items.nth(5)).toBeFocused(); // wraps to Delete (last enabled)
  await page.keyboard.press('ArrowDown');
  await expect(items.first()).toBeFocused(); // wraps to top
});

test('Home/End jump to first/last enabled item', async ({ page }) => {
  const menu = await openMenu(page);
  const items = menu.locator('[role="menuitem"]');
  await page.keyboard.press('End');
  await expect(items.nth(5)).toBeFocused(); // Delete, past the disabled item
  await page.keyboard.press('Home');
  await expect(items.first()).toBeFocused();
});

test('type-ahead focuses the matching item', async ({ page }) => {
  const menu = await openMenu(page);
  const items = menu.locator('[role="menuitem"]');
  await page.keyboard.press('d');
  await expect(items.nth(3)).toBeFocused(); // Duplicate
  // The harness mocks timers; advance past the 500ms type-ahead window so
  // the next keystroke starts a fresh buffer.
  await page.clock.runFor(600);
  await page.keyboard.press('c');
  await expect(items.nth(2)).toBeFocused(); // Copy link
});

test('submenu: ArrowRight opens and focuses first item; ArrowLeft returns', async ({ page }) => {
  const menu = await openMenu(page);
  await page.keyboard.press('ArrowDown'); // Move to
  const parent = menu.locator('[role="menuitem"][aria-haspopup="menu"]');
  await expect(parent).toBeFocused();

  await page.keyboard.press('ArrowRight');
  await expect(parent).toHaveAttribute('aria-expanded', 'true');
  const submenu = page.locator('.qh-submenu [role="menuitem"]');
  await expect(submenu.first()).toBeFocused();
  await expect(submenu.first()).toHaveText('Alpha');

  await page.keyboard.press('ArrowLeft');
  await expect(parent).toHaveAttribute('aria-expanded', 'false');
  await expect(parent).toBeFocused();
});

test('Enter activates the item, closes the menu, returns focus to the trigger', async ({
  page,
}) => {
  await openMenu(page);
  await page.keyboard.press('Enter'); // activates "Open"
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(page.locator(TRIGGER)).toBeFocused();
  await expect(page.getByTestId('menu-last-action')).toHaveText('Last action: open');
});

test('Escape closes the menu and returns focus to the trigger', async ({ page }) => {
  await openMenu(page);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(page.locator(TRIGGER)).toBeFocused();
});

test('keepOpen item stays open and records its action', async ({ page }) => {
  const menu = await openMenu(page);
  await page.keyboard.press('c'); // type-ahead to Copy link
  await page.keyboard.press('Enter');
  await expect(topMenu(page)).toBeVisible();
  await expect(page.getByTestId('menu-last-action')).toHaveText('Last action: copy');
});

test('pointer: clicking an item closes; clicking the trigger toggles', async ({ page }) => {
  const menu = await openMenu(page);
  await menu.locator('[role="menuitem"]', { hasText: 'Duplicate' }).click();
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(page.getByTestId('menu-last-action')).toHaveText('Last action: duplicate');

  // Toggle: open, then click the trigger again to close.
  await openMenu(page);
  await page.click(TRIGGER);
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
});

test('pointer: clicking a disabled item neither activates nor closes', async ({ page }) => {
  const menu = await openMenu(page);
  await menu.locator('[role="menuitem"]', { hasText: 'Unavailable action' }).click({ force: true });
  await expect(topMenu(page)).toBeVisible();
  await expect(page.getByTestId('menu-last-action')).toHaveText('Last action: (none)');
});

test('pointer: clicking outside closes without focusing the trigger', async ({ page }) => {
  await openMenu(page);
  await page.click('h1');
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(page.locator(TRIGGER)).not.toBeFocused();
});
