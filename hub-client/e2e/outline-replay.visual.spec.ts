/**
 * Visual coverage for the outline panel's symbol icons and the replay
 * drawer's actor chips (Phase 5, bd-tfsdmytf) — the surfaces whose
 * off-palette token values are re-mapped onto the Posit palette.
 * Element-cropped captures: full-page baseline-screens proved too coarse
 * for sidebar-local changes (a 0.86% pixel diff once slipped under the
 * 1% tolerance), so these screenshot the surface's own element.
 */

import { test, expect } from '@playwright/test';
import { THEMES, bootHarness } from './helpers/visual';

// bootHarness does two page loads (identity pinning) against a shared dev
// server; under full parallelism the default 30s budget is too tight.
test.setTimeout(60_000);

for (const theme of THEMES) {
  test(`outline panel symbol icons — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'sidebar', '.sidebar-sections', theme);
    const outline = page.locator('.outline-panel');
    await expect(outline).toBeVisible();
    await expect(outline).toHaveScreenshot(`outline-panel-${theme}.png`);
  });

  test(`replay drawer actor chips — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'replay', '.replay-drawer', theme);
    await expect(page.locator('[data-testid="replay-fixture"]')).toHaveScreenshot(
      `replay-drawer-${theme}.png`,
    );
  });
}
