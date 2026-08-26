/**
 * axe-core scans of interactive gallery states — the open menu and the
 * visible tooltip — which the static per-page baseline scans never
 * render (Phase 2's extension of the Phase 0 axe coverage to every
 * gallery surface).
 *
 * Unlike the characterization baseline (which records existing
 * violations), these surfaces were built to the APG patterns in Phase 1,
 * so the contract is strict: any serious/critical violation inside the
 * interactive element fails. Scans are scoped with .include() so the
 * gallery page's own baselined contrast issues don't leak in.
 */

import { test, expect } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';
import { THEMES, bootHarness, type Theme } from './helpers/visual';

test.setTimeout(60_000);

async function expectNoBlockingViolations(page: Parameters<typeof bootHarness>[0], include: string) {
  const results = await new AxeBuilder({ page }).include(include).analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === 'serious' || v.impact === 'critical',
  );
  expect(blocking.map((v) => `${v.id} (${v.nodes.length} node(s))`)).toEqual([]);
}

for (const theme of THEMES) {
  test(`axe: gallery menu open — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'gallery', 'text=Component gallery', theme as Theme);
    await page.click('button:has-text("Gallery menu")');
    await expect(page.locator('[role="menu"]')).toBeVisible();
    await expectNoBlockingViolations(page, '[role="menu"]');
  });

  test(`axe: gallery tooltip visible — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'gallery', 'text=Component gallery', theme as Theme);
    await page.getByRole('button', { name: 'Hover or focus me' }).focus();
    await expect(page.locator('[role="tooltip"]')).toBeVisible();
    await expectNoBlockingViolations(page, '[role="tooltip"]');
  });
}
