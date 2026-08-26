/**
 * Shared helpers for visual regression and axe-scan specs that run against
 * dev-harness routes (#/dev/...) — no hub server required.
 */

import { expect } from '@playwright/test';
import type { Locator, Page } from '@playwright/test';

export const THEMES = ['light', 'dark'] as const;
export type Theme = (typeof THEMES)[number];

/**
 * Fixed clock for deterministic screenshots: ProjectsHome renders
 * relative dates ("yesterday", "Thu") from Date.now(), so every visual
 * spec installs this fixed time before navigation.
 */
export const FIXED_NOW = new Date('2026-08-25T12:00:00.000Z');

/**
 * Fixed local user identity. The app otherwise generates a random
 * anonymous identity (random name + presence color) on first boot, which
 * makes the header avatar nondeterministic — for screenshots AND for
 * axe's contrast check (some palette colors fail with white text).
 */
const FIXED_IDENTITY = {
  key: 'identity',
  userId: '00000000-0000-4000-8000-000000000001',
  userName: 'Ada Lovelace',
  userColor: '#447099',
  createdAt: '2026-08-01T10:00:00.000Z',
  updatedAt: '2026-08-01T10:00:00.000Z',
};

/**
 * Seed the color-scheme preference before app boot. The app reads its
 * colorScheme preference from localStorage at boot (ThemeProvider); the
 * full shape is required — validatePreferences() falls back to defaults
 * (colorScheme: 'auto') unless every required key is present.
 */
export async function forceTheme(page: Page, theme: Theme): Promise<void> {
  await page.addInitScript((t) => {
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
  // Motion is already deterministic: bootHarness emulates reduced motion
  // and the app's global reduced-motion rule (ui.css, Phase 3) collapses
  // all transitions/animations — including the ~200ms theme-class color
  // flip whose mid-transition blends used to cause intermittent axe
  // node-count drift on projects-home.
  // Fonts shift both pixels and axe's contrast measurements; under parallel
  // worker load they can land late. Wait them out before asserting.
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(200);
}

/**
 * Boot a dev-harness route deterministically: seed the theme, navigate,
 * wait for the app to create its (random) identity in IndexedDB, replace
 * it with the fixed identity, reload so every component re-reads it, then
 * wait for the route's marker selector and settle.
 *
 * `motion` defaults to 'reduce' so screenshots are deterministic via the
 * app's own motion safety (the global prefers-reduced-motion rule in
 * ui.css, Phase 3) — this subsumes the addStyleTag transition-killer
 * Phase 0 used here. Pass 'no-preference' only for motion counter-checks.
 */
export async function bootHarness(
  page: Page,
  route: string,
  selector: string,
  theme: Theme,
  motion: 'reduce' | 'no-preference' = 'reduce',
): Promise<void> {
  await page.clock.install();
  await page.clock.setFixedTime(FIXED_NOW);
  await forceTheme(page, theme);
  await page.emulateMedia({ reducedMotion: motion });

  await page.goto(`/#/dev/${route}`);

  // Wait for the app's boot to create the identity record, then pin it.
  // The dev server can trigger a full-page reload mid-boot (vite re-
  // optimizes dependencies on a cold cache — the CI environment), which
  // destroys the execution context under either of these probes. Both
  // are idempotent, so retry past the reload.
  for (let attempt = 0; ; attempt++) {
    try {
      await page.waitForFunction(
        () =>
          new Promise<boolean>((resolve) => {
            const req = indexedDB.open('quarto-hub');
            req.onsuccess = () => {
              const db = req.result;
              if (!db.objectStoreNames.contains('userSettings')) {
                db.close();
                resolve(false);
                return;
              }
              const tx = db.transaction('userSettings', 'readonly');
              const get = tx.objectStore('userSettings').get('identity');
              get.onsuccess = () => {
                db.close();
                resolve(!!get.result);
              };
              get.onerror = () => {
                db.close();
                resolve(false);
              };
            };
            req.onerror = () => resolve(false);
          }),
        undefined,
        { timeout: 15000 },
      );
      await page.evaluate((identity) => {
        return new Promise<void>((resolve, reject) => {
          const req = indexedDB.open('quarto-hub');
          req.onsuccess = () => {
            const db = req.result;
            const tx = db.transaction('userSettings', 'readwrite');
            tx.objectStore('userSettings').put(identity);
            tx.oncomplete = () => {
              db.close();
              resolve();
            };
            tx.onerror = () => {
              db.close();
              reject(tx.error);
            };
          };
          req.onerror = () => reject(req.error);
        });
      }, FIXED_IDENTITY);
      break;
    } catch (err) {
      if (attempt >= 2 || !/Execution context was destroyed/.test(String(err))) {
        throw err;
      }
      await page.waitForLoadState('load');
    }
  }

  await page.reload();
  await page.waitForSelector(selector, { timeout: 15000 });
  await settleTheme(page, theme);
}

/**
 * WCAG 1.4.10 reflow: at narrow widths the page must not scroll
 * horizontally. Asserts the document, and optionally a surface container
 * whose own scrollWidth is what actually grows when the surface is
 * fixed-position (`.projects-home`) or clips overflow (`.editor-main`) —
 * in those cases the document itself never reports the overflow. A 1px
 * tolerance absorbs sub-pixel rounding.
 */
export async function expectNoHorizontalScroll(
  page: Page,
  surfaceSelector?: string,
): Promise<void> {
  const docOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(docOverflow, 'document has horizontal overflow').toBeLessThanOrEqual(1);
  if (surfaceSelector) {
    const surfaceOverflow = await page
      .locator(surfaceSelector)
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(
      surfaceOverflow,
      `${surfaceSelector} has horizontal overflow`,
    ).toBeLessThanOrEqual(1);
  }
}

/**
 * Assert a locator's rendered box stays fully inside the viewport — the
 * "no clipped controls" half of the narrow-viewport contract. A 1px
 * tolerance absorbs sub-pixel rounding.
 */
export async function expectInsideViewport(
  page: Page,
  locator: Locator,
  label: string,
): Promise<void> {
  const box = await locator.boundingBox();
  const vp = page.viewportSize();
  expect(box, `${label} has no layout box`).not.toBeNull();
  expect(vp, 'page has no viewport size').not.toBeNull();
  expect(box!.x, `${label} is clipped at the left edge`).toBeGreaterThanOrEqual(-1);
  expect(box!.y, `${label} is clipped at the top edge`).toBeGreaterThanOrEqual(-1);
  expect(box!.x + box!.width, `${label} is clipped at the right edge`).toBeLessThanOrEqual(
    vp!.width + 1,
  );
  expect(box!.y + box!.height, `${label} is clipped at the bottom edge`).toBeLessThanOrEqual(
    vp!.height + 1,
  );
}
