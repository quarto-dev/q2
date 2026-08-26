/**
 * Characterization baselines for the hub-client UI/UX modernization
 * (Phase 0). These capture the CURRENT appearance of key surfaces via
 * dev-harness routes, in both themes, BEFORE any token/consistency work
 * lands. Every later visual change must show up here as a deliberate,
 * reviewed diff in the Playwright report.
 *
 * The full editor shell is intentionally not screenshotted here: it needs
 * a live sync connection + Monaco + WASM, which this no-server config
 * avoids by design. Its chrome (header, sidebar sections, dialogs,
 * notifications) is covered surface-by-surface via the routes below.
 *
 * Run with: npm run test:visual
 * Update after an intentional change: npm run test:visual:update
 */

import { test, expect } from '@playwright/test';
import { THEMES, bootHarness } from './helpers/visual';

// bootHarness does two page loads (identity pinning) against a shared dev
// server; under full parallelism the default 30s budget is too tight.
test.setTimeout(60_000);

const BASELINE_PAGES: { page: string; label: string; selector: string }[] = [
  { page: 'projects-home', label: 'projects-home', selector: '.projects-home' },
  { page: 'dialog-new-file', label: 'dialog-new-file', selector: '.new-file-dialog' },
  { page: 'dialog-share', label: 'dialog-share', selector: '.share-dialog' },
  { page: 'dialog-new-asset', label: 'dialog-new-asset', selector: '.new-asset-dialog' },
  { page: 'sidebar', label: 'sidebar-sections', selector: '.sidebar-sections' },
  { page: 'header', label: 'minimal-header', selector: '.minimal-header' },
  { page: 'notifications', label: 'notifications', selector: '.ephemeral-session-banner' },
  { page: 'tokens', label: 'tokens', selector: 'text=Design tokens' },
  { page: 'gallery', label: 'gallery', selector: 'text=Component gallery' },
];

for (const { page, label, selector } of BASELINE_PAGES) {
  for (const theme of THEMES) {
    test(`${label} — ${theme} theme`, async ({ page: browserPage }) => {
      await bootHarness(browserPage, page, selector, theme);

      await expect(browserPage).toHaveScreenshot(`${label}-${theme}.png`, {
        // Allow small pixel differences for anti-aliasing variance
        maxDiffPixelRatio: 0.01,
      });
    });
  }
}
