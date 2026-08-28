/**
 * Motion safety (Phase 3 of the UI/UX modernization plan).
 *
 * A global prefers-reduced-motion rule in ui.css collapses every
 * transition/animation to an effectively-instant 0.01ms for users who
 * request reduced motion, so any motion added later (Phase 5 motion
 * design) is covered automatically. These specs emulate the preference
 * and assert — via computed styles — that no real motion is applied to
 * representative animated/transitioned surfaces.
 *
 * bootHarness emulates reducedMotion: 'reduce' for every visual spec
 * (deterministic screenshots via the app's own rule, subsuming the old
 * addStyleTag hack), so the second describe boots with 'no-preference'
 * as a counter-check: without the preference the same elements DO
 * animate, proving the assertions above are not vacuous.
 */

import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import { bootHarness } from './helpers/harness';

test.setTimeout(60_000);

/** Computed CSS time → effectively instant? (the global rule uses 0.01ms). */
const instant = (t: string) => parseFloat(t) < 0.001;

function transitionDuration(page: Page, selector: string): Promise<string> {
  return page
    .locator(selector)
    .first()
    .evaluate((el) => getComputedStyle(el).transitionDuration);
}

function animationDuration(page: Page, selector: string): Promise<string> {
  return page
    .locator(selector)
    .first()
    .evaluate((el) => getComputedStyle(el).animationDuration);
}

function animationIterationCount(page: Page, selector: string): Promise<string> {
  return page
    .locator(selector)
    .first()
    .evaluate((el) => getComputedStyle(el).animationIterationCount);
}

test.describe('prefers-reduced-motion: reduce', () => {
  test('no transitions on interactive chrome', async ({ page }) => {
    await bootHarness(page, 'sidebar', '.sidebar-sections', 'light');
    // .file-item has `transition: background 0.15s` (FileSidebar.css)
    expect(instant(await transitionDuration(page, '.file-item'))).toBe(true);
    // .section-header has `transition: background 0.15s` (SidebarTabs.css)
    expect(instant(await transitionDuration(page, '.section-header'))).toBe(true);
  });

  test('no transitions on header chrome', async ({ page }) => {
    await bootHarness(page, 'header', '.top-bars', 'light');
    // .qh-icon-btn has `transition: background/color/border-color 0.15s`
    expect(instant(await transitionDuration(page, '.top-bar .qh-icon-btn'))).toBe(true);
  });

  test('no entrance animation on notifications', async ({ page }) => {
    await bootHarness(page, 'notifications', '.ephemeral-session-banner', 'light');
    // .toast has `animation: toast-fade-in 0.2s ease-out` (notifications.css)
    expect(instant(await animationDuration(page, '.toast'))).toBe(true);
    expect(await animationIterationCount(page, '.toast')).toBe('1');
  });

  test('no entrance animations on Phase 5 motion surfaces', async ({ page }) => {
    // Dialog: fade (backdrop) + 4px rise (panel)
    await bootHarness(page, 'dialog-new-file', '.qh-dialog', 'light');
    expect(instant(await animationDuration(page, '.qh-dialog'))).toBe(true);
    expect(instant(await animationDuration(page, '.qh-dialog-backdrop'))).toBe(true);

    // Sidebar section content: rise on expand (mount animation)
    await bootHarness(page, 'sidebar', '.sidebar-sections', 'light');
    expect(instant(await animationDuration(page, '.section-content'))).toBe(true);

    // Menu + tooltip: fade + subtle scale (shared qh-pop-in keyframes)
    await bootHarness(page, 'gallery', 'text=Component gallery', 'light');
    await page.click('button:has-text("Gallery menu")');
    await expect(page.locator('[role="menu"]')).toBeVisible();
    expect(instant(await animationDuration(page, '.qh-menu'))).toBe(true);
    await page.keyboard.press('Escape');
    await page.focus('button:has-text("Hover or focus me")');
    await expect(page.locator('.qh-tooltip')).toBeVisible();
    expect(instant(await animationDuration(page, '.qh-tooltip'))).toBe(true);
  });
});

test.describe('counter-check: no-preference still animates', () => {
  test('transitions apply without the preference', async ({ page }) => {
    await bootHarness(page, 'sidebar', '.sidebar-sections', 'light', 'no-preference');
    expect(instant(await transitionDuration(page, '.file-item'))).toBe(false);
  });

  test('entrance animations apply without the preference', async ({ page }) => {
    await bootHarness(page, 'notifications', '.ephemeral-session-banner', 'light', 'no-preference');
    expect(instant(await animationDuration(page, '.toast'))).toBe(false);
  });

  test('Phase 5 entrance animations apply without the preference', async ({ page }) => {
    await bootHarness(page, 'dialog-new-file', '.qh-dialog', 'light', 'no-preference');
    expect(instant(await animationDuration(page, '.qh-dialog'))).toBe(false);
    expect(instant(await animationDuration(page, '.qh-dialog-backdrop'))).toBe(false);

    await bootHarness(page, 'sidebar', '.sidebar-sections', 'light', 'no-preference');
    expect(instant(await animationDuration(page, '.section-content'))).toBe(false);

    await bootHarness(page, 'gallery', 'text=Component gallery', 'light', 'no-preference');
    await page.click('button:has-text("Gallery menu")');
    await expect(page.locator('[role="menu"]')).toBeVisible();
    expect(instant(await animationDuration(page, '.qh-menu'))).toBe(false);
    await page.keyboard.press('Escape');
    await page.focus('button:has-text("Hover or focus me")');
    await expect(page.locator('.qh-tooltip')).toBeVisible();
    expect(instant(await animationDuration(page, '.qh-tooltip'))).toBe(false);
  });
});
