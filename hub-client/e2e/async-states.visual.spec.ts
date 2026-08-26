/**
 * Async surfaces: loading / error-with-retry / empty (Phase 3 of the
 * UI/UX modernization plan).
 *
 * Every async surface gets a dev-harness route that pins the state, and
 * each state is asserted functionally (correct copy, working retry) and
 * screenshotted in both themes. axe coverage for these routes lives in
 * the characterization baseline (baseline-a11y.visual.spec.ts) rather
 * than strict scans here: these surfaces render pre-existing chrome
 * (teal primary buttons, editor muted text) whose contrast debt is
 * already baselined on other routes. The full editor shell and the real
 * preview boot can't run in the no-server harness (Phase 0's known
 * limit), so the preview pane boot is covered at its status surface
 * (StatusTab's WASM renderer states).
 */

import { test, expect } from '@playwright/test';
import { THEMES, bootHarness } from './helpers/visual';

test.setTimeout(60_000);

// Screenshots capture the surface's container element with the same 1%
// tolerance baseline-screens uses — it absorbs anti-aliasing variance and
// the ProjectsHome footer's build-hash text (__GIT_COMMIT_HASH__ changes
// every commit).

/** Offscreen action recorder rendered by the stateful harness routes. */
const RECORDER = '[data-testid="async-last-action"]';

/* ---- project list: loading ---- */

for (const theme of THEMES) {
  test(`project list loading — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'projects-home-loading', '.qh-loading', theme);
    const status = page.getByRole('status');
    await expect(status).toContainText('Connecting to project set…');
    await expect(page.locator('.qh-spinner')).toBeAttached();
    await expect(page.locator('.projects-home')).toHaveScreenshot(
      `projects-home-loading-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

/* ---- project list: error with working retry ---- */

for (const theme of THEMES) {
  test(`project list error — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'projects-home-error', '.qh-error', theme);
    await expect(page.locator('.qh-error')).toContainText('Could not reach the sync server.');
    await expect(page.locator('.projects-home')).toHaveScreenshot(
      `projects-home-error-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

test('project list error — retry re-attempts the load', async ({ page }) => {
  await bootHarness(page, 'projects-home-error', '.qh-error', 'light');
  await expect(page.locator(RECORDER)).toHaveText('none');
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(page.locator(RECORDER)).toHaveText('retry');
});

/* ---- project list: empty ---- */

for (const theme of THEMES) {
  test(`project list empty — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'projects-home-empty', '.qh-empty-state', theme);
    const emptyState = page.locator('.qh-empty-state');
    await expect(emptyState.locator('h2')).toHaveText('No projects yet');
    await expect(
      emptyState.getByRole('button', { name: '＋ New project' }),
    ).toBeVisible();
    await expect(
      emptyState.getByRole('button', { name: 'Connect / Import' }),
    ).toBeVisible();
    await expect(page.locator('.projects-home')).toHaveScreenshot(
      `projects-home-empty-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

/* ---- file tree: empty ---- */

for (const theme of THEMES) {
  test(`file tree empty — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'sidebar-empty', '.empty-state', theme);
    await expect(page.locator('.empty-state')).toContainText('No files yet');
    await expect(page.locator('.empty-state')).toContainText(
      'Drop files here or click + to create',
    );
    await expect(page.locator('.sidebar-sections')).toHaveScreenshot(
      `sidebar-empty-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

/* ---- preview pane boot (StatusTab WASM renderer states) ---- */

for (const theme of THEMES) {
  test(`preview boot loading — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'status-tab-loading', '.status-tab', theme);
    await expect(page.locator('.status-tab')).toContainText('Loading WASM…');
    await expect(page.locator('.status-tab')).toHaveScreenshot(
      `status-tab-loading-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test(`preview boot error — ${theme} theme`, async ({ page }) => {
    await bootHarness(page, 'status-tab-error', '.status-tab', theme);
    await expect(page.locator('.status-error')).toContainText(
      'WebAssembly compilation failed',
    );
    await expect(page.locator('.status-tab')).toHaveScreenshot(
      `status-tab-error-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

test('preview boot error — reload retries the boot', async ({ page }) => {
  await bootHarness(page, 'status-tab-error', '.status-tab', 'light');
  await expect(page.locator(RECORDER)).toHaveText('none');
  await page.getByRole('button', { name: 'Reload' }).click();
  await expect(page.locator(RECORDER)).toHaveText('reload');
});
