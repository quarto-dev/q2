/**
 * Visual regression tests for ProjectSetSetup screens.
 *
 * These tests use dev harness routes (#/dev/...) to render the setup
 * component with canned data, then capture screenshots in both light
 * and dark themes for pixel-diff comparison.
 *
 * Run with: npx playwright test --config playwright.visual.config.ts
 */

import { test, expect } from '@playwright/test';

const SETUP_PAGES = [
  { page: 'setup-migration', label: 'migration' },
  { page: 'setup-migration-error', label: 'migration-error' },
  { page: 'setup-fresh', label: 'fresh-setup' },
];

const THEMES = ['dark', 'light'] as const;

for (const { page, label } of SETUP_PAGES) {
  for (const theme of THEMES) {
    test(`${label} — ${theme} theme`, async ({ page: browserPage }) => {
      // Set the theme class before navigating to avoid flash
      await browserPage.addInitScript((t) => {
        document.addEventListener('DOMContentLoaded', () => {
          document.documentElement.classList.remove('dark', 'light');
          document.documentElement.classList.add(t);
        });
      }, theme);

      // Deterministic screenshots via the app's global reduced-motion
      // rule (ui.css, Phase 3) — same mechanism bootHarness uses.
      await browserPage.emulateMedia({ reducedMotion: 'reduce' });

      await browserPage.goto(`/#/dev/${page}`);

      // Wait for the setup modal to be visible
      await browserPage.waitForSelector('.setup-modal', { timeout: 10000 });

      // Force the theme class (in case ThemeProvider overrode our init script)
      await browserPage.evaluate((t) => {
        document.documentElement.classList.remove('dark', 'light');
        document.documentElement.classList.add(t);
      }, theme);

      // Small delay for CSS transitions to settle
      await browserPage.waitForTimeout(200);

      await expect(browserPage).toHaveScreenshot(`${label}-${theme}.png`, {
        // Allow small pixel differences for anti-aliasing variance
        maxDiffPixelRatio: 0.01,
      });
    });
  }
}
