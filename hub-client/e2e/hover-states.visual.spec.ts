/**
 * Hover/press micro-state captures (Phase 5, bd-tfsdmytf). The filled-
 * accent buttons' hover/press backgrounds are token-driven (the
 * --btn-accent and --btn-danger families) and every interactive element shares
 * the 100ms ease-out hover transition. These states had no pixel
 * coverage before — these captures are the regression baseline going
 * forward (no before-state to diff; Phase 5 review deck documents the
 * computed color change).
 *
 * Reduced-motion emulation (bootHarness default) collapses the
 * transition, so the captured end-state is deterministic.
 */

import { test, expect } from '@playwright/test';
import { THEMES, bootHarness } from './helpers/visual';

test.setTimeout(60_000);

for (const theme of THEMES) {
  test(`filled button hover — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'gallery', 'text=Component gallery', theme);
    const primary = page.locator('button.qh-btn.primary').first();
    const row = primary.locator('..');
    await primary.hover();
    await expect(row).toHaveScreenshot(`buttons-hover-${theme}.png`);
  });

  test(`filled button press — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'gallery', 'text=Component gallery', theme);
    const danger = page.locator('button.qh-btn.danger').first();
    const row = danger.locator('..');
    // Hold the mouse down: :active applies while pressed.
    await danger.hover();
    await page.mouse.down();
    await expect(row).toHaveScreenshot(`buttons-press-${theme}.png`);
    await page.mouse.up();
  });

  test(`icon button hover — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'gallery', 'text=Component gallery', theme);
    const iconBtn = page.locator('.qh-icon-btn').first();
    const row = iconBtn.locator('..');
    await iconBtn.hover();
    await expect(row).toHaveScreenshot(`icon-btn-hover-${theme}.png`);
  });
}
