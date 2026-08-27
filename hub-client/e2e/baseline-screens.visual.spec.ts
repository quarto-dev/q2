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

// capture: 'element' screenshots the surface's container, so a change
// local to that surface can't hide inside a mostly-empty 1280×720 page
// (a sidebar-wide change once measured 0.86% of full-page pixels — under
// the tolerance). 'page' is kept where the selector isn't a container
// (tokens/gallery headings) or the surface has fixed-position children
// outside it (notifications' toasts).
const BASELINE_PAGES: {
  page: string;
  label: string;
  selector: string;
  capture: 'element' | 'page';
  /** Selector masked out of the capture (dynamic content). */
  mask?: string;
}[] = [
  // The projects-home footer renders the live git commit hash — mask it
  // so baselines don't go stale on every commit (the 1% tolerance was
  // silently absorbing the churn).
  { page: 'projects-home', label: 'projects-home', selector: '.projects-home', capture: 'element', mask: '.qh-footer' },
  { page: 'dialog-new-file', label: 'dialog-new-file', selector: '.new-file-dialog', capture: 'element' },
  { page: 'dialog-share', label: 'dialog-share', selector: '.share-dialog', capture: 'element' },
  { page: 'dialog-new-asset', label: 'dialog-new-asset', selector: '.new-asset-dialog', capture: 'element' },
  { page: 'sidebar', label: 'sidebar-sections', selector: '.sidebar-sections', capture: 'element' },
  { page: 'about-tab', label: 'about-tab', selector: '.about-tab', capture: 'element' },
  { page: 'header', label: 'minimal-header', selector: '.minimal-header', capture: 'element' },
  { page: 'notifications', label: 'notifications', selector: '.ephemeral-session-banner', capture: 'page' },
  { page: 'tokens', label: 'tokens', selector: 'text=Design tokens', capture: 'page' },
  { page: 'gallery', label: 'gallery', selector: 'text=Component gallery', capture: 'page' },
];

for (const { page, label, selector, capture, mask } of BASELINE_PAGES) {
  for (const theme of THEMES) {
    test(`${label} — ${theme} theme`, async ({ page: browserPage }) => {
      await bootHarness(browserPage, page, selector, theme);

      const target =
        capture === 'element' ? browserPage.locator(selector) : browserPage;
      await expect(target).toHaveScreenshot(`${label}-${theme}.png`, {
        // Allow small pixel differences for anti-aliasing variance
        maxDiffPixelRatio: 0.01,
        ...(mask ? { mask: [browserPage.locator(mask)] } : {}),
      });
    });
  }
}
