/**
 * Viewport matrix specs (Phase 4 of the UI/UX modernization plan):
 * graceful narrow viewports. Small windows and split-screen use must
 * degrade gracefully — no horizontal scroll, no clipped controls — down
 * to 320px, which covers the WCAG 1.4.10 reflow requirement (400% zoom
 * at 1280px).
 *
 * Matrix: 1280 / 900 / 700 / 480 / 320px widths over the editor shell,
 * projects home, and every dialog. The full editor can't boot in the
 * no-server harness (Phase 0's known limit), so the shell is covered by
 * the composed `#/dev/editor-shell*` routes: the real MinimalHeader,
 * SidebarTabs, and `.editor-main view-mode-*` flex rules with placeholder
 * panes for Monaco/iframe.
 *
 * Layout assertions run in the light theme (geometry is
 * theme-independent); screenshots run in both themes at the widths where
 * rendering actually departs from the 1280px baselines captured in
 * baseline-screens.visual.spec.ts.
 */

import { test, expect } from '@playwright/test';
import {
  THEMES,
  bootHarness,
  expectNoHorizontalScroll,
  expectInsideViewport,
} from './helpers/visual';

test.setTimeout(60_000);

const WIDTHS = [1280, 900, 700, 480, 320] as const;
const HEIGHT = 720;
/** Widths that get screenshots (1280 is already baselined). */
const SHOT_WIDTHS = [900, 700, 480, 320] as const;

async function bootAt(
  page: Parameters<typeof bootHarness>[0],
  width: number,
  route: string,
  selector: string,
  theme: (typeof THEMES)[number] = 'light',
  height: number = HEIGHT,
): Promise<void> {
  await page.setViewportSize({ width, height });
  await bootHarness(page, route, selector, theme);
}

/* ---- projects home ---- */

for (const width of WIDTHS) {
  test(`projects home reflows without clipping at ${width}px`, async ({ page }) => {
    await bootAt(page, width, 'projects-home', '.projects-home');
    await expectNoHorizontalScroll(page, '.projects-home');
    await expectInsideViewport(
      page,
      page.getByPlaceholder('Search projects…'),
      'search input',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: 'Connect / Import ▾' }),
      'Connect/Import button',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: '＋ New ▾' }),
      'New menu button',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: 'Account: Ada Lovelace' }),
      'avatar button',
    );
  });
}

for (const width of SHOT_WIDTHS) {
  for (const theme of THEMES) {
    test(`projects home at ${width}px — ${theme} theme`, async ({ page }) => {
      await bootAt(page, width, 'projects-home', '.projects-home', theme);
      await expect(page.locator('.projects-home')).toHaveScreenshot(
        `projects-home-${width}-${theme}.png`,
        { maxDiffPixelRatio: 0.01 },
      );
    });
  }
}

/* ---- menus at narrow widths ---- */

test('project rows keep their identity at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  // The nowrap metadata plus row actions once squeezed the name button
  // to zero width. The name must keep a usable floor (8ch + padding).
  const firstName = page.locator('.qh-row-name').first();
  await expect(firstName).toHaveText('Research Paper');
  const box = await firstName.boundingBox();
  expect(box, 'first row name has no layout box').not.toBeNull();
  expect(box!.width, 'row name squeezed to zero').toBeGreaterThan(50);
});

test('projects home New menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: '＋ New ▾' }).click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'New menu');
});

test('projects home avatar menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: 'Account: Ada Lovelace' }).click();
  const menu = page.locator('.qh-avatar-menu');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'avatar menu');
});

test('file-row context menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'sidebar', '.sidebar-sections');
  const row = page.getByRole('treeitem', { name: /references\.bib/ });
  await row.focus();
  await page.keyboard.press('Shift+F10');
  const menu = page.locator('[role="menu"][aria-label="Actions for references.bib"]');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'context menu');
});

test('peek popover stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page
    .getByRole('button', { name: "Peek — see what's inside Research Paper" })
    .click();
  const peek = page.locator('.qh-peek');
  await expect(peek).toBeVisible();
  await expectInsideViewport(page, peek, 'peek popover');
});

test('row menu submenu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: 'Actions for Research Paper' }).click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  // Submenus open on hover (pointer parity); keyboard uses ArrowRight.
  await menu.locator('.qh-submenu-parent', { hasText: 'Move to collection' }).hover();
  const submenu = page.locator('.qh-submenu');
  await expect(submenu).toBeVisible();
  await expectInsideViewport(page, submenu, 'Move to collection submenu');
});

/* ---- dialogs ---- */

const DIALOG_ROUTES: { route: string; selector: string; label: string }[] = [
  { route: 'dialog-new-file', selector: '.new-file-dialog', label: 'new-file dialog' },
  { route: 'dialog-share', selector: '.share-dialog', label: 'share dialog' },
  { route: 'dialog-new-asset', selector: '.new-asset-dialog', label: 'new-asset dialog' },
];

for (const { route, selector, label } of DIALOG_ROUTES) {
  for (const width of WIDTHS) {
    test(`${label} fits inside the viewport at ${width}px`, async ({ page }) => {
      await bootAt(page, width, route, selector);
      await expectNoHorizontalScroll(page);
      await expectInsideViewport(page, page.locator(selector), label);
    });
  }

  // 480px is where the .qh-dialog max-width rule (100vw - 48px) first
  // bites on the 480–520px-wide dialogs; 320px is the reflow floor.
  for (const width of [480, 320] as const) {
    for (const theme of THEMES) {
      test(`${label} at ${width}px — ${theme} theme`, async ({ page }) => {
        await bootAt(page, width, route, selector, theme);
        await expect(page.locator(selector)).toHaveScreenshot(
          `${route}-${width}-${theme}.png`,
          { maxDiffPixelRatio: 0.01 },
        );
      });
    }
  }
}

test('new-asset dialog stays reachable in a short viewport (320×400)', async ({ page }) => {
  await bootAt(page, 320, 'dialog-new-asset', '.new-asset-dialog', 'light', 400);
  await expectInsideViewport(page, page.locator('.new-asset-dialog'), 'new-asset dialog');
  // The footer actions must remain reachable — the dialog scrolls its
  // content internally rather than clipping past the viewport bottom.
  await expectInsideViewport(
    page,
    page.locator('.new-asset-dialog .dialog-actions'),
    'dialog actions',
  );
});

test('projects-home form dialog fits inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: '＋ New ▾' }).click();
  await page.locator('[role="menu"] [role="menuitem"]').first().click();
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expectInsideViewport(page, dialog, 'New project dialog');
});

/* ---- editor shell ---- */

const SHELL_ROUTES = [
  { route: 'editor-shell', mode: 'both' },
  { route: 'editor-shell-markup', mode: 'markup' },
  { route: 'editor-shell-preview', mode: 'preview' },
] as const;

for (const { route, mode } of SHELL_ROUTES) {
  for (const width of WIDTHS) {
    test(`editor shell (${mode} mode) reflows without clipping at ${width}px`, async ({
      page,
    }) => {
      await bootAt(page, width, route, '.editor-main');
      // .editor-main (overflow: hidden) reports internal flex clipping
      // via scrollWidth; .editor-container never sees it.
      await expectNoHorizontalScroll(page, '.editor-main');
      // .header-left can collapse to zero flex width while its buttons
      // paint on, ending up *under* .header-right — an internal overlap
      // no viewport assertion catches. Its own scrollWidth reports it.
      await expectNoHorizontalScroll(page, '.header-left');
      // Header controls stay reachable.
      await expectInsideViewport(
        page,
        page.getByRole('button', { name: 'Fullscreen preview' }),
        'preview button',
      );
      await expectInsideViewport(
        page,
        page.locator('.connection-indicator'),
        'connection indicator',
      );
      // No pane is clipped past the viewport's right edge.
      await expectInsideViewport(page, page.locator('.sidebar-sections'), 'sidebar');
      await expectInsideViewport(page, page.locator('.editor-pane'), 'editor pane');
      await expectInsideViewport(page, page.locator('.preview-pane'), 'preview pane');
    });
  }
}

for (const width of SHOT_WIDTHS) {
  for (const theme of THEMES) {
    test(`editor shell at ${width}px — ${theme} theme`, async ({ page }) => {
      await bootAt(page, width, 'editor-shell', '.editor-main', theme);
      await expect(page.locator('.editor-container')).toHaveScreenshot(
        `editor-shell-${width}-${theme}.png`,
        { maxDiffPixelRatio: 0.01 },
      );
    });
  }
}

/* ---- sidebar (fixed-width surface) ---- */

test('sidebar rows truncate without internal scroll at 320px', async ({ page }) => {
  await bootAt(page, 320, 'sidebar', '.sidebar-sections');
  await expectNoHorizontalScroll(page, '.sidebar-sections');
});

for (const theme of THEMES) {
  test(`sidebar at 320px — ${theme} theme`, async ({ page }) => {
    await bootAt(page, 320, 'sidebar', '.sidebar-sections', theme);
    await expect(page.locator('.sidebar-sections')).toHaveScreenshot(
      `sidebar-320-${theme}.png`,
      { maxDiffPixelRatio: 0.01 },
    );
  });
}

/* ---- notifications ---- */

for (const width of [480, 320] as const) {
  test(`notifications stay inside the viewport at ${width}px`, async ({ page }) => {
    await bootAt(page, width, 'notifications', '.ephemeral-session-banner');
    await expectInsideViewport(page, page.locator('.toast'), 'toast');
    await expectInsideViewport(
      page,
      page.locator('.update-available-toast'),
      'update toast',
    );
  });

  for (const theme of THEMES) {
    test(`notifications at ${width}px — ${theme} theme`, async ({ page }) => {
      await bootAt(page, width, 'notifications', '.ephemeral-session-banner', theme);
      // Page capture: the toasts are fixed-position, outside the banner.
      await expect(page).toHaveScreenshot(`notifications-${width}-${theme}.png`, {
        maxDiffPixelRatio: 0.01,
      });
    });
  }
}
