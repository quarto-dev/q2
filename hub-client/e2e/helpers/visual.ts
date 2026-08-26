/**
 * Shared helpers for visual regression and axe-scan specs that run against
 * dev-harness routes (#/dev/...) — no hub server required.
 */

import type { Page } from '@playwright/test';

export const THEMES = ['light', 'dark'] as const;
export type Theme = (typeof THEMES)[number];

/**
 * Fixed clock for deterministic screenshots: ProjectsHome renders
 * relative dates ("yesterday", "Thu") from Date.now(), so every visual
 * spec installs this fixed time before navigation.
 */
export const FIXED_NOW = new Date('2026-08-25T12:00:00.000Z');

/**
 * Force a color theme deterministically. The app reads its colorScheme
 * preference from localStorage at boot (ThemeProvider), so seed it via an
 * init script; then re-assert the class after load in case anything
 * re-applied the stored value mid-boot.
 */
export async function forceTheme(page: Page, theme: Theme): Promise<void> {
  await page.addInitScript((t) => {
    // Full shape required — validatePreferences() falls back to defaults
    // (colorScheme: 'auto') unless every required key is present.
    localStorage.setItem(
      'quarto-hub:preferences',
      JSON.stringify({
        version: 1,
        scrollSyncEnabled: true,
        errorOverlayCollapsed: true,
        colorScheme: t,
        unlockNestingCursor: true,
        richText: true,
      }),
    );
  }, theme);
}

/** Re-assert the theme class post-load and let transitions settle. */
export async function settleTheme(page: Page, theme: Theme): Promise<void> {
  await page.evaluate((t) => {
    document.documentElement.classList.remove('dark', 'light');
    document.documentElement.classList.add(t);
  }, theme);
  // Kill transitions/animations: the theme class flip animates colors over
  // ~200ms, and axe's contrast check (or a screenshot) taken mid-transition
  // measures a blended color — the source of an intermittent 3-vs-4 node
  // drift on projects-home. (Phase 3's animations-off spec config will
  // subsume this.)
  await page.addStyleTag({
    content: '*, *::before, *::after { transition: none !important; animation: none !important; }',
  });
  // Fonts shift both pixels and axe's contrast measurements; under parallel
  // worker load they can land late. Wait them out before asserting.
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(200);
}
